//! Persisted Windows microphone presentation; dictation remains the source of truth.
use crate::{overlay, state::AppState};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter as _, LogicalSize, Manager as _, PhysicalPosition, WebviewWindow};

/// The separate language popup; the microphone keeps `overlay::LABEL`.
pub const LABEL: &str = "dictation-languages";
const KEY: &str = "dictation.floatingMicrophone";
const EVENT: &str = "overlay-preferences";
const SIZE: f64 = 96.0;
const PANEL_WIDTH: f64 = 280.0;
const PANEL_HEIGHT: f64 = 360.0;
const PANEL_X: f64 = (PANEL_WIDTH - SIZE) / 2.0;
const PANEL_Y: f64 = PANEL_HEIGHT - SIZE;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub enabled: bool,
    pub position: Option<SavedPosition>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPosition {
    monitor: String,
    x: f64,
    y: f64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub supported: bool,
    pub enabled: bool,
    pub vertical: bool,
}
#[derive(Clone, Copy)]
struct Drag {
    cursor: PhysicalPosition<f64>,
    origin: PhysicalPosition<i32>,
}
#[derive(Default)]
pub struct Controller {
    prefs: Mutex<Preferences>,
    recording: AtomicBool,
    drag: Mutex<Option<Drag>>,
    writes: tokio::sync::Mutex<()>,
    message: AtomicBool,
    panel_origin: Mutex<Option<PhysicalPosition<i32>>>,
    vertical: AtomicBool,
    #[cfg(windows)]
    input_region: Mutex<Option<Option<kw_platform::window::WorkArea>>>,
    #[cfg(windows)]
    clicks: Mutex<Option<kw_platform::pointer_watch::Watch>>,
    #[cfg(windows)]
    menu_clicks: Mutex<Option<kw_platform::pointer_watch::Watch>>,
}
fn controller(app: &AppHandle) -> tauri::State<'_, Controller> {
    app.state::<Controller>()
}
fn snapshot(app: &AppHandle) -> Result<Snapshot, String> {
    let state = controller(app);
    let prefs = state.prefs.lock().map_err(|e| e.to_string())?;
    Ok(Snapshot {
        supported: cfg!(windows),
        enabled: cfg!(windows) && prefs.enabled,
        vertical: state.vertical.load(Ordering::SeqCst),
    })
}
pub fn initialize(app: &AppHandle) {
    app.manage(Controller::default());
    let db = app.state::<AppState>();
    match tauri::async_runtime::block_on(kw_storage::settings::get::<Preferences>(
        db.db().pool(),
        KEY,
    )) {
        Ok(Some(prefs)) => {
            if let Ok(mut held) = controller(app).prefs.lock() {
                *held = prefs;
            }
        }
        Ok(None) => (),
        Err(error) => tracing::warn!(%error, "could not load microphone preferences"),
    }
}
pub fn recording(app: &AppHandle) -> bool {
    app.try_state::<Controller>()
        .is_some_and(|state| state.recording.load(Ordering::SeqCst))
}
pub fn state_changed(app: &AppHandle, active: bool) {
    let Some(state) = app.try_state::<Controller>() else {
        return;
    };
    state.recording.store(active, Ordering::SeqCst);
    if let Err(error) = reconcile(app, false) {
        tracing::error!(%error, "could not update dictation indicator");
    }
}
fn overlay_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(overlay::LABEL)
        .ok_or_else(|| "The dictation overlay is unavailable".to_owned())
}
fn clamp(window: &WebviewWindow) -> Result<(), String> {
    let mut area = crate::window_placement::resolved_work_area(window, "floating microphone")
        .ok_or("Monitor work area unavailable")?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let width = i32::try_from(size.width).map_err(|e| e.to_string())?;
    let height = i32::try_from(size.height).map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    // The compact window includes transparent space below its buttons. Let that
    // space overlap the taskbar while keeping both clickable parts above it.
    if size == LogicalSize::new(SIZE, SIZE).to_physical::<u32>(scale) {
        let allowance = if at_side_edge(pos.x, size.width, area, scale) {
            4.0
        } else {
            24.0
        };
        area.bottom = area
            .bottom
            .saturating_add((allowance * scale).round() as i32);
    }
    let (x, y) = bounded_position(pos.x, pos.y, width, height, area);
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}
/// Use work-area edges and logical distance so docking behaves across DPI scales.
fn update_orientation(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    let area = crate::window_placement::resolved_work_area(window, "floating microphone")
        .ok_or("Monitor work area unavailable")?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let vertical = at_side_edge(pos.x, size.width, area, scale);
    if controller(app).vertical.swap(vertical, Ordering::SeqCst) != vertical {
        app.emit(EVENT, snapshot(app)?).map_err(|e| e.to_string())?;
    }
    update_input_region(app, window)
}

