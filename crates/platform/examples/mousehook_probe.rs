//! What `WH_MOUSE_LL` actually delivers for the side buttons, and whether the
//! hook fires on them.
//!
//! An example rather than a test, for the reason `keyhook_probe` is one: it
//! installs a global low-level hook and injects input into the machine, neither
//! of which belongs in `cargo test` on somebody's desktop.
//!
//! # Why this exists at all
//!
//! `MSLLHOOKSTRUCT::mouseData` carries the X-button number in its **high**
//! word, and `SendInput`'s `MOUSEINPUT::mouseData` carries the same number in
//! its **low** word. Two fields, one name, opposite halves. A decoder written
//! from the wrong half reads zero for every click and reports a mouse with no
//! side buttons — which looks exactly like a mouse with no side buttons. That
//! is not a thing to reason out from documentation, so it is measured.
//!
//! # The control
//!
//! Phase one injects **mouse movement and a wheel tick before any button**, and
//! both are cases whose answer is already known: a move is not a button press,
//! and a wheel notch puts `120` in the very high word the button number lives
//! in. If the run reports no movement at all, the hook is dead and every later
//! "no button here" line is worthless — which is precisely how
//! `native_path_probe` once reported failure for every case including its
//! control, and how that was caught.
//!
//! Run with `cargo run -p kw-platform --example mousehook_probe`.

fn main() {
    let support = kw_platform::keyhook::support();
    println!("support: {support:?}");
    if !support.is_available() {
        return;
    }
    raw::measure();
    library::measure();
}

/// Phase one: a hook of our own, reporting the bytes rather than a decision.
#[cfg(target_os = "windows")]
mod raw {
    use std::sync::mpsc;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageA, SetWindowsHookExW,
        UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_QUIT, WM_XBUTTONDOWN, WM_XBUTTONUP,
    };

    /// Every message the hook saw, as (message, `mouseData`).
    static SEEN: OnceLock<Mutex<Vec<(u32, u32)>>> = OnceLock::new();

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for a WH_MOUSE_LL hook with a non-negative code, lparam
            // points at an MSLLHOOKSTRUCT for the lifetime of this call.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            if let Some(seen) = SEEN.get() {
                if let Ok(mut held) = seen.lock() {
                    held.push((wparam.0 as u32, info.mouseData));
                }
            }
        }
        // SAFETY: the next hook in the chain, arguments unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    pub fn measure() {
        let _ = SEEN.set(Mutex::new(Vec::new()));

        let (ready, started) = mpsc::channel::<u32>();
        let join = std::thread::spawn(move || {
            // SAFETY: a null module handle and thread id 0 install a global
            // low-level hook, the documented use of WH_MOUSE_LL.
            let hook: HHOOK = match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(proc), None, 0) } {
                Ok(hook) => hook,
                Err(err) => {
                    println!("could not install a raw mouse hook: {err}");
                    return;
                }
            };
            // SAFETY: no arguments.
            let id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            let _ = ready.send(id);
            let mut message = MSG::default();
            // SAFETY: standard message loop; GetMessageW returns 0 on WM_QUIT.
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    DispatchMessageW(&message);
                }
                let _ = UnhookWindowsHookEx(hook);
            }
        });

        let Ok(thread_id) = started.recv() else {
            println!("the raw hook thread stopped before installing");
            return;
        };
        std::thread::sleep(Duration::from_millis(300));

        println!("\n== phase 1: what arrives ==");
        println!("-- control: three pixels right, three back, and one wheel notch --");
        super::inject::move_by(3, 0);
        super::inject::move_by(-3, 0);
        super::inject::wheel();
        std::thread::sleep(Duration::from_millis(200));

        println!("-- XBUTTON1, then XBUTTON2 --");
        super::inject::x_button(1);
        std::thread::sleep(Duration::from_millis(80));
        super::inject::x_button(2);
        std::thread::sleep(Duration::from_millis(300));

        // SAFETY: posting to a thread id this function created.
        unsafe {
            let _ = PostThreadMessageA(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = join.join();

        let Some(seen) = SEEN.get().and_then(|s| s.lock().ok()) else {
            println!("nothing was recorded");
            return;
        };

        let mut moves = 0_usize;
        let mut wheels = 0_usize;
        let mut buttons: Vec<(u32, u16)> = Vec::new();
        for (message, data) in seen.iter() {
            #[allow(clippy::cast_possible_truncation)]
            let high = ((*data >> 16) & 0xFFFF) as u16;
            match *message {
                WM_MOUSEMOVE => moves += 1,
                WM_MOUSEWHEEL => {
                    wheels += 1;
                    println!("  WM_MOUSEWHEEL   mouseData=0x{data:08X} high word {high}");
                }
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let name = if *message == WM_XBUTTONDOWN {
                        "DOWN"
                    } else {
                        "UP  "
                    };
                    println!("  WM_XBUTTON{name} mouseData=0x{data:08X} high word {high}");
                    buttons.push((*message, high));
                }
                _ => {}
            }
        }

        println!("\ncontrol: {moves} movement message(s), {wheels} wheel message(s)");
        if moves == 0 {
            println!(
                "  THE CONTROL FAILED. No movement reached the hook, so it is not \
                 listening and nothing below this line is evidence of anything."
            );
            return;
        }
        println!("  the hook is alive, and neither a move nor a wheel is an X button.");

        let downs: Vec<u16> = buttons
            .iter()
            .filter(|(message, _)| *message == WM_XBUTTONDOWN)
            .map(|(_, high)| *high)
            .collect();
        println!("X-button presses seen, by high word: {downs:?}");
        if downs == vec![1_u16, 2] {
            println!(
                "  measured: the button number is in the HIGH word -- 1 for XBUTTON1 \
                 (mouse 4) and 2 for XBUTTON2 (mouse 5). SendInput took the same two \
                 numbers in the LOW word, which is the asymmetry this probe is for."
            );
        } else {
            println!("  NOT what was expected: [1, 2]. The decoder in keyhook.rs assumes it.");
        }
    }
}

