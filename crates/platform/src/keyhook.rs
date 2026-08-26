//! The low-level keyboard hook that makes a modifier-only shortcut work.
//!
//! `Control+Super` — upstream's default dictation shortcut — cannot be
//! registered through any cross-platform shortcut API, so
//! `hotkey::classify` has been reporting it as
//! [`ModifierOnly`](crate::hotkey::Accelerator::ModifierOnly) and telling the
//! user it does nothing. On Windows it does something now.
//!
//! Deliberately thin. Everything that decides *when a chord has fired* is in
//! [`crate::keys`] and is fully tested; what is here is the Win32 plumbing, which
//! no test can reach.
//!
//! # How it differs from upstream
//!
//! Upstream ran **one native listener process per watched key**, communicating
//! over stdout. This is one `WH_KEYBOARD_LL` hook on one thread inside the
//! application: no child processes, no pipes, and no per-key process to leak.
//!
//! # What it does not do
//!
//! It does not swallow keystrokes. The hook always calls the next hook in the
//! chain, so `Control+Super` still reaches the shell and Windows still opens
//! whatever it opens. Consuming the event is technically possible and is not
//! done: an application that silently eats a system shortcut is indistinguishable
//! from a broken keyboard, and `docs/SECURITY.md` rules out interfering with the
//! rest of the machine.
//!
//! It does not log keystrokes. The hook sees every key the user presses,
//! including into other applications and including passwords. What leaves this
//! module is the *index of a chord that matched*, never a key. There is no
//! logging of key events at any level, and that is not an oversight to be
//! corrected by adding a `trace!` later — a keystroke log is the thing this
//! mechanism must never become.

use crate::keys::{Chord, ChordWatcher, Fired};
use crate::PlatformError;

/// Whether this platform can watch a modifier-only chord, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSupport {
    /// A hook can be installed.
    Available { mechanism: &'static str },
    /// This platform has no such mechanism in this build, with the reason.
    Unsupported { reason: String },
}

