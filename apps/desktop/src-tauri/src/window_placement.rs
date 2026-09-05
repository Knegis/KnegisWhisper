//! Placement shared by the three small always-on-top windows.
//!
//! Windows uses `MONITORINFO.rcWork`, which excludes the taskbar and docked app
//! bars. Other platforms retain the monitor rectangle Tauri exposes until they
//! have an equally strong work-area API. All final arithmetic is in physical
//! pixels so negative origins and mixed-DPI monitor coordinates remain intact.

use kw_platform::window::WorkArea;
use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

/// UI-space gap from the usable desktop edge.
const MARGIN: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    BottomRight,
    BottomCenter,
}

/// Put a notification at the bottom-right of its monitor's usable area.
pub(crate) fn bottom_right(window: &WebviewWindow, width: f64, height: f64, role: &'static str) {
    place(window, width, height, Anchor::BottomRight, role);
}

/// Put the dictation indicator at bottom-center of its monitor's usable area.
pub(crate) fn bottom_center(window: &WebviewWindow, width: f64, height: f64, role: &'static str) {
    place(window, width, height, Anchor::BottomCenter, role);
}

fn place(window: &WebviewWindow, width: f64, height: f64, anchor: Anchor, role: &'static str) {
    if let Err(err) = window.set_size(LogicalSize::new(width, height)) {
        tracing::debug!(error = %err, role, "could not set the window size");
    }

    let scale = match window.scale_factor() {
        Ok(scale) if scale.is_finite() && scale > 0.0 => scale,
        Ok(scale) => {
            tracing::warn!(scale, role, "the window reported an invalid scale factor");
            return;
        }
        Err(err) => {
            tracing::debug!(error = %err, role, "could not read the window scale factor");
            return;
        }
    };
    let Some((physical_width, physical_height, physical_margin)) =
        physical_metrics(width, height, scale)
    else {
        tracing::warn!(
            width,
            height,
            scale,
            role,
            "the window has invalid placement metrics"
        );
        return;
    };

    let Some(area) = resolved_work_area(window, role) else {
        return;
    };
    let Some(position) = anchored_position(
        area,
        physical_width,
        physical_height,
        physical_margin,
        anchor,
    ) else {
        tracing::warn!(
            ?area,
            physical_width,
            physical_height,
            physical_margin,
            role,
            "the window does not fit inside the monitor work area"
        );
        return;
    };

    if let Err(err) = window.set_position(position) {
        tracing::warn!(error = %err, role, "could not position the window");
    }
}

fn physical_metrics(width: f64, height: f64, scale: f64) -> Option<(u32, u32, u32)> {
    if !width.is_finite()
        || !height.is_finite()
        || !scale.is_finite()
        || width <= 0.0
        || height <= 0.0
        || scale <= 0.0
    {
        return None;
    }

    let size = LogicalSize::new(width, height).to_physical::<u32>(scale);
    let margin = LogicalSize::new(MARGIN, MARGIN).to_physical::<u32>(scale);
    (size.width > 0 && size.height > 0 && margin.width > 0).then_some((
        size.width,
        size.height,
        margin.width,
    ))
}

fn anchored_position(
    area: WorkArea,
    width: u32,
    height: u32,
    margin: u32,
    anchor: Anchor,
) -> Option<PhysicalPosition<i32>> {
    let left = i64::from(area.left);
    let top = i64::from(area.top);
    let right = i64::from(area.right);
    let bottom = i64::from(area.bottom);
    let width = i64::from(width);
    let height = i64::from(height);
    let margin = i64::from(margin);

    let usable_width = right.checked_sub(left)?;
    let usable_height = bottom.checked_sub(top)?;
    let twice_margin = margin.checked_mul(2)?;
    if width.checked_add(twice_margin)? > usable_width
        || height.checked_add(twice_margin)? > usable_height
    {
        return None;
    }

    let x = match anchor {
        Anchor::BottomRight => right.checked_sub(width)?.checked_sub(margin)?,
        Anchor::BottomCenter => left.checked_add(usable_width.checked_sub(width)? / 2)?,
    };
    let y = bottom.checked_sub(height)?.checked_sub(margin)?;

    Some(PhysicalPosition::new(
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
    ))
}

fn resolved_work_area(window: &WebviewWindow, role: &'static str) -> Option<WorkArea> {
    #[cfg(target_os = "windows")]
    {
        match window
            .hwnd()
            .map_err(|err| err.to_string())
            .and_then(|hwnd| {
                kw_platform::window::work_area(kw_platform::window::NativeWindow(hwnd.0 as isize))
                    .map_err(|err| err.to_string())
            }) {
            Ok(area) => return Some(area),
            Err(err) => tracing::warn!(
                error = %err,
                role,
                "could not read the Windows work area; falling back to monitor bounds"
            ),
        }
    }

    monitor_bounds(window, role)
}

fn monitor_bounds(window: &WebviewWindow, role: &'static str) -> Option<WorkArea> {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => monitor,
        Ok(None) => {
            tracing::debug!(
                role,
                "no monitor reported; leaving the window where it was placed"
            );
            return None;
        }
        Err(err) => {
            tracing::debug!(error = %err, role, "could not read the monitor");
            return None;
        }
    };

    let origin = monitor.position();
    let size = monitor.size();
    let width = i32::try_from(size.width).ok()?;
    let height = i32::try_from(size.height).ok()?;
    let right = origin.x.checked_add(width)?;
    let bottom = origin.y.checked_add(height)?;
    WorkArea::from_edges(origin.x, origin.y, right, bottom)
        .inspect_err(|err| tracing::debug!(error = %err, role, "the monitor bounds are invalid"))
        .ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MEASURED_WORK_AREA: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1_280,
        bottom: 752,
    };

    #[test]
    fn notifications_keep_twenty_four_pixels_clear_of_the_measured_work_area() {
        let position =
            anchored_position(MEASURED_WORK_AREA, 392, 92, 24, Anchor::BottomRight).unwrap();
        assert_eq!(position, PhysicalPosition::new(864, 636));
        assert_eq!(MEASURED_WORK_AREA.right - (position.x + 392), 24);
        assert_eq!(MEASURED_WORK_AREA.bottom - (position.y + 92), 24);
    }

    #[test]
    fn overlay_is_bottom_centered_with_the_same_measured_margin() {
        let position =
            anchored_position(MEASURED_WORK_AREA, 280, 96, 24, Anchor::BottomCenter).unwrap();
        assert_eq!(position, PhysicalPosition::new(500, 632));
        assert_eq!(MEASURED_WORK_AREA.bottom - (position.y + 96), 24);
        assert_eq!(
            position.x + 140,
            640,
            "the overlay center is the work-area center"
        );
    }

    #[test]
    fn placement_preserves_a_negative_monitor_origin() {
        let area = WorkArea {
            left: -1_280,
            top: -200,
            right: 0,
            bottom: 552,
        };
        let position = anchored_position(area, 392, 92, 24, Anchor::BottomRight).unwrap();
        assert_eq!(position, PhysicalPosition::new(-416, 436));
    }

    #[test]
    fn logical_margin_and_window_size_scale_together() {
        let metrics = physical_metrics(392.0, 92.0, 1.5).unwrap();
        assert_eq!(metrics, (588, 138, 36));
    }

    #[test]
    fn a_window_too_large_for_both_margins_is_not_placed() {
        let area = WorkArea {
            left: 0,
            top: 0,
            right: 400,
            bottom: 100,
        };
        assert!(anchored_position(area, 392, 92, 24, Anchor::BottomRight).is_none());
        assert!(physical_metrics(0.0, 92.0, 1.0).is_none());
    }
}