/// Phase two: the shipped hook, asked whether it fires.
#[cfg(target_os = "windows")]
mod library {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use kw_platform::keyhook::{Edge, KeyHook};

    pub fn measure() {
        println!("\n== phase 2: what the hook reports ==");

        let events: Arc<Mutex<Vec<(usize, Edge)>>> = Arc::new(Mutex::new(Vec::new()));
        let during_control = Arc::new(AtomicUsize::new(0));

        let sink = Arc::clone(&events);
        let hook =
            match KeyHook::install(&["Mouse4".to_owned(), "Mouse5".to_owned()], move |i, e| {
                if let Ok(mut held) = sink.lock() {
                    held.push((i, e));
                }
            }) {
                Ok(hook) => hook,
                Err(err) => {
                    println!("could not install: {err}");
                    return;
                }
            };
        println!("installed, watching {:?}", hook.watching());
        std::thread::sleep(Duration::from_millis(300));

        println!("-- control: movement and a wheel notch, which are not button 4 --");
        super::inject::move_by(3, 0);
        super::inject::move_by(-3, 0);
        super::inject::wheel();
        std::thread::sleep(Duration::from_millis(250));
        during_control.store(
            events.lock().map(|held| held.len()).unwrap_or(usize::MAX),
            Ordering::SeqCst,
        );
        println!(
            "  {} fire(s) during the control",
            during_control.load(Ordering::SeqCst)
        );

        println!("-- Mouse4, then Mouse5 --");
        super::inject::x_button(1);
        std::thread::sleep(Duration::from_millis(120));
        super::inject::x_button(2);
        std::thread::sleep(Duration::from_millis(400));

        let seen = events.lock().map(|held| held.clone()).unwrap_or_default();
        println!("\nreported: {seen:?}");

        let expected = vec![
            (0_usize, Edge::Press),
            (0, Edge::Release),
            (1, Edge::Press),
            (1, Edge::Release),
        ];
        if during_control.load(Ordering::SeqCst) != 0 {
            println!("THE CONTROL FAILED: movement fired a mouse-button binding.");
        } else if seen == expected {
            println!(
                "the mouse hook works: each side button fires its own index, once on the \
                 way down and once on the way up, and nothing else does."
            );
        } else {
            println!("NOT what was expected: {expected:?}");
        }

        drop(hook);
        println!("uninstalled.");
    }
}

#[cfg(target_os = "windows")]
mod inject {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };

    fn event(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) {
        // SAFETY: a well-formed INPUT array and the matching size.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            println!("  SendInput accepted {sent} of {}", inputs.len());
        }
    }

    /// Move the pointer, relatively. A zero-pixel move is filtered by Windows
    /// and reaches no hook, which is why the control moves and moves back
    /// rather than standing still.
    pub fn move_by(dx: i32, dy: i32) {
        send(&[event(MOUSEEVENTF_MOVE, dx, dy, 0)]);
    }

    /// One wheel notch. The delta lands in the same high word a button number
    /// would, which is what makes it a control rather than noise.
    pub fn wheel() {
        send(&[event(MOUSEEVENTF_WHEEL, 0, 0, 120)]);
    }

    /// Press and release one X button. `which` is 1 or 2, in the LOW word --
    /// see the module documentation.
    pub fn x_button(which: u32) {
        send(&[event(MOUSEEVENTF_XDOWN, 0, 0, which)]);
        std::thread::sleep(std::time::Duration::from_millis(40));
        send(&[event(MOUSEEVENTF_XUP, 0, 0, which)]);
    }
}

#[cfg(not(target_os = "windows"))]
mod raw {
    pub fn measure() {}
}

#[cfg(not(target_os = "windows"))]
mod library {
    pub fn measure() {}
}
