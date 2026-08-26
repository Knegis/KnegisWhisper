//! Whether the keyboard hook really fires.
//!
//! An example rather than a test: it installs a global low-level hook and
//! injects keystrokes into the machine, neither of which belongs in `cargo test`
//! on somebody's desktop. It is here because `keys.rs` can prove the chord logic
//! and nothing but a real hook can prove the Win32 half.
//!
//! `RightControl+RightShift` is used rather than the real `Control+Super`
//! default: injecting Super puts a Start-menu keystroke into whatever the user is
//! doing, and a probe should not do that.
//!
//! Run with `cargo run -p kw-platform --example keyhook_probe`.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let support = kw_platform::keyhook::support();
    println!("support: {support:?}");
    if !support.is_available() {
        return;
    }

    let fires = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fires);

    let hook = match kw_platform::keyhook::KeyHook::install(
        &["RightControl+RightShift".to_owned()],
        move |index| {
            counter.fetch_add(1, Ordering::SeqCst);
            println!("  chord {index} fired");
        },
    ) {
        Ok(hook) => hook,
        Err(err) => {
            println!("could not install: {err}");
            return;
        }
    };
    println!("installed, watching {:?}", hook.watching());

    // Let the hook thread reach its message loop before anything is injected.
    std::thread::sleep(Duration::from_millis(300));

    println!("\ninjecting RightControl+RightShift twice...");
    for _ in 0..2 {
        inject::chord_press();
        std::thread::sleep(Duration::from_millis(150));
    }
    std::thread::sleep(Duration::from_millis(300));

    let count = fires.load(Ordering::SeqCst);
    println!("\nfired {count} time(s) for two presses");
    if count == 2 {
        println!("the hook works, and fires once per press.");
    } else if count == 0 {
        println!("the hook never fired.");
    } else {
        println!("the hook fired {count} times, which is not once per press.");
    }

    drop(hook);
    println!("uninstalled.");
}

#[cfg(target_os = "windows")]
mod inject {
    use std::time::Duration;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_RCONTROL, VK_RSHIFT,
    };

    fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) {
        // SAFETY: a well-formed INPUT array and the matching size.
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            println!("  SendInput accepted {sent} of {}", inputs.len());
        }
    }

    /// Press both keys, then release both. The chord should fire on the second
    /// key going down, and exactly once.
    pub fn chord_press() {
        send(&[key(VK_RCONTROL, KEYBD_EVENT_FLAGS(0))]);
        std::thread::sleep(Duration::from_millis(30));
        send(&[key(VK_RSHIFT, KEYBD_EVENT_FLAGS(0))]);
        std::thread::sleep(Duration::from_millis(30));
        send(&[
            key(VK_RSHIFT, KEYEVENTF_KEYUP),
            key(VK_RCONTROL, KEYEVENTF_KEYUP),
        ]);
    }
}

#[cfg(not(target_os = "windows"))]
mod inject {
    pub fn chord_press() {}
}
