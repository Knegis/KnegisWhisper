//! Is a window whose display affinity reads back as excluded actually absent
//! from the pixels screen recorders receive?
//!
//! `GetWindowDisplayAffinity` proves that Windows stored the requested flag. It
//! cannot prove what a recorder sees, because that answer belongs to the
//! recorder's capture path. This probe therefore measures the three paths used
//! by real recorders: GDI `BitBlt` for legacy recorders and screenshot tools,
//! Windows.Graphics.Capture monitor capture for current OBS, Teams, Snipping
//! Tool and Game Bar display capture, and DXGI Desktop Duplication for OBS's
//! DXGI mode and commercial recorders. Windows.Graphics.Capture window capture
//! is also reported, informationally, because it represents sharing one window
//! rather than recording a display. When exclusion holds, `CreateForWindow` on
//! the excluded window produces no frames at all within three seconds rather
//! than a black frame, so the informational `wgc-window` row reports `NO ANSWER`
//! with that note while the same call on the control window delivers frames;
//! this is expected and does not mean that capture path is broken.
//!
//! The default run creates a magenta `EXCLUDED` window and a green `CONTROL`
//! window. It first proves that each capture path can see both, then applies the
//! application's own `exclude_from_capture` function and measures again. A
//! method earns `EFFECTIVE` only if its control remains visible. An unavailable
//! method or an invisible control is `NO ANSWER`, because a capture path that
//! cannot find its own known-positive pixels has measured no absence.
//!
//! ```text
//! cargo run -p kw-platform --example capture_probe
//! cargo run -p kw-platform --example capture_probe -- --target-title "KnegisWhisper meeting"
//! cargo run -p kw-platform --example capture_probe -- --dump target\capture-probe-dump
//! cargo run -p kw-platform --example capture_probe -- --hold-ms 500
//! ```
//!
//! `--target-title` does not modify the application window. It compares captured
//! pixels with the window's own `PrintWindow` rendering, while retaining the
//! green control beside it. `--dump` writes every crop as a 24-bit BMP for human
//! inspection. `--hold-ms` controls the paint-settling message pump.
//!
//! # Results
//!
//! Not recorded here. Runs are recorded in `docs/BENCH_CHECK.md`.

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("capture_probe needs Windows because its capture APIs are Windows-only");
}