impl HookSupport {
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// What this build can do about modifier-only shortcuts here.
#[must_use]
pub fn support() -> HookSupport {
    #[cfg(target_os = "windows")]
    {
        HookSupport::Available {
            mechanism: "a WH_KEYBOARD_LL hook on a dedicated thread",
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        HookSupport::Unsupported {
            reason: "modifier-only shortcuts need a low-level keyboard hook. macOS needs a \
                     CGEventTap and the Accessibility permission that goes with it; Wayland has \
                     no equivalent at all and its global-shortcuts portal takes only chords the \
                     compositor itself can register. Neither is written, and neither can be \
                     verified from the machine this is developed on."
                .to_owned(),
        }
    }
}

/// A running hook. Dropping it uninstalls the hook and stops the thread.
///
/// Not `Clone`: there is one hook, and two owners deciding independently when to
/// uninstall it is how a hook outlives the application that installed it.
#[derive(Debug)]
pub struct KeyHook {
    /// Held for its `Drop`, which posts `WM_QUIT` to the hook thread and lets it
    /// unhook itself. Never read: the value *is* the hook's lifetime.
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    inner: windows_impl::Installed,
    /// The chords being watched, so a caller can report what is live.
    watching: Vec<String>,
}

impl KeyHook {
    /// Install a hook watching `chords`, calling `on_fire` with the index of
    /// whichever fired.
    ///
    /// `on_fire` runs on the hook thread and must not block: a slow callback
    /// delays every keystroke on the machine, because a low-level hook sits in
    /// the input path. Send on a channel and return.
    ///
    /// Returns `Err` where the platform has no mechanism, rather than a handle
    /// that does nothing — a caller holding a `KeyHook` must be able to tell the
    /// user their shortcut works.
    pub fn install<F>(accelerators: &[String], on_fire: F) -> Result<Self, PlatformError>
    where
        F: Fn(usize) + Send + 'static,
    {
        if accelerators.is_empty() {
            return Err(PlatformError::Unsupported(
                "no modifier-only shortcuts are configured, so there is nothing to hook".to_owned(),
            ));
        }

        let mut chords = Vec::with_capacity(accelerators.len());
        for accelerator in accelerators {
            chords.push(
                Chord::parse(accelerator).map_err(|e| PlatformError::Unsupported(e.to_string()))?,
            );
        }

        #[cfg(target_os = "windows")]
        {
            let inner = windows_impl::install(ChordWatcher::new(chords), on_fire)?;
            Ok(Self {
                inner,
                watching: accelerators.to_vec(),
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (chords, on_fire);
            Err(PlatformError::Unsupported(match support() {
                HookSupport::Unsupported { reason } => reason,
                HookSupport::Available { .. } => unreachable!("no hook on this platform"),
            }))
        }
    }

    /// The accelerators this hook is watching.
    #[must_use]
    pub fn watching(&self) -> &[String] {
        &self.watching
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::mpsc;
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageA, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
        WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::{ChordWatcher, Fired, PlatformError};
    use crate::keys::Key;

    /// The state the hook callback reaches.
    ///
    /// A `static` because a Win32 hook procedure is a bare `extern "system" fn`
    /// with no user pointer — there is nowhere to put a closure. One hook per
    /// process is the actual constraint, so a single global is not a shortcut
    /// around a limitation; it *is* the limitation, stated.
    struct HookState {
        watcher: Mutex<ChordWatcher>,
        on_fire: Box<dyn Fn(usize) + Send + Sync>,
    }

    static STATE: OnceLock<HookState> = OnceLock::new();

    /// A hook that is installed, and the thread running its message loop.
    pub struct Installed {
        /// The thread to post `WM_QUIT` to when dropping.
        thread_id: u32,
        join: Option<thread::JoinHandle<()>>,
    }

    impl std::fmt::Debug for Installed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Installed")
                .field("thread_id", &self.thread_id)
                .finish()
        }
    }

    impl Drop for Installed {
        fn drop(&mut self) {
            // The hook is owned by its thread and must be removed there, so the
            // thread is asked to quit rather than the hook being unhooked from
            // here. A hook left installed after the application exits is a
            // process Windows keeps calling into.
            //
            // SAFETY: posting a message to a thread id we created.
            unsafe {
                let _ = PostThreadMessageA(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(join) = self.join.take() {
                // Not waited on for long: if the loop is wedged, leaking the
                // thread at shutdown is better than hanging the application.
                let _ = join.join();
            }
        }
    }

    /// Translate a virtual key code into the keys a chord can be built from.
    ///
    /// `VK_LCONTROL` and friends rather than `VK_CONTROL`: a low-level hook
    /// reports the sided codes, which is the whole reason left and right can be
    /// told apart here and cannot through the cross-platform API.
    const fn key_of(vk: u32) -> Key {
        match vk {
            0xA2 => Key::LeftControl,
            0xA3 => Key::RightControl,
            0xA0 => Key::LeftShift,
            0xA1 => Key::RightShift,
            0xA4 => Key::LeftAlt,
            0xA5 => Key::RightAlt,
            0x5B => Key::LeftSuper,
            0x5C => Key::RightSuper,
            other => Key::Other(other),
        }
    }

    /// The hook procedure.
    ///
    /// Runs in the input path of every keystroke on the machine, so it does as
    /// little as possible and **always** calls the next hook. It never logs a
    /// key: see the module documentation.
    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // A negative code means "pass it on without inspecting", per the API
        // contract.
        if code < 0 {
            // SAFETY: passing the arguments straight on, which is the documented
            // behaviour for a code the hook must not inspect.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        if let Some(state) = STATE.get() {
            // SAFETY: for a WH_KEYBOARD_LL hook with a non-negative code,
            // lparam points at a KBDLLHOOKSTRUCT for the lifetime of this call.
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let key = key_of(info.vkCode);

            // Recover from a poisoned lock rather than panicking: a panic here
            // unwinds through a Win32 callback, which is undefined behaviour.
            let mut watcher = state
                .watcher
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let fired = match wparam.0 as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => watcher.key_down(key),
                WM_KEYUP | WM_SYSKEYUP => {
                    watcher.key_up(key);
                    Fired::Nothing
                }
                _ => Fired::Nothing,
            };

            // Dropped before the callback: `on_fire` must not be able to
            // re-enter the watcher and deadlock the input path.
            drop(watcher);

            if let Fired::Chord(index) = fired {
                (state.on_fire)(index);
            }
        }

        // Always. The shortcut still reaches the rest of the system; nothing is
        // swallowed. See the module documentation.
        // SAFETY: the next hook in the chain, with our arguments unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    pub fn install<F>(watcher: ChordWatcher, on_fire: F) -> Result<Installed, PlatformError>
    where
        F: Fn(usize) + Send + 'static,
    {
        // `Fn(usize) + Send` from the caller, stored as `Send + Sync` because the
        // hook procedure reaches it through a `static`. Wrapping in a mutex-free
        // way needs `Sync`, so the closure is boxed behind one.
        struct Sendable<F>(F);
        // SAFETY: the closure is only ever called from the single hook thread.
        // `Sync` is required by `OnceLock` and is not exercised.
        unsafe impl<F: Send> Sync for Sendable<F> {}

        impl<F: Fn(usize)> Sendable<F> {
            fn call(&self, index: usize) {
                (self.0)(index);
            }
        }

        let sendable = Sendable(on_fire);
        let boxed: Box<dyn Fn(usize) + Send + Sync> = Box::new(move |i| sendable.call(i));

        STATE
            .set(HookState {
                watcher: Mutex::new(watcher),
                on_fire: boxed,
            })
            .map_err(|_| {
                PlatformError::Unsupported(
                    "a keyboard hook is already installed. Windows allows one per process, so \
                     changing the watched shortcuts means restarting the application."
                        .to_owned(),
                )
            })?;

        // The hook has to be installed on a thread with a message loop, because
        // Windows delivers low-level hook callbacks by dispatching to the
        // installing thread's queue. Installing it on the main thread would put
        // every keystroke on the machine behind the UI thread.
        let (ready, started) = mpsc::channel::<Result<u32, String>>();

        let join = thread::Builder::new()
            .name("kw-keyhook".to_owned())
            .spawn(move || {
                // SAFETY: a null module handle and thread id 0 install a global
                // low-level hook, which is the documented use of
                // WH_KEYBOARD_LL.
                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), None, 0) };

                let hook: HHOOK = match hook {
                    Ok(hook) => hook,
                    Err(err) => {
                        let _ = ready.send(Err(err.to_string()));
                        return;
                    }
                };

                // SAFETY: no arguments.
                let id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
                if ready.send(Ok(id)).is_err() {
                    // Nobody is waiting, so nobody owns this hook.
                    // SAFETY: unhooking a hook this thread installed.
                    unsafe {
                        let _ = UnhookWindowsHookEx(hook);
                    }
                    return;
                }

                let mut message = MSG::default();
                // SAFETY: standard message loop. `GetMessageW` returns 0 on
                // WM_QUIT, which is what `Installed::drop` posts.
                unsafe {
                    while GetMessageW(&mut message, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    let _ = UnhookWindowsHookEx(hook);
                }
            })
            .map_err(|e| {
                PlatformError::Unsupported(format!("could not start a hook thread: {e}"))
            })?;

        match started.recv() {
            Ok(Ok(thread_id)) => Ok(Installed {
                thread_id,
                join: Some(join),
            }),
            Ok(Err(reason)) => Err(PlatformError::Unsupported(format!(
                "Windows refused the keyboard hook: {reason}. This usually means another \
                 application at a higher integrity level owns the input path."
            ))),
            Err(_) => Err(PlatformError::Unsupported(
                "the hook thread stopped before reporting whether it installed".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The support report matches the platform this is built for.
    #[test]
    fn support_matches_the_platform() {
        let support = support();
        assert_eq!(support.is_available(), cfg!(target_os = "windows"));

        if let HookSupport::Unsupported { reason } = support {
            // A reason has to name what would be needed, or it is not a reason.
            assert!(
                reason.contains("CGEventTap") || reason.contains("Wayland"),
                "{reason}"
            );
        }
    }

    /// Nothing to watch is refused rather than installed.
    ///
    /// A hook in the input path of every keystroke on the machine, watching
    /// nothing, is pure cost.
    #[test]
    fn installing_with_no_chords_is_refused() {
        let err = KeyHook::install(&[], |_| {}).expect_err("must refuse");
        assert!(format!("{err}").contains("nothing to hook"), "{err}");
    }

    /// A chord the hook has no business claiming is refused before installation.
    ///
    /// `Control+Shift+D` belongs to the platform's shortcut API, which arbitrates
    /// it against other applications. Claiming it here as well would mean two
    /// mechanisms firing for one keystroke.
    #[test]
    fn a_chord_with_an_ordinary_key_is_refused_before_the_hook_is_installed() {
        let err =
            KeyHook::install(&["Control+Shift+D".to_owned()], |_| {}).expect_err("must refuse");
        assert!(format!("{err}").contains("not a modifier"), "{err}");
    }
}
