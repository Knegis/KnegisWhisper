//! Persisted Windows microphone presentation; dictation remains the source of truth.
use crate::{overlay, state::AppState};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter as _, LogicalSize, Manager as _, PhysicalPosition, WebviewWindow};

const KEY: &str = "dictation.floatingMicrophone";
const EVENT: &str = "overlay-preferences";
const SIZE: f64 = 64.0;
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
    vertical: AtomicBool,
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
    let area = crate::window_placement::resolved_work_area(window, "floating microphone")
        .ok_or("Monitor work area unavailable")?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let width = i32::try_from(size.width).map_err(|e| e.to_string())?;
    let height = i32::try_from(size.height).map_err(|e| e.to_string())?;
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
    state.message.store(false, Ordering::SeqCst);
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
    state.message.store(false, Ordering::SeqCst);
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
        prefs.position = monitor.name().map(|name| SavedPosition {
            monitor: name.clone(),
            x: (f64::from(pos.x) - f64::from(monitor.position().x)) / monitor.scale_factor()
                + if message { 108.0 } else { 0.0 },
            y: (f64::from(pos.y) - f64::from(monitor.position().y)) / monitor.scale_factor()
                + if message { 80.0 } else { 0.0 },
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
) -> Result<(), String> {
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
    let direction = if visible { 1.0 } else { -1.0 };
    window
        .set_size(LogicalSize::new(
            if visible { 280.0 } else { SIZE },
            if visible { 144.0 } else { SIZE },
        ))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(
            f64::from(pos.x) - direction * 108.0 * scale,
            f64::from(pos.y) - direction * 80.0 * scale,
        ))
        .map_err(|e| e.to_string())?;
    clamp(&window)?;
    state.message.store(visible, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