#[cfg(target_os = "windows")]
fn main() {
    // SAFETY: this is deliberately the first operation in main, before any
    // coordinate-bearing API or window exists, which is the documented point
    // at which process DPI awareness may be selected.
    let dpi = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };
    if let Err(error) = dpi {
        eprintln!("DPI setup failed: {error}");
        std::process::exit(2);
    }

    match probe::run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("capture_probe could not run: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
mod probe {
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use kw_platform::window::{self, NativeWindow};
    use windows::core::{factory, Interface, PCWSTR};
    use windows::Graphics::Capture::{
        Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
        GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{
        COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, RPC_E_CHANGED_MODE, WPARAM,
    };
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
        D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Dxgi::{
        IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST,
        DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    };
    use windows::Win32::Graphics::Gdi::{
        BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
        DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, InvalidateRect,
        MonitorFromWindow, ReleaseDC, SelectObject, UpdateWindow, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBRUSH, HDC, HGDIOBJ, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, SRCCOPY,
    };
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
        GetClientRect, GetSystemMetrics, GetWindowDisplayAffinity, GetWindowRect, MoveWindow,
        PeekMessageW, RegisterClassW, ShowWindow, TranslateMessage, UnregisterClassW, CS_HREDRAW,
        CS_VREDRAW, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
        WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const WINDOW_WIDTH: i32 = 240;
    const WINDOW_HEIGHT: i32 = 120;
    const WINDOW_GAP: i32 = 24;
    const CROP_INSET: i32 = 4;
    const CHANNEL_TOLERANCE: u8 = 8;
    const VISIBLE_THRESHOLD: f64 = 0.90;
    const ABSENT_THRESHOLD: f64 = 0.05;
    const TARGET_MATCH_MAX_MAD: f64 = 12.0;
    const TARGET_DIFFERS_MIN_MAD: f64 = 40.0;
    const DEFAULT_HOLD_MS: u64 = 300;
    const WGC_INITIAL_WAIT_MS: u64 = 250;
    const WGC_TIMEOUT_MS: u64 = 3_000;
    const DUPLICATION_TIMEOUT_MS: u64 = 2_000;
    const DUPLICATION_REPAINT_MS: u64 = 50;

    const MAGENTA: Fill = Fill {
        r: 255,
        g: 0,
        b: 255,
    };
    const GREEN: Fill = Fill { r: 0, g: 255, b: 0 };

    #[link(name = "user32")]
    extern "system" {
        // `windows` 0.58 places this user32 export under an unrelated XPS
        // feature. Declaring the stable Win32 ABI here avoids enabling a
        // shipping-unrelated feature solely for one dev-only probe.
        fn PrintWindow(hwnd: HWND, hdc: HDC, flags: u32) -> i32;
    }
    const PW_RENDERFULLCONTENT: u32 = 2;

    #[derive(Debug, Clone, Copy)]
    struct Fill {
        r: u8,
        g: u8,
        b: u8,
    }

    impl Fill {
        const fn colorref(self) -> COLORREF {
            COLORREF((self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16))
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct ScreenRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl ScreenRect {
        fn width(self) -> Result<u32, String> {
            u32::try_from(self.right - self.left)
                .map_err(|_| format!("invalid rectangle width: {self:?}"))
        }

        fn height(self) -> Result<u32, String> {
            u32::try_from(self.bottom - self.top)
                .map_err(|_| format!("invalid rectangle height: {self:?}"))
        }

        fn inset(self, pixels: i32) -> Result<Self, String> {
            let inset = Self {
                left: self.left + pixels,
                top: self.top + pixels,
                right: self.right - pixels,
                bottom: self.bottom - pixels,
            };
            let _ = inset.width()?;
            let _ = inset.height()?;
            Ok(inset)
        }
    }

    #[derive(Debug, Clone)]
    struct Crop {
        width: u32,
        height: u32,
        bgra: Vec<u8>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Method {
        Gdi,
        WgcMonitor,
        DxgiDup,
        WgcWindow,
    }

    impl Method {
        const ALL: [Self; 4] = [Self::Gdi, Self::WgcMonitor, Self::DxgiDup, Self::WgcWindow];
        const VERDICT: [Self; 3] = [Self::Gdi, Self::WgcMonitor, Self::DxgiDup];

        const fn name(self) -> &'static str {
            match self {
                Self::Gdi => "gdi",
                Self::WgcMonitor => "wgc-monitor",
                Self::DxgiDup => "dxgi-dup",
                Self::WgcWindow => "wgc-window",
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Class {
        Visible,
        Absent,
        Mixed,
        MatchesWindow,
        Differs,
        NoAnswer,
    }

    impl Class {
        const fn name(self) -> &'static str {
            match self {
                Self::Visible => "VISIBLE",
                Self::Absent => "ABSENT",
                Self::Mixed => "MIXED",
                Self::MatchesWindow => "MATCHES WINDOW",
                Self::Differs => "DIFFERS",
                Self::NoAnswer => "NO ANSWER",
            }
        }
    }

    #[derive(Debug, Clone)]
    struct Reading {
        class: Class,
        match_fraction: f64,
        dominant: Fill,
        note: String,
    }

    #[derive(Debug, Clone)]
    struct Config {
        target_title: Option<String>,
        dump: Option<PathBuf>,
        hold_ms: u64,
    }

    #[derive(Debug)]
    struct D3d {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        winrt_device: Result<IDirect3DDevice, String>,
    }

    #[derive(Debug)]
    struct ProbeWindow {
        hwnd: HWND,
        class_name: Vec<u16>,
        brush: HBRUSH,
    }

    impl Drop for ProbeWindow {
        fn drop(&mut self) {
            // SAFETY: these handles were created and retained by this value;
            // destruction happens once, after all capture objects are dropped.
            unsafe {
                let _ = DestroyWindow(self.hwnd);
                let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), None);
                let _ = DeleteObject(self.brush);
            }
        }
    }

    unsafe extern "system" fn probe_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the procedure forwards the exact arguments supplied by
        // user32 and retains none of their data after the call returns.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    #[derive(Debug, Clone)]
    struct Subject {
        name: &'static str,
        hwnd: HWND,
        rect: ScreenRect,
        expected: Option<Fill>,
    }

    pub(super) fn run() -> Result<(), String> {
        let config = parse_args()?;
        if let Some(directory) = &config.dump {
            fs::create_dir_all(directory).map_err(|error| {
                format!("could not create dump directory {directory:?}: {error}")
            })?;
        }

        // SAFETY: the process requests the multithreaded WinRT apartment before
        // any WinRT object is created. A changed mode means another component
        // initialized COM first; WinRT remains usable and is explicitly allowed.
        let winrt_setup = match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(()),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => Ok(()),
            Err(error) => Err(format!(
                "RoInitialize(RO_INIT_MULTITHREADED) failed: {error}"
            )),
        };

        let d3d = create_d3d(winrt_setup)?;
        match config.target_title.clone() {
            Some(title) => run_target(&config, &d3d, &title),
            None => run_default(&config, &d3d),
        }
    }

    fn parse_args() -> Result<Config, String> {
        let mut target_title = None;
        let mut dump = None;
        let mut hold_ms = DEFAULT_HOLD_MS;
        let mut args = std::env::args().skip(1);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--target-title" => {
                    target_title = Some(
                        args.next()
                            .ok_or_else(|| "--target-title needs a title".to_owned())?,
                    );
                }
                "--dump" => {
                    dump = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| "--dump needs a directory".to_owned())?,
                    ));
                }
                "--hold-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--hold-ms needs milliseconds".to_owned())?;
                    hold_ms = value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --hold-ms value {value:?}: {error}"))?;
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(Config {
            target_title,
            dump,
            hold_ms,
        })
    }

    fn create_d3d(winrt_setup: Result<(), String>) -> Result<D3d, String> {
        let mut device = None;
        let mut context = None;
        // SAFETY: output pointers address live Option slots, the adapter and
        // software module are null for the documented hardware-driver path,
        // and the SDK version matches the headers represented by this crate.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }
        .map_err(|error| format!("D3D11CreateDevice failed: {error}"))?;
        let device = device.ok_or_else(|| "D3D11CreateDevice returned no device".to_owned())?;
        let context = context.ok_or_else(|| "D3D11CreateDevice returned no context".to_owned())?;
        let dxgi: IDXGIDevice = device
            .cast()
            .map_err(|error| format!("ID3D11Device -> IDXGIDevice failed: {error}"))?;
        let winrt_device = winrt_setup.and_then(|()| {
            // SAFETY: dxgi is a live DXGI device from the D3D11 device above.
            let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
                .map_err(|error| format!("CreateDirect3D11DeviceFromDXGIDevice failed: {error}"))?;
            inspectable
                .cast()
                .map_err(|error| format!("IInspectable -> IDirect3DDevice failed: {error}"))
        });
        Ok(D3d {
            device,
            context,
            winrt_device,
        })
    }

    fn run_default(config: &Config, d3d: &D3d) -> Result<(), String> {
        let excluded = create_probe_window("CaptureProbeExcluded", "EXCLUDED", MAGENTA)?;
        let area = work_area_or_primary(excluded.hwnd);
        let required_width = u32::try_from(WINDOW_WIDTH * 2 + WINDOW_GAP + 64)
            .map_err(|_| "probe placement width is invalid".to_owned())?;
        let required_height = u32::try_from(WINDOW_HEIGHT + 64)
            .map_err(|_| "probe placement height is invalid".to_owned())?;
        if area.width()? < required_width || area.height()? < required_height {
            return Err(format!(
                "primary work area {area:?} is too small for two non-overlapping 240x120 probe windows"
            ));
        }
        let x = area.left + 32;
        let y = area.top + 32;
        move_probe_window(excluded.hwnd, x, y)?;
        let control = create_probe_window("CaptureProbeControl", "CONTROL", GREEN)?;
        move_probe_window(control.hwnd, x + WINDOW_WIDTH + WINDOW_GAP, y)?;
        pump(config.hold_ms);

        let subjects = vec![
            Subject {
                name: "excluded",
                hwnd: excluded.hwnd,
                rect: client_screen_rect(excluded.hwnd)?.inset(CROP_INSET)?,
                expected: Some(MAGENTA),
            },
            Subject {
                name: "control",
                hwnd: control.hwnd,
                rect: client_screen_rect(control.hwnd)?.inset(CROP_INSET)?,
                expected: Some(GREEN),
            },
        ];

        let owned_windows = [excluded.hwnd, control.hwnd];
        let phase1 = measure_phase("1", &subjects, &owned_windows, d3d, config, None);
        let exclusion = window::exclude_from_capture(NativeWindow(excluded.hwnd.0 as isize));
        println!("capture_exclusion={exclusion:?}");
        pump(config.hold_ms);
        let phase2 = measure_phase("2", &subjects, &owned_windows, d3d, config, None);
        print_verdicts(&phase1, &phase2);
        Ok(())
    }

    fn run_target(config: &Config, d3d: &D3d, title: &str) -> Result<(), String> {
        let title_wide = wide(title);
        // SAFETY: the title buffer is nul-terminated for the duration of the
        // lookup; a null class name intentionally matches any window class.
        let target = unsafe { FindWindowW(None, PCWSTR(title_wide.as_ptr())) }
            .map_err(|error| format!("FindWindowW({title:?}) failed: {error}"))?;
        let target_outer = window_rect(target)?;
        let target_rect = client_screen_rect(target)?.inset(CROP_INSET)?;
        let mut affinity = 0_u32;
        // SAFETY: target came from FindWindowW and affinity points to a live u32.
        let affinity_note = match unsafe { GetWindowDisplayAffinity(target, &mut affinity) } {
            Ok(()) => format!("{affinity:#010x}"),
            Err(error) => format!("ERROR {error}"),
        };
        println!(
            "target_title={title:?} rect=({},{}-{}, {}) affinity={affinity_note}",
            target_outer.left, target_outer.top, target_outer.right, target_outer.bottom
        );

        let control = create_probe_window("CaptureProbeTargetControl", "CONTROL", GREEN)?;
        let area = work_area_or_primary(target);
        let (control_x, control_y) = control_position(target_outer, area)?;
        move_probe_window(control.hwnd, control_x, control_y)?;
        pump(config.hold_ms);

        let reference = print_window_reference(target, target_rect);
        match &reference {
            Ok(crop) => println!(
                "print_window_reference_flat={} note=PrintWindow independence from affinity is believed, not measured; {}",
                is_single_flat_colour(crop),
                if is_single_flat_colour(crop) {
                    "comparison may be uninformative because PrintWindow returned one flat colour"
                } else {
                    "PrintWindow returned varied pixels"
                }
            ),
            Err(error) => println!(
                "print_window_reference_flat=unknown note=PrintWindow independence from affinity is believed, not measured; {error}"
            ),
        }

        let subjects = vec![
            Subject {
                name: "target",
                hwnd: target,
                rect: target_rect,
                expected: None,
            },
            Subject {
                name: "control",
                hwnd: control.hwnd,
                rect: client_screen_rect(control.hwnd)?.inset(CROP_INSET)?,
                expected: Some(GREEN),
            },
        ];
        let reference_for_measurement = Some(reference.as_ref().map_err(String::as_str));
        let readings = measure_phase(
            "1",
            &subjects,
            &[control.hwnd],
            d3d,
            config,
            reference_for_measurement,
        );
        for method in Method::ALL {
            let target_answer = readings.get(&(method, "target"));
            let control_answer = readings.get(&(method, "control"));
            let verdict = if control_answer.is_none_or(|reading| reading.class != Class::Visible) {
                "NO ANSWER"
            } else if target_answer.is_some_and(|reading| {
                matches!(
                    reading.class,
                    Class::MatchesWindow | Class::Differs | Class::Mixed
                )
            }) {
                "MEASURED"
            } else {
                "NO ANSWER"
            };
            println!("method={} verdict={verdict}", method.name());
        }
        println!(
            "verdict: target mode is informational; recorder methods require visible controls"
        );
        Ok(())
    }

    fn control_position(target: ScreenRect, area: ScreenRect) -> Result<(i32, i32), String> {
        let control_width = u32::try_from(WINDOW_WIDTH)
            .map_err(|_| "control window width is invalid".to_owned())?;
        let control_height = u32::try_from(WINDOW_HEIGHT)
            .map_err(|_| "control window height is invalid".to_owned())?;
        if area.width()? < control_width || area.height()? < control_height {
            return Err(format!(
                "monitor work area {area:?} is too small for the 240x120 control window"
            ));
        }
        let aligned_y = target.top.clamp(area.top, area.bottom - WINDOW_HEIGHT);
        let right = target.right + WINDOW_GAP;
        if right + WINDOW_WIDTH <= area.right {
            return Ok((right, aligned_y));
        }
        let left = target.left - WINDOW_GAP - WINDOW_WIDTH;
        if left >= area.left {
            return Ok((left, aligned_y));
        }
        let aligned_x = target.left.clamp(area.left, area.right - WINDOW_WIDTH);
        let below = target.bottom + WINDOW_GAP;
        if below + WINDOW_HEIGHT <= area.bottom {
            return Ok((aligned_x, below));
        }
        let above = target.top - WINDOW_GAP - WINDOW_HEIGHT;
        if above >= area.top {
            return Ok((aligned_x, above));
        }
        Err(format!(
            "the target {:?} leaves no non-overlapping 240x120 control position inside monitor {:?}",
            target, area
        ))
    }

    fn measure_phase(
        phase: &str,
        subjects: &[Subject],
        owned_windows: &[HWND],
        d3d: &D3d,
        config: &Config,
        target_reference: Option<Result<&Crop, &str>>,
    ) -> HashMap<(Method, &'static str), Reading> {
        let mut readings = HashMap::new();
        for method in Method::ALL {
            let captures = capture_method(method, subjects, owned_windows, d3d);
            for subject in subjects {
                let capture = captures
                    .iter()
                    .find(|(name, _)| *name == subject.name)
                    .map(|(_, result)| result);
                let reading = match capture {
                    Some(Ok(crop)) => {
                        if let Some(directory) = &config.dump {
                            if let Err(error) =
                                dump_crop(directory, phase, method, subject.name, crop)
                            {
                                eprintln!("could not dump {}: {error}", subject.name);
                            }
                        }
                        match subject.expected {
                            Some(fill) => classify_fill(crop, fill),
                            None => match target_reference {
                                Some(Ok(reference)) => classify_target(crop, reference),
                                Some(Err(error)) => no_answer(error),
                                None => no_answer("target comparison has no PrintWindow reference"),
                            },
                        }
                    }
                    Some(Err(error)) => no_answer(error),
                    None => no_answer("capture method returned no row"),
                };
                print_row(phase, method, subject.name, &reading);
                readings.insert((method, subject.name), reading);
            }
        }
        readings
    }

    fn capture_method(
        method: Method,
        subjects: &[Subject],
        owned_windows: &[HWND],
        d3d: &D3d,
    ) -> Vec<(&'static str, Result<Crop, String>)> {
        match method {
            Method::Gdi => subjects
                .iter()
                .map(|subject| (subject.name, capture_gdi(subject.rect)))
                .collect(),
            Method::WgcWindow => subjects
                .iter()
                .map(|subject| {
                    (
                        subject.name,
                        capture_wgc_window(d3d, subject.hwnd, subject.rect),
                    )
                })
                .collect(),
            Method::WgcMonitor => capture_full_for_subjects(subjects, || {
                capture_wgc_monitor(d3d, subjects.first().map(|subject| subject.hwnd))
            }),
            Method::DxgiDup => capture_full_for_subjects(subjects, || {
                capture_desktop_duplication(
                    d3d,
                    subjects.first().map(|subject| subject.rect),
                    owned_windows,
                )
            }),
        }
    }

    fn capture_full_for_subjects<F>(
        subjects: &[Subject],
        capture: F,
    ) -> Vec<(&'static str, Result<Crop, String>)>
    where
        F: FnOnce() -> Result<(Crop, i32, i32), String>,
    {
        match capture() {
            Ok((full, origin_x, origin_y)) => subjects
                .iter()
                .map(|subject| {
                    (
                        subject.name,
                        crop_from(
                            &full,
                            ScreenRect {
                                left: subject.rect.left - origin_x,
                                top: subject.rect.top - origin_y,
                                right: subject.rect.right - origin_x,
                                bottom: subject.rect.bottom - origin_y,
                            },
                        ),
                    )
                })
                .collect(),
            Err(error) => subjects
                .iter()
                .map(|subject| (subject.name, Err(error.clone())))
                .collect(),
        }
    }

    fn print_verdicts(
        phase1: &HashMap<(Method, &'static str), Reading>,
        phase2: &HashMap<(Method, &'static str), Reading>,
    ) {
        let mut effective = 0_u32;
        let mut ineffective = 0_u32;
        let mut no_answer_count = 0_u32;
        for method in Method::VERDICT {
            let p1_excluded = class_for(phase1, method, "excluded");
            let p1_control = class_for(phase1, method, "control");
            let p2_excluded = class_for(phase2, method, "excluded");
            let p2_control = class_for(phase2, method, "control");
            let verdict = if p1_excluded == Some(Class::Visible)
                && p1_control == Some(Class::Visible)
                && p2_excluded == Some(Class::Absent)
                && p2_control == Some(Class::Visible)
            {
                effective += 1;
                "EFFECTIVE"
            } else if p1_excluded == Some(Class::Visible)
                && p1_control == Some(Class::Visible)
                && p2_excluded == Some(Class::Visible)
                && p2_control == Some(Class::Visible)
            {
                ineffective += 1;
                "INEFFECTIVE"
            } else {
                no_answer_count += 1;
                "NO ANSWER"
            };
            println!("method={} verdict={verdict}", method.name());
        }
        println!("method=wgc-window verdict=INFORMATIONAL");
        println!(
            "verdict: {effective} of {} methods EFFECTIVE, {ineffective} INEFFECTIVE, {no_answer_count} NO ANSWER",
            Method::VERDICT.len()
        );
    }

    fn class_for(
        readings: &HashMap<(Method, &'static str), Reading>,
        method: Method,
        window: &'static str,
    ) -> Option<Class> {
        readings.get(&(method, window)).map(|reading| reading.class)
    }

    fn print_row(phase: &str, method: Method, window: &str, reading: &Reading) {
        println!(
            "phase={phase} method={} window={window} class={} match={:.3} dominant=#{:02x}{:02x}{:02x} note={}",
            method.name(),
            reading.class.name(),
            reading.match_fraction,
            reading.dominant.r,
            reading.dominant.g,
            reading.dominant.b,
            reading.note.replace(['\r', '\n'], " ")
        );
    }

    fn no_answer(note: impl ToString) -> Reading {
        Reading {
            class: Class::NoAnswer,
            match_fraction: 0.0,
            dominant: Fill { r: 0, g: 0, b: 0 },
            note: note.to_string(),
        }
    }

    fn classify_fill(crop: &Crop, expected: Fill) -> Reading {
        let pixel_count = crop.bgra.as_chunks::<4>().0.iter().len();
        let matching = crop
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| {
                channel_close(pixel.first().copied(), expected.b)
                    && channel_close(pixel.get(1).copied(), expected.g)
                    && channel_close(pixel.get(2).copied(), expected.r)
            })
            .count();
        let fraction = if pixel_count == 0 {
            0.0
        } else {
            matching as f64 / pixel_count as f64
        };
        let class = if fraction >= VISIBLE_THRESHOLD {
            Class::Visible
        } else if fraction <= ABSENT_THRESHOLD {
            Class::Absent
        } else {
            Class::Mixed
        };
        Reading {
            class,
            match_fraction: fraction,
            dominant: dominant_colour(crop),
            note: format!(
                "expected=#{:02x}{:02x}{:02x}; tolerance={CHANNEL_TOLERANCE}; pixels={pixel_count}",
                expected.r, expected.g, expected.b
            ),
        }
    }

    fn channel_close(actual: Option<u8>, expected: u8) -> bool {
        actual.is_some_and(|value| value.abs_diff(expected) <= CHANNEL_TOLERANCE)
    }

    fn dominant_colour(crop: &Crop) -> Fill {
        let mut counts = HashMap::<u16, usize>::new();
        for pixel in crop.bgra.as_chunks::<4>().0.iter() {
            let Some(&b) = pixel.first() else { continue };
            let Some(&g) = pixel.get(1) else { continue };
            let Some(&r) = pixel.get(2) else { continue };
            let key = (u16::from(r >> 3) << 10) | (u16::from(g >> 3) << 5) | u16::from(b >> 3);
            *counts.entry(key).or_default() += 1;
        }
        let key = counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map_or(0, |(key, _)| key);
        let (red, green, blue, count) = crop.bgra.as_chunks::<4>().0.iter().fold(
            (0_u64, 0_u64, 0_u64, 0_u64),
            |(red, green, blue, count), pixel| {
                let b = pixel.first().copied().unwrap_or(0);
                let g = pixel.get(1).copied().unwrap_or(0);
                let r = pixel.get(2).copied().unwrap_or(0);
                let pixel_key =
                    (u16::from(r >> 3) << 10) | (u16::from(g >> 3) << 5) | u16::from(b >> 3);
                if pixel_key == key {
                    (
                        red + u64::from(r),
                        green + u64::from(g),
                        blue + u64::from(b),
                        count + 1,
                    )
                } else {
                    (red, green, blue, count)
                }
            },
        );
        let average =
            |sum: u64| -> u8 { u8::try_from(sum.checked_div(count).unwrap_or(0)).unwrap_or(0) };
        Fill {
            r: average(red),
            g: average(green),
            b: average(blue),
        }
    }

    fn classify_target(crop: &Crop, reference: &Crop) -> Reading {
        let compared = crop.bgra.len().min(reference.bgra.len()) / 4;
        if crop.width != reference.width || crop.height != reference.height || compared == 0 {
            return no_answer(format!(
                "capture {}x{} and PrintWindow {}x{} cannot be compared",
                crop.width, crop.height, reference.width, reference.height
            ));
        }
        let total_difference: u64 = crop
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .zip(reference.bgra.as_chunks::<4>().0.iter())
            .map(|(actual, expected)| {
                actual
                    .iter()
                    .take(3)
                    .zip(expected.iter().take(3))
                    .map(|(a, b)| u64::from(a.abs_diff(*b)))
                    .sum::<u64>()
            })
            .sum();
        let channels = compared.saturating_mul(3);
        let mad = if channels == 0 {
            255.0
        } else {
            total_difference as f64 / channels as f64
        };
        let class = if mad < TARGET_MATCH_MAX_MAD {
            Class::MatchesWindow
        } else if mad > TARGET_DIFFERS_MIN_MAD {
            Class::Differs
        } else {
            Class::Mixed
        };
        Reading {
            class,
            match_fraction: (1.0 - mad / 255.0).clamp(0.0, 1.0),
            dominant: dominant_colour(crop),
            note: format!("mean_absolute_difference_per_channel={mad:.3}"),
        }
    }

    fn is_single_flat_colour(crop: &Crop) -> bool {
        let Some(first) = crop.bgra.as_chunks::<4>().0.iter().next() else {
            return true;
        };
        crop.bgra
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel.iter().take(3).eq(first.iter().take(3)))
    }

    fn create_probe_window(class: &str, title: &str, fill: Fill) -> Result<ProbeWindow, String> {
        let class_name = wide(class);
        let title_name = wide(title);
        // SAFETY: COLORREF is a value and CreateSolidBrush owns no borrowed data.
        let brush = unsafe { CreateSolidBrush(fill.colorref()) };
        if brush.is_invalid() {
            return Err(format!(
                "CreateSolidBrush for {title} failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        let descriptor = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(probe_window_proc),
            hbrBackground: brush,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: descriptor points to live storage, its strings remain alive
        // in this function and then in ProbeWindow until unregistration.
        let atom = unsafe { RegisterClassW(&raw const descriptor) };
        if atom == 0 {
            // SAFETY: brush is owned here because class registration failed.
            unsafe {
                let _ = DeleteObject(brush);
            }
            return Err(format!(
                "RegisterClassW({class}) failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: the registered class and title buffers are valid and
        // nul-terminated; this top-level popup has no parent, menu or payload.
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_name.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                None,
                None,
            )
        };
        let hwnd = match created {
            Ok(hwnd) => hwnd,
            Err(error) => {
                // SAFETY: class and brush were created above and no window owns them.
                unsafe {
                    let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), None);
                    let _ = DeleteObject(brush);
                }
                return Err(format!("CreateWindowExW({title}) failed: {error}"));
            }
        };
        // SAFETY: hwnd is the live window just created. NOACTIVATE is preserved.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
        }
        Ok(ProbeWindow {
            hwnd,
            class_name,
            brush,
        })
    }

    fn move_probe_window(hwnd: HWND, x: i32, y: i32) -> Result<(), String> {
        // SAFETY: hwnd is a live probe window; dimensions are physical pixels
        // because DPI awareness was established before it was created.
        unsafe { MoveWindow(hwnd, x, y, WINDOW_WIDTH, WINDOW_HEIGHT, true) }
            .map_err(|error| format!("MoveWindow failed: {error}"))?;
        // SAFETY: hwnd remains live and UpdateWindow only requests its paint.
        unsafe {
            let _ = UpdateWindow(hwnd);
        }
        Ok(())
    }

    fn work_area_or_primary(hwnd: HWND) -> ScreenRect {
        if let Ok(area) = window::work_area(NativeWindow(hwnd.0 as isize)) {
            return ScreenRect {
                left: area.left,
                top: area.top,
                right: area.right,
                bottom: area.bottom,
            };
        }
        // SAFETY: these metrics require no pointer arguments and are read only.
        let (width, height) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        ScreenRect {
            left: 0,
            top: 0,
            right: width.max(WINDOW_WIDTH * 2 + WINDOW_GAP),
            bottom: height.max(WINDOW_HEIGHT),
        }
    }

    fn pump(milliseconds: u64) {
        let deadline = Instant::now() + Duration::from_millis(milliseconds);
        loop {
            let mut message = MSG::default();
            // SAFETY: message points to live storage and null HWND requests all
            // messages for this thread; each removed message is dispatched once.
            unsafe {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn window_rect(hwnd: HWND) -> Result<ScreenRect, String> {
        let mut rect = RECT::default();
        // SAFETY: hwnd is supplied by the OS or created by this process and
        // rect points to live writable storage.
        unsafe { GetWindowRect(hwnd, &mut rect) }
            .map_err(|error| format!("GetWindowRect failed: {error}"))?;
        Ok(ScreenRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    fn client_screen_rect(hwnd: HWND) -> Result<ScreenRect, String> {
        let mut client = RECT::default();
        // SAFETY: hwnd is live and client points to writable storage.
        unsafe { GetClientRect(hwnd, &mut client) }
            .map_err(|error| format!("GetClientRect failed: {error}"))?;
        let mut top_left = POINT {
            x: client.left,
            y: client.top,
        };
        let mut bottom_right = POINT {
            x: client.right,
            y: client.bottom,
        };
        // SAFETY: both points are valid and hwnd supplies their client-space origin.
        unsafe {
            if !ClientToScreen(hwnd, &mut top_left).as_bool()
                || !ClientToScreen(hwnd, &mut bottom_right).as_bool()
            {
                return Err(format!(
                    "ClientToScreen failed: {}",
                    windows::core::Error::from_win32()
                ));
            }
        }
        Ok(ScreenRect {
            left: top_left.x,
            top: top_left.y,
            right: bottom_right.x,
            bottom: bottom_right.y,
        })
    }

    fn capture_gdi(rect: ScreenRect) -> Result<Crop, String> {
        let width = rect.width()?;
        let height = rect.height()?;
        let width_i32 = i32::try_from(width).map_err(|_| "GDI width exceeds i32".to_owned())?;
        let height_i32 = i32::try_from(height).map_err(|_| "GDI height exceeds i32".to_owned())?;
        // SAFETY: null HWND requests the screen DC.
        let screen = unsafe { GetDC(None) };
        if screen.is_invalid() {
            return Err(format!(
                "GetDC(NULL) failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: screen is a live screen DC.
        let memory = unsafe { CreateCompatibleDC(screen) };
        if memory.is_invalid() {
            // SAFETY: screen was acquired above and has not been released.
            unsafe {
                let _ = ReleaseDC(None, screen);
            }
            return Err(format!(
                "CreateCompatibleDC failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: dimensions are positive and screen is live.
        let bitmap = unsafe { CreateCompatibleBitmap(screen, width_i32, height_i32) };
        if bitmap.is_invalid() {
            // SAFETY: both DC handles are live and owned here.
            unsafe {
                let _ = DeleteDC(memory);
                let _ = ReleaseDC(None, screen);
            }
            return Err(format!(
                "CreateCompatibleBitmap failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: bitmap and memory DC are compatible live GDI objects.
        let previous = unsafe { SelectObject(memory, bitmap) };
        if previous.is_invalid() {
            // SAFETY: selection failed, so the bitmap is not owned by the DC;
            // every successfully-created handle can be released directly.
            unsafe {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(memory);
                let _ = ReleaseDC(None, screen);
            }
            return Err(format!(
                "SelectObject failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: both DCs are live, the bitmap is selected into memory, and
        // the source rectangle is expressed in physical screen pixels.
        let copied = unsafe {
            BitBlt(
                memory,
                0,
                0,
                width_i32,
                height_i32,
                screen,
                rect.left,
                rect.top,
                SRCCOPY | CAPTUREBLT,
            )
        };
        // SAFETY: previous is the object returned by the successful selection
        // above. GetDIBits requires the bitmap not be selected into any DC.
        unsafe {
            let _ = SelectObject(memory, previous);
        }
        let result = copied
            .map_err(|error| format!("BitBlt failed: {error}"))
            .and_then(|()| dib_pixels(memory, bitmap, width, height));
        // SAFETY: the bitmap is no longer selected. Delete both owned GDI
        // objects, then release the screen DC to its owner.
        unsafe {
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(memory);
            let _ = ReleaseDC(None, screen);
        }
        result
    }

    fn dib_pixels(
        dc: HDC,
        bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
        width: u32,
        height: u32,
    ) -> Result<Crop, String> {
        let bytes = pixel_bytes(width, height)?;
        let mut bgra = vec![0_u8; bytes];
        let height_i32 =
            i32::try_from(height).map_err(|_| "bitmap height exceeds i32".to_owned())?;
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>())
                    .map_err(|_| "BITMAPINFOHEADER size exceeds u32".to_owned())?,
                biWidth: i32::try_from(width).map_err(|_| "bitmap width exceeds i32".to_owned())?,
                biHeight: -height_i32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: dc and bitmap are live and compatible, bgra has width*height*4
        // bytes, and the top-down 32-bit description exactly matches that buffer.
        let lines = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                height,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if lines != height_i32 {
            return Err(format!(
                "GetDIBits returned {lines} of {height} rows: {}",
                windows::core::Error::from_win32()
            ));
        }
        Ok(Crop {
            width,
            height,
            bgra,
        })
    }

    fn capture_wgc_monitor(d3d: &D3d, hwnd: Option<HWND>) -> Result<(Crop, i32, i32), String> {
        ensure_wgc_supported()?;
        let hwnd = hwnd.ok_or_else(|| "wgc-monitor has no subject window".to_owned())?;
        // SAFETY: hwnd is a live subject; nearest-monitor fallback always returns a monitor for a real window.
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_invalid() {
            return Err("MonitorFromWindow returned no monitor".to_owned());
        }
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|error| format!("GraphicsCaptureItem interop factory failed: {error}"))?;
        // SAFETY: monitor is the live monitor selected above and the requested
        // interface is the runtime class implemented by this interop factory.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor) }
            .map_err(|error| format!("CreateForMonitor failed: {error}"))?;
        let full = capture_wgc_item(d3d, &item)?;
        let mut info = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>())
                .map_err(|_| "MONITORINFO size exceeds u32".to_owned())?,
            ..Default::default()
        };
        // SAFETY: monitor is live and info has the required cbSize and writable storage.
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return Err(format!(
                "GetMonitorInfoW failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        Ok((full, info.rcMonitor.left, info.rcMonitor.top))
    }

    fn capture_wgc_window(d3d: &D3d, hwnd: HWND, wanted: ScreenRect) -> Result<Crop, String> {
        ensure_wgc_supported()?;
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|error| format!("GraphicsCaptureItem interop factory failed: {error}"))?;
        // SAFETY: hwnd is a live subject and GraphicsCaptureItem is the interface exposed by the interop factory.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }
            .map_err(|error| format!("CreateForWindow failed: {error}"))?;
        let full = capture_wgc_item(d3d, &item)?;
        let outer = window_rect(hwnd)?;
        crop_from(
            &full,
            ScreenRect {
                left: wanted.left - outer.left,
                top: wanted.top - outer.top,
                right: wanted.right - outer.left,
                bottom: wanted.bottom - outer.top,
            },
        )
    }

    fn ensure_wgc_supported() -> Result<(), String> {
        GraphicsCaptureSession::IsSupported()
            .map_err(|error| format!("GraphicsCaptureSession::IsSupported failed: {error}"))?
            .then_some(())
            .ok_or_else(|| "unsupported".to_owned())
    }

    fn capture_wgc_item(d3d: &D3d, item: &GraphicsCaptureItem) -> Result<Crop, String> {
        let winrt_device = d3d.winrt_device.as_ref().map_err(Clone::clone)?;
        let size = item
            .Size()
            .map_err(|error| format!("GraphicsCaptureItem::Size failed: {error}"))?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|error| format!("CreateFreeThreaded failed: {error}"))?;
        let session = match pool.CreateCaptureSession(item) {
            Ok(session) => session,
            Err(error) => {
                let _ = pool.Close();
                return Err(format!("CreateCaptureSession failed: {error}"));
            }
        };
        let result = (|| {
            session
                .SetIsCursorCaptureEnabled(false)
                .map_err(|error| format!("SetIsCursorCaptureEnabled(false) failed: {error}"))?;
            let _ = session.SetIsBorderRequired(false);
            session
                .StartCapture()
                .map_err(|error| format!("StartCapture failed: {error}"))?;
            thread::sleep(Duration::from_millis(WGC_INITIAL_WAIT_MS));
            let deadline = Instant::now() + Duration::from_millis(WGC_TIMEOUT_MS);
            let mut latest: Option<Direct3D11CaptureFrame> = None;
            while Instant::now() < deadline {
                let mut received = false;
                while let Ok(frame) = pool.TryGetNextFrame() {
                    latest = Some(frame);
                    received = true;
                }
                if received {
                    thread::sleep(Duration::from_millis(20));
                } else if latest.is_some() {
                    break;
                } else {
                    thread::sleep(Duration::from_millis(20));
                }
            }
            let frame = latest
                .ok_or_else(|| "TryGetNextFrame produced no frame within 3 seconds".to_owned())?;
            let surface = frame
                .Surface()
                .map_err(|error| format!("frame.Surface failed: {error}"))?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|error| {
                format!("surface -> IDirect3DDxgiInterfaceAccess failed: {error}")
            })?;
            // SAFETY: access is the DXGI interface for the live frame surface.
            let texture: ID3D11Texture2D = unsafe { access.GetInterface() }
                .map_err(|error| format!("GetInterface<ID3D11Texture2D> failed: {error}"))?;
            read_texture(d3d, &texture)
        })();
        let close_session = session.Close();
        let close_pool = pool.Close();
        result.and_then(|crop| {
            close_session
                .map_err(|error| format!("GraphicsCaptureSession::Close failed: {error}"))?;
            close_pool
                .map_err(|error| format!("Direct3D11CaptureFramePool::Close failed: {error}"))?;
            Ok(crop)
        })
    }

    fn capture_desktop_duplication(
        d3d: &D3d,
        wanted: Option<ScreenRect>,
        owned_windows: &[HWND],
    ) -> Result<(Crop, i32, i32), String> {
        let wanted = wanted.ok_or_else(|| "dxgi-dup has no subject rectangle".to_owned())?;
        let dxgi: IDXGIDevice = d3d
            .device
            .cast()
            .map_err(|error| format!("ID3D11Device -> IDXGIDevice failed: {error}"))?;
        // SAFETY: dxgi is a live device interface.
        let adapter = unsafe { dxgi.GetAdapter() }
            .map_err(|error| format!("IDXGIDevice::GetAdapter failed: {error}"))?;
        let mut index = 0_u32;
        let (output, description) = loop {
            // SAFETY: indexes are enumerated from zero until DXGI reports exhaustion.
            let candidate = unsafe { adapter.EnumOutputs(index) }
                .map_err(|error| format!("no DXGI output contains {:?}: {error}", wanted))?;
            // SAFETY: candidate is a live output returned by this adapter.
            let description = unsafe { candidate.GetDesc() }
                .map_err(|error| format!("IDXGIOutput::GetDesc failed: {error}"))?;
            let rect = description.DesktopCoordinates;
            if wanted.left >= rect.left
                && wanted.top >= rect.top
                && wanted.right <= rect.right
                && wanted.bottom <= rect.bottom
            {
                break (candidate, description);
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| "DXGI output index overflow".to_owned())?;
        };
        let output1: IDXGIOutput1 = output
            .cast()
            .map_err(|error| format!("IDXGIOutput -> IDXGIOutput1 failed: {error}"))?;
        let mut duplication = duplicate_output_and_provoke_present(&output1, d3d, owned_windows)?;
        let deadline = Instant::now() + Duration::from_millis(DUPLICATION_TIMEOUT_MS);
        let mut recreated = false;
        let mut saw_empty_frame = false;
        loop {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            // SAFETY: info and resource are live output slots and ReleaseFrame is called after every successful acquisition.
            let acquired = unsafe { duplication.AcquireNextFrame(500, &mut info, &mut resource) };
            match acquired {
                Ok(()) => {
                    if info.LastPresentTime == 0 {
                        saw_empty_frame = true;
                        // SAFETY: AcquireNextFrame succeeded and exactly one frame is outstanding.
                        unsafe { duplication.ReleaseFrame() }
                            .map_err(|error| format!("ReleaseFrame failed: {error}"))?;
                        if Instant::now() >= deadline {
                            return Err(no_desktop_image_error());
                        }
                        continue;
                    }
                    let result = resource
                        .ok_or_else(|| "AcquireNextFrame returned no resource".to_owned())
                        .and_then(|resource| {
                            let texture: ID3D11Texture2D = resource.cast().map_err(|error| {
                                format!("IDXGIResource -> ID3D11Texture2D failed: {error}")
                            })?;
                            read_texture(d3d, &texture)
                        });
                    // SAFETY: AcquireNextFrame succeeded and exactly one frame is outstanding.
                    let released = unsafe { duplication.ReleaseFrame() };
                    let crop = match (result, released) {
                        (Ok(crop), Ok(())) => crop,
                        (Err(error), _) => return Err(error),
                        (Ok(_), Err(error)) => {
                            return Err(format!("ReleaseFrame failed: {error}"));
                        }
                    };
                    return Ok((
                        crop,
                        description.DesktopCoordinates.left,
                        description.DesktopCoordinates.top,
                    ));
                }
                Err(error)
                    if error.code() == DXGI_ERROR_WAIT_TIMEOUT && Instant::now() < deadline =>
                {
                    continue
                }
                Err(error)
                    if error.code() == DXGI_ERROR_ACCESS_LOST
                        && !recreated
                        && Instant::now() < deadline =>
                {
                    drop(duplication);
                    duplication =
                        duplicate_output_and_provoke_present(&output1, d3d, owned_windows)?;
                    recreated = true;
                    continue;
                }
                Err(_) if saw_empty_frame && Instant::now() >= deadline => {
                    return Err(no_desktop_image_error());
                }
                Err(error) => return Err(format!("AcquireNextFrame failed: {error}")),
            }
        }
    }

    fn duplicate_output_and_provoke_present(
        output: &IDXGIOutput1,
        d3d: &D3d,
        owned_windows: &[HWND],
    ) -> Result<IDXGIOutputDuplication, String> {
        // SAFETY: output and device belong to the same adapter by construction.
        let duplication = unsafe { output.DuplicateOutput(&d3d.device) }
            .map_err(|error| format!("DuplicateOutput failed: {error}"))?;
        for hwnd in owned_windows {
            // SAFETY: every HWND in this slice belongs to this probe and remains
            // live throughout measurement; None invalidates the full client area.
            unsafe {
                let _ = InvalidateRect(*hwnd, None, true);
                let _ = UpdateWindow(*hwnd);
            }
        }
        pump(DUPLICATION_REPAINT_MS);
        Ok(duplication)
    }

    fn no_desktop_image_error() -> String {
        "Desktop Duplication delivered no desktop image within 2 s (every frame had LastPresentTime == 0)"
            .to_owned()
    }

    fn read_texture(d3d: &D3d, texture: &ID3D11Texture2D) -> Result<Crop, String> {
        let mut source = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: source points to writable storage and texture is live.
        unsafe {
            texture.GetDesc(&mut source);
        }
        if source.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(format!(
                "capture texture format {:?} is not DXGI_FORMAT_B8G8R8A8_UNORM",
                source.Format
            ));
        }
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..source
        };
        let mut staging = None;
        // SAFETY: staging_desc is initialized and the output slot is live; no initial data is required for a staging copy.
        unsafe {
            d3d.device
                .CreateTexture2D(&staging_desc, None, Some(&raw mut staging))
        }
        .map_err(|error| format!("CreateTexture2D(staging) failed: {error}"))?;
        let staging =
            staging.ok_or_else(|| "CreateTexture2D returned no staging texture".to_owned())?;
        // SAFETY: source and destination are same-description textures and both remain live through the copy.
        unsafe {
            d3d.context.CopyResource(&staging, texture);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: staging was created CPU-readable and mapped points to writable storage.
        unsafe {
            d3d.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|error| format!("ID3D11DeviceContext::Map failed: {error}"))?;
        let copied = copy_mapped_rows(source.Width, source.Height, mapped);
        // SAFETY: subresource zero was successfully mapped immediately above and has not yet been unmapped.
        unsafe {
            d3d.context.Unmap(&staging, 0);
        }
        copied
    }

    fn copy_mapped_rows(
        width: u32,
        height: u32,
        mapped: D3D11_MAPPED_SUBRESOURCE,
    ) -> Result<Crop, String> {
        let row_bytes = usize::try_from(width)
            .map_err(|_| "texture width exceeds usize".to_owned())?
            .checked_mul(4)
            .ok_or_else(|| "texture row size overflow".to_owned())?;
        let pitch =
            usize::try_from(mapped.RowPitch).map_err(|_| "RowPitch exceeds usize".to_owned())?;
        if pitch < row_bytes {
            return Err(format!(
                "RowPitch {pitch} is smaller than row bytes {row_bytes}"
            ));
        }
        let source_bytes = pitch
            .checked_mul(
                usize::try_from(height).map_err(|_| "texture height exceeds usize".to_owned())?,
            )
            .ok_or_else(|| "mapped texture size overflow".to_owned())?;
        // SAFETY: Map returned pData for at least RowPitch*height readable bytes and the mapping stays live until this function returns to its caller.
        let source = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), source_bytes) };
        let mut bgra = Vec::with_capacity(pixel_bytes(width, height)?);
        for row in source
            .chunks_exact(pitch)
            .take(usize::try_from(height).map_err(|_| "height exceeds usize".to_owned())?)
        {
            let pixels = row
                .get(..row_bytes)
                .ok_or_else(|| "mapped row is shorter than expected".to_owned())?;
            bgra.extend_from_slice(pixels);
        }
        Ok(Crop {
            width,
            height,
            bgra,
        })
    }

    fn crop_from(source: &Crop, rect: ScreenRect) -> Result<Crop, String> {
        let width = rect.width()?;
        let height = rect.height()?;
        let left = u32::try_from(rect.left)
            .map_err(|_| format!("crop begins left of capture: {rect:?}"))?;
        let top =
            u32::try_from(rect.top).map_err(|_| format!("crop begins above capture: {rect:?}"))?;
        if left
            .checked_add(width)
            .is_none_or(|right| right > source.width)
            || top
                .checked_add(height)
                .is_none_or(|bottom| bottom > source.height)
        {
            return Err(format!(
                "crop {rect:?} lies outside {}x{} capture",
                source.width, source.height
            ));
        }
        let source_stride = usize::try_from(source.width)
            .map_err(|_| "source width exceeds usize".to_owned())?
            .checked_mul(4)
            .ok_or_else(|| "source stride overflow".to_owned())?;
        let left_bytes = usize::try_from(left)
            .map_err(|_| "crop left exceeds usize".to_owned())?
            .checked_mul(4)
            .ok_or_else(|| "crop offset overflow".to_owned())?;
        let row_bytes = usize::try_from(width)
            .map_err(|_| "crop width exceeds usize".to_owned())?
            .checked_mul(4)
            .ok_or_else(|| "crop row overflow".to_owned())?;
        let mut bgra = Vec::with_capacity(pixel_bytes(width, height)?);
        for y in top..top + height {
            let start = usize::try_from(y)
                .map_err(|_| "crop row exceeds usize".to_owned())?
                .checked_mul(source_stride)
                .and_then(|offset| offset.checked_add(left_bytes))
                .ok_or_else(|| "crop byte offset overflow".to_owned())?;
            let end = start
                .checked_add(row_bytes)
                .ok_or_else(|| "crop row end overflow".to_owned())?;
            let row = source
                .bgra
                .get(start..end)
                .ok_or_else(|| "crop row lies outside pixel buffer".to_owned())?;
            bgra.extend_from_slice(row);
        }
        Ok(Crop {
            width,
            height,
            bgra,
        })
    }

    fn print_window_reference(hwnd: HWND, wanted: ScreenRect) -> Result<Crop, String> {
        let outer = window_rect(hwnd)?;
        let width = outer.width()?;
        let height = outer.height()?;
        let width_i32 =
            i32::try_from(width).map_err(|_| "PrintWindow width exceeds i32".to_owned())?;
        let height_i32 =
            i32::try_from(height).map_err(|_| "PrintWindow height exceeds i32".to_owned())?;
        // SAFETY: null HWND requests a screen-compatible reference DC.
        let screen = unsafe { GetDC(None) };
        if screen.is_invalid() {
            return Err(format!(
                "GetDC for PrintWindow failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: screen is live.
        let memory = unsafe { CreateCompatibleDC(screen) };
        let bitmap = if memory.is_invalid() {
            windows::Win32::Graphics::Gdi::HBITMAP::default()
        } else {
            // SAFETY: screen is live and dimensions are positive.
            unsafe { CreateCompatibleBitmap(screen, width_i32, height_i32) }
        };
        if memory.is_invalid() || bitmap.is_invalid() {
            // SAFETY: release only handles which were successfully acquired.
            unsafe {
                if !memory.is_invalid() {
                    let _ = DeleteDC(memory);
                }
                let _ = ReleaseDC(None, screen);
            }
            return Err(format!(
                "could not allocate PrintWindow bitmap: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: bitmap and memory DC are live and compatible.
        let previous: HGDIOBJ = unsafe { SelectObject(memory, bitmap) };
        if previous.is_invalid() {
            // SAFETY: the failed selection left the bitmap independent of the
            // DC, so every successfully-acquired handle can be released.
            unsafe {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(memory);
                let _ = ReleaseDC(None, screen);
            }
            return Err(format!(
                "SelectObject for PrintWindow failed: {}",
                windows::core::Error::from_win32()
            ));
        }
        // SAFETY: hwnd is live, memory has a suitably-sized selected bitmap and PW_RENDERFULLCONTENT is a documented flag.
        let rendered = unsafe { PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT) } != 0;
        // SAFETY: previous came from the successful selection above and must be
        // restored before GetDIBits reads the bitmap.
        unsafe {
            let _ = SelectObject(memory, previous);
        }
        let result = if rendered {
            dib_pixels(memory, bitmap, width, height).and_then(|full| {
                crop_from(
                    &full,
                    ScreenRect {
                        left: wanted.left - outer.left,
                        top: wanted.top - outer.top,
                        right: wanted.right - outer.left,
                        bottom: wanted.bottom - outer.top,
                    },
                )
            })
        } else {
            Err(format!(
                "PrintWindow failed: {}",
                windows::core::Error::from_win32()
            ))
        };
        // SAFETY: the bitmap was deselected above, so every GDI handle can now
        // be released in reverse ownership order.
        unsafe {
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(memory);
            let _ = ReleaseDC(None, screen);
        }
        result
    }

    fn dump_crop(
        directory: &Path,
        phase: &str,
        method: Method,
        window: &str,
        crop: &Crop,
    ) -> Result<(), String> {
        let path = directory.join(format!("{phase}-{}-{window}.bmp", method.name()));
        let row_bytes = usize::try_from(crop.width)
            .map_err(|_| "BMP width exceeds usize".to_owned())?
            .checked_mul(3)
            .ok_or_else(|| "BMP row overflow".to_owned())?;
        let padded = row_bytes
            .checked_add(3)
            .map(|value| value & !3)
            .ok_or_else(|| "BMP padding overflow".to_owned())?;
        let image_size = padded
            .checked_mul(
                usize::try_from(crop.height).map_err(|_| "BMP height exceeds usize".to_owned())?,
            )
            .ok_or_else(|| "BMP image size overflow".to_owned())?;
        let file_size = 54_usize
            .checked_add(image_size)
            .ok_or_else(|| "BMP file size overflow".to_owned())?;
        let mut file =
            fs::File::create(&path).map_err(|error| format!("create {path:?}: {error}"))?;
        let mut header = Vec::with_capacity(54);
        header.extend_from_slice(b"BM");
        header.extend_from_slice(
            &u32::try_from(file_size)
                .map_err(|_| "BMP file exceeds u32".to_owned())?
                .to_le_bytes(),
        );
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&54_u32.to_le_bytes());
        header.extend_from_slice(&40_u32.to_le_bytes());
        header.extend_from_slice(
            &i32::try_from(crop.width)
                .map_err(|_| "BMP width exceeds i32".to_owned())?
                .to_le_bytes(),
        );
        header.extend_from_slice(
            &i32::try_from(crop.height)
                .map_err(|_| "BMP height exceeds i32".to_owned())?
                .to_le_bytes(),
        );
        header.extend_from_slice(&1_u16.to_le_bytes());
        header.extend_from_slice(&24_u16.to_le_bytes());
        header.extend_from_slice(&0_u32.to_le_bytes());
        header.extend_from_slice(
            &u32::try_from(image_size)
                .map_err(|_| "BMP image exceeds u32".to_owned())?
                .to_le_bytes(),
        );
        header.extend_from_slice(&[0; 16]);
        file.write_all(&header)
            .map_err(|error| format!("write {path:?}: {error}"))?;
        let padding = vec![0_u8; padded - row_bytes];
        let source_stride = usize::try_from(crop.width)
            .map_err(|_| "crop width exceeds usize".to_owned())?
            .checked_mul(4)
            .ok_or_else(|| "crop stride overflow".to_owned())?;
        for row in crop.bgra.chunks_exact(source_stride).rev() {
            for pixel in row.as_chunks::<4>().0.iter() {
                let bgr = pixel
                    .get(..3)
                    .ok_or_else(|| "short BGRA pixel".to_owned())?;
                file.write_all(bgr)
                    .map_err(|error| format!("write {path:?}: {error}"))?;
            }
            file.write_all(&padding)
                .map_err(|error| format!("write {path:?}: {error}"))?;
        }
        Ok(())
    }

    fn pixel_bytes(width: u32, height: u32) -> Result<usize, String> {
        usize::try_from(width)
            .map_err(|_| "width exceeds usize".to_owned())?
            .checked_mul(usize::try_from(height).map_err(|_| "height exceeds usize".to_owned())?)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "pixel buffer size overflow".to_owned())
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