// Match the fixed microphone and arrow hit targets in styles.css. Keep the
// 96px layout/drag anchor, but remove its transparent padding from OS hit tests.
#[cfg(any(windows, test))]
fn control_region(vertical: bool, scale: f64) -> kw_platform::window::WorkArea {
    let (left, top, right, bottom) = if vertical {
        (32.0, 22.0, 64.0, 90.0)
    } else {
        (22.0, 32.0, 90.0, 64.0)
    };
    kw_platform::window::WorkArea {
        left: (left * scale).floor() as i32,
        top: (top * scale).floor() as i32,
        right: (right * scale).ceil() as i32,
        bottom: (bottom * scale).ceil() as i32,
    }
}

fn update_input_region(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let state = controller(app);
        let bounds = if snapshot(app)?.enabled && !state.message.load(Ordering::SeqCst) {
            Some(control_region(
                state.vertical.load(Ordering::SeqCst),
                window.scale_factor().map_err(|e| e.to_string())?,
            ))
        } else {
            None
        };
        let mut applied = state.input_region.lock().map_err(|e| e.to_string())?;
        if *applied != Some(bounds) {
            let hwnd = window.hwnd().map_err(|e| e.to_string())?;
            kw_platform::window::set_input_region(
                kw_platform::window::NativeWindow(hwnd.0 as isize),
                bounds,
            )
            .map_err(|e| e.to_string())?;
            *applied = Some(bounds);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, window);
    Ok(())
}
fn at_side_edge(x: i32, width: u32, area: kw_platform::window::WorkArea, scale: f64) -> bool {
    let distance = (f64::from(x) - f64::from(area.left))
        .min(f64::from(area.right) - f64::from(x) - f64::from(width));
    distance <= 12.0 * scale
}

fn bounded_position(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    area: kw_platform::window::WorkArea,
) -> (i32, i32) {
    (
        x.clamp(area.left, area.right.saturating_sub(width).max(area.left)),
        y.clamp(area.top, area.bottom.saturating_sub(height).max(area.top)),
    )
}

fn restore(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    let saved = controller(app)
        .prefs
        .lock()
        .map_err(|e| e.to_string())?
        .position
        .clone();
    if let Some(saved) = saved.filter(|p| p.x.is_finite() && p.y.is_finite()) {
        if let Some(monitor) = window
            .available_monitors()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.name() == Some(&saved.monitor))
        {
            let scale = monitor.scale_factor();
            window
                .set_position(PhysicalPosition::new(
                    f64::from(monitor.position().x) + saved.x * scale,
                    f64::from(monitor.position().y) + saved.y * scale,
                ))
                .map_err(|e| e.to_string())?;
            return clamp(window);
        }
    }
    crate::window_placement::bottom_center(window, SIZE, SIZE, "floating microphone");
    // Use the same lower boundary as dragging, including the transparent padding.
    if let Some(area) = crate::window_placement::resolved_work_area(window, "floating microphone") {
        let pos = window.outer_position().map_err(|e| e.to_string())?;
        window
            .set_position(PhysicalPosition::new(pos.x, area.bottom))
            .map_err(|e| e.to_string())?;
        clamp(window)?;
    }
    Ok(())
}
pub fn reconcile(app: &AppHandle, restore_position: bool) -> Result<(), String> {
    let window = overlay_window(app)?;
    let enabled = snapshot(app)?.enabled;
    let active = recording(app);
    if enabled {
        if restore_position {
            window
                .set_size(LogicalSize::new(SIZE, SIZE))
                .map_err(|e| e.to_string())?;
            restore(app, &window)?;
        }
        clamp(&window)?;
        if !controller(app).message.load(Ordering::SeqCst) {
            update_orientation(app, &window)?;
        }
        window.show().map_err(|e| e.to_string())
    } else {
        update_input_region(app, &window)?;
        overlay::set_visible(app, active);
        Ok(())
    }
}
async fn save(app: &AppHandle, prefs: Preferences) -> Result<Snapshot, String> {
    let db = app.state::<AppState>();
    kw_storage::settings::set(db.db().pool(), KEY, &prefs)
        .await
        .map_err(|e| e.to_string())?;
    publish(app, prefs)
}
fn publish(app: &AppHandle, prefs: Preferences) -> Result<Snapshot, String> {
    *controller(app).prefs.lock().map_err(|e| e.to_string())? = prefs;
    let value = snapshot(app)?;
    app.emit(EVENT, &value).map_err(|e| e.to_string())?;
    Ok(value)
}
#[tauri::command]
pub fn overlay_preferences(app: AppHandle) -> Result<Snapshot, String> {
    snapshot(&app)
}
#[tauri::command]
pub async fn overlay_preferences_set(app: AppHandle, enabled: bool) -> Result<Snapshot, String> {
    if !cfg!(windows) && enabled {
        return Err("Floating microphone is available on Windows".into());
    }
    let state = controller(&app);
    let _guard = state.writes.lock().await;
    let mut prefs = state.prefs.lock().map_err(|e| e.to_string())?.clone();
    prefs.enabled = enabled;
    let db = app.state::<AppState>();
    kw_storage::settings::set(db.db().pool(), KEY, &prefs)
        .await
        .map_err(|e| e.to_string())?;
    publish(&app, prefs)?;
    hide_language_menu(&app)?;
    state.message.store(false, Ordering::SeqCst);
    #[cfg(windows)]
    {
        *state.clicks.lock().map_err(|e| e.to_string())? = None;
    }
    *state.drag.lock().map_err(|e| e.to_string())? = None;
    reconcile(&app, true)?;
    snapshot(&app)
}
#[tauri::command]
pub async fn overlay_position_reset(app: AppHandle) -> Result<(), String> {
    let state = controller(&app);
    let _guard = state.writes.lock().await;
    let mut prefs = state.prefs.lock().map_err(|e| e.to_string())?.clone();
    prefs.position = None;
    hide_language_menu(&app)?;
    state.message.store(false, Ordering::SeqCst);
    #[cfg(windows)]
    {
        *state.clicks.lock().map_err(|e| e.to_string())? = None;
    }
    save(&app, prefs).await?;
    reconcile(&app, true)
}
/// Dragging uses physical native cursor coordinates and never activates the window.
#[tauri::command]
pub async fn overlay_drag(
    app: AppHandle,
    window: WebviewWindow,
    phase: String,
) -> Result<(), String> {
    if window.label() != overlay::LABEL || !snapshot(&app)?.enabled {
        return Err("Microphone dragging unavailable".into());
    }
    let state = controller(&app);
    let _guard = state.writes.lock().await;
    if phase == "start" {
        *state.drag.lock().map_err(|e| e.to_string())? = Some(Drag {
            cursor: window.cursor_position().map_err(|e| e.to_string())?,
            origin: window.outer_position().map_err(|e| e.to_string())?,
        });
        return Ok(());
    }
    if !matches!(phase.as_str(), "move" | "end" | "cancel") {
        return Err("Invalid drag phase".into());
    }
    if phase == "cancel" {
        *state.drag.lock().map_err(|e| e.to_string())? = None;
        return Ok(());
    }
    let drag = *state.drag.lock().map_err(|e| e.to_string())?;
    if let Some(drag) = drag {
        let cursor = window.cursor_position().map_err(|e| e.to_string())?;
        window
            .set_position(PhysicalPosition::new(
                f64::from(drag.origin.x) + cursor.x - drag.cursor.x,
                f64::from(drag.origin.y) + cursor.y - drag.cursor.y,
            ))
            .map_err(|e| e.to_string())?;
        clamp(&window)?;
        update_orientation(&app, &window)?;
    }
    if phase == "end" {
        *state.drag.lock().map_err(|e| e.to_string())? = None;
        let monitor = window
            .current_monitor()
            .map_err(|e| e.to_string())?
            .ok_or("Monitor unavailable")?;
        let pos = window.outer_position().map_err(|e| e.to_string())?;
        let mut prefs = state.prefs.lock().map_err(|e| e.to_string())?.clone();
        let message = state.message.load(Ordering::SeqCst);
        if message {
            *state.panel_origin.lock().map_err(|e| e.to_string())? = Some(PhysicalPosition::new(
                pos.x + (PANEL_X * monitor.scale_factor()) as i32,
                pos.y + (PANEL_Y * monitor.scale_factor()) as i32,
            ));
        }
        prefs.position = monitor.name().map(|name| SavedPosition {
            monitor: name.clone(),
            x: (f64::from(pos.x) - f64::from(monitor.position().x)) / monitor.scale_factor()
                + if message { PANEL_X } else { 0.0 },
            y: (f64::from(pos.y) - f64::from(monitor.position().y)) / monitor.scale_factor()
                + if message { PANEL_Y } else { 0.0 },
        });
        save(&app, prefs).await?;
    }
    Ok(())
}
/// Make room for an actionable error bubble while keeping the button anchored.
#[tauri::command]
pub async fn overlay_message(
    app: AppHandle,
    window: WebviewWindow,
    visible: bool,
    menu: Option<bool>,
) -> Result<(), String> {
    if menu == Some(true) {
        if window.label() != overlay::LABEL && (window.label() != LABEL || visible) {
            return Err("Language menu unavailable".into());
        }
        return language_menu(&app, visible).await;
    }
    if window.label() != overlay::LABEL || !snapshot(&app)?.enabled {
        return Ok(());
    }
    let state = controller(&app);
    let _guard = state.writes.lock().await;
    if state.message.load(Ordering::SeqCst) == visible {
        return Ok(());
    }
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let anchor = if visible {
        *state.panel_origin.lock().map_err(|e| e.to_string())? = Some(pos);
        pos
    } else {
        state
            .panel_origin
            .lock()
            .map_err(|e| e.to_string())?
            .take()
            .unwrap_or(pos)
    };
    // Apply the native bounds together on the UI thread, so no paint can occur
    // between resizing, moving, and clamping the transparent overlay.
    let (send, receive) = tokio::sync::oneshot::channel();
    let panel = window.clone();
    window
        .run_on_main_thread(move || {
            let result = position_panel(&panel, visible, anchor, scale);
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    receive.await.map_err(|e| e.to_string())??;
    state.message.store(visible, Ordering::SeqCst);
    update_input_region(&app, &window)?;
    #[cfg(windows)]
    {
        let mut clicks = state.clicks.lock().map_err(|e| e.to_string())?;
        *clicks = None;
        if visible {
            let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
            *clicks = Some(kw_platform::pointer_watch::watch(send).map_err(|e| e.to_string())?);
            let window = window.clone();
            tauri::async_runtime::spawn(async move {
                while let Some((x, y)) = receive.recv().await {
                    if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
                        if i64::from(x) < i64::from(pos.x)
                            || i64::from(y) < i64::from(pos.y)
                            || i64::from(x) >= i64::from(pos.x) + i64::from(size.width)
                            || i64::from(y) >= i64::from(pos.y) + i64::from(size.height)
                        {
                            let _ = window.emit("overlay-outside-click", ());
                        }
                    }
                }
            });
        }
    }
    if !visible {
        update_orientation(&app, &window)?;
    }
    Ok(())
}

pub fn create_language_popup(app: &AppHandle) -> tauri::Result<()> {
    let popup = tauri::WebviewWindowBuilder::new(
        app,
        LABEL,
        tauri::WebviewUrl::App("index.html#dictation-languages".into()),
    )
    .title("Dictation languages")
    .inner_size(280.0, 272.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focusable(!cfg!(windows))
    .focused(false)
    .visible(false)
    .build()?;
    let _ = overlay::apply_treatments(&popup);
    Ok(())
}
fn hide_language_menu(app: &AppHandle) -> Result<(), String> {
    if let Some(popup) = app.get_webview_window(LABEL) {
        popup.hide().map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        *controller(app)
            .menu_clicks
            .lock()
            .map_err(|e| e.to_string())? = None;
    }
    app.emit("overlay-language-menu", false)
        .map_err(|e| e.to_string())
}
async fn language_menu(app: &AppHandle, visible: bool) -> Result<(), String> {
    let state = controller(app);
    let _guard = state.writes.lock().await;
    if !visible || !snapshot(app)?.enabled {
        return hide_language_menu(app);
    }
    let mic = overlay_window(app)?;
    let popup = app
        .get_webview_window(LABEL)
        .ok_or("Language menu unavailable")?;
    let pos = mic.outer_position().map_err(|e| e.to_string())?;
    let scale = mic.scale_factor().map_err(|e| e.to_string())?;
    // Only move the popup. The microphone's native window never changes bounds.
    let above = f64::from(pos.y) - 224.0 * scale;
    let y = match crate::window_placement::resolved_work_area(&mic, "language popup") {
        Some(area) if above < f64::from(area.top) => f64::from(pos.y) + 74.0 * scale,
        _ => above,
    };
    popup
        .set_position(PhysicalPosition::new(f64::from(pos.x) - 92.0 * scale, y))
        .map_err(|e| e.to_string())?;
    clamp(&popup)?;
    popup.show().map_err(|e| e.to_string())?;
    app.emit("overlay-language-menu", true)
        .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
        *state.menu_clicks.lock().map_err(|e| e.to_string())? =
            Some(kw_platform::pointer_watch::watch(send).map_err(|e| e.to_string())?);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some((x, y)) = receive.recv().await {
                if !contains_point(&popup, x, y) && !contains_point(&mic, x, y) {
                    let _ = hide_language_menu(&app);
                    break;
                }
            }
        });
    }
    Ok(())
}
#[cfg(windows)]
fn contains_point(window: &WebviewWindow, x: i32, y: i32) -> bool {
    match (window.outer_position(), window.outer_size()) {
        (Ok(pos), Ok(size)) => {
            if window.label() == overlay::LABEL {
                let state = controller(window.app_handle());
                if let Ok(applied) = state.input_region.lock() {
                    if let Some(Some(region)) = *applied {
                        let local_x = i64::from(x) - i64::from(pos.x);
                        let local_y = i64::from(y) - i64::from(pos.y);
                        return local_x >= i64::from(region.left)
                            && local_y >= i64::from(region.top)
                            && local_x < i64::from(region.right)
                            && local_y < i64::from(region.bottom);
                    }
                };
            }
            i64::from(x) >= i64::from(pos.x)
                && i64::from(y) >= i64::from(pos.y)
                && i64::from(x) < i64::from(pos.x) + i64::from(size.width)
                && i64::from(y) < i64::from(pos.y) + i64::from(size.height)
        }
        _ => false,
    }
}

fn position_panel(
    window: &WebviewWindow,
    visible: bool,
    anchor: PhysicalPosition<i32>,
    scale: f64,
) -> Result<(), String> {
    window
        .set_size(LogicalSize::new(
            if visible { PANEL_WIDTH } else { SIZE },
            if visible { PANEL_HEIGHT } else { SIZE },
        ))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(
            f64::from(anchor.x) - if visible { PANEL_X * scale } else { 0.0 },
            f64::from(anchor.y) - if visible { PANEL_Y * scale } else { 0.0 },
        ))
        .map_err(|e| e.to_string())?;
    clamp(window)?;
    let placed = window.outer_position().map_err(|e| e.to_string())?;
    // Clamping the popup must not move its microphone away from the saved anchor.
    window.emit("overlay-anchor-offset", serde_json::json!({
        "x": if visible { (f64::from(anchor.x) - f64::from(placed.x)) / scale - PANEL_X } else { 0.0 },
        "y": if visible { (f64::from(anchor.y) - f64::from(placed.y)) / scale - PANEL_Y } else { 0.0 },
    })).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_control_region_excludes_padding_and_retains_both_buttons() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for vertical in [false, true] {
                let region = control_region(vertical, scale);
                let contains = |x: f64, y: f64| {
                    x * scale >= f64::from(region.left)
                        && x * scale < f64::from(region.right)
                        && y * scale >= f64::from(region.top)
                        && y * scale < f64::from(region.bottom)
                };
                assert!(contains(48.0, 48.0));
                assert!(if vertical {
                    contains(48.0, 82.0)
                } else {
                    contains(82.0, 48.0)
                });
                for (x, y) in [(4.0, 48.0), (48.0, 4.0), (94.0, 48.0), (48.0, 94.0)] {
                    assert!(!contains(x, y));
                }
            }
        }
    }
    #[test]
    fn preferences_default_to_the_existing_recording_box() {
        let prefs = Preferences::default();
        assert!(!prefs.enabled);
        assert!(prefs.position.is_none());
    }
    #[test]
    fn placement_stays_inside_negative_origin_work_areas_at_multiple_scales() {
        let area = kw_platform::window::WorkArea {
            left: -1920,
            top: -200,
            right: 0,
            bottom: 840,
        };
        assert_eq!(bounded_position(-2000, -300, 64, 64, area), (-1920, -200));
        assert_eq!(bounded_position(500, 900, 128, 128, area), (-128, 712));
        assert_eq!(bounded_position(-900, 100, 96, 96, area), (-900, 100));
    }
    #[test]
    fn side_docking_uses_both_edges_and_scales_the_threshold() {
        let area = kw_platform::window::WorkArea {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        assert!(at_side_edge(-1920, 64, area, 1.0));
        assert!(at_side_edge(-76, 64, area, 1.0));
        assert!(!at_side_edge(-77, 64, area, 1.0));
        assert!(at_side_edge(-1900, 128, area, 2.0));
        assert!(!at_side_edge(-960, 64, area, 1.0));
    }
    #[tokio::test]
    async fn preferences_survive_storage_round_trip() -> anyhow::Result<()> {
        let db = kw_storage::Db::open_in_memory().await?;
        let prefs = Preferences {
            enabled: true,
            position: Some(SavedPosition {
                monitor: "secondary".into(),
                x: 123.5,
                y: 45.0,
            }),
        };
        kw_storage::settings::set(db.pool(), KEY, &prefs).await?;
        let saved = kw_storage::settings::get::<Preferences>(db.pool(), KEY)
            .await?
            .ok_or_else(|| anyhow::anyhow!("missing preferences"))?;
        assert!(saved.enabled);
        assert_eq!(saved.position.map(|p| p.x), Some(123.5));
        Ok(())
    }
}
