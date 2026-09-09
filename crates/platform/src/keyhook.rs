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
//! native Windows tests and probes exercise separately.
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
//!
//! # The mouse hook
//!
//! A second hook, `WH_MOUSE_LL`, **on the same thread**, and only when a mouse
//! button is actually bound. Both callbacks share thread-local state; the
//! thread is shared because it
//! already owns a message loop and a `WM_QUIT` shutdown path, and a second
//! thread would be a second thing to get wrong at exit for no capability.
//!
//! Only when bound, because `WH_MOUSE_LL` sits in the path of every mouse
//! *move* — hundreds a second while the pointer is in use — and the callback
//! returns immediately for anything that is not an X-button message. The
//! keyboard hook has no equivalent escape, because every keystroke is a
//! candidate; the mouse one does, and takes it.
//!
//! The encoding is **measured, not read**: `mouseData`'s high word is 1 for
//! `XBUTTON1` and 2 for `XBUTTON2`, and `SendInput` takes the same numbers
//! *unshifted* — an asymmetry that is exactly where this goes wrong. See
//! `cargo run -p kw-platform --example mousehook_probe`.

use crate::keys::Watched;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::keys::{Fired, HookWatcher};
use crate::PlatformError;

/// Which edge of a press an event is.
///
/// Reported to the caller rather than resolved here, because what a release
/// *means* is a setting — push-to-talk versus toggle — and this module has no
/// business knowing which one is configured. It reports what the hardware did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Press,
    Release,
    Activated,
}

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
            mechanism: "low-level keyboard and mouse hooks on a dedicated thread",
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Asked, not assumed. A tap is created only for a process the user has
        // granted Accessibility to, and `CGEventTapCreate` answers a refusal
        // with a null pointer and no reason -- so the reason has to come from
        // here, before the attempt, or the user is told "it did not work".
        if objc2_core_graphics::CGPreflightListenEventAccess() {
            HookSupport::Available {
                mechanism: "CGEventTap on a dedicated CFRunLoop thread",
            }
        } else {
            HookSupport::Unsupported {
                reason: "macOS has not granted this application permission to observe input, so \
                         modifier-only shortcuts and mouse buttons cannot be watched. Grant it in \
                         System Settings > Privacy & Security > Accessibility, then restart \
                         KnegisWhisper. Ordinary accelerators with a letter or number in them are \
                         unaffected: those are registered through the system shortcut API and \
                         need no permission."
                    .to_owned(),
            }
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        HookSupport::Unsupported {
            reason: "modifier-only shortcuts and mouse buttons need low-level input hooks, and \
                     this platform has none. X11 has no way to observe a modifier-only chord \
                     globally without grabbing the keyboard from every other application; \
                     Wayland has no equivalent at all, and its global-shortcuts portal takes \
                     only chords the compositor itself can register -- never a bare pair of \
                     modifiers. Ordinary accelerators with a letter or number in them are \
                     unaffected and go through the portal or the system shortcut API."
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
    /// The same arrangement on macOS: dropping it stops the run loop, which
    /// lets the thread drop the tap. Never read for the same reason.
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    inner: macos_impl::Installed,
    /// The chords being watched, so a caller can report what is live.
    watching: Vec<String>,
}

impl KeyHook {
    /// Install a hook watching `accelerators`, calling `on_fire` with the index
    /// of whichever fired and which edge it was.
    ///
    /// An accelerator is either a modifier-only chord or a mouse button; the
    /// index is into the list as given, so a caller with one list of
    /// accelerators does not have to work out which of two lists an index came
    /// from. See [`Watched`].
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
        F: Fn(usize, Edge) + Send + 'static,
    {
        if accelerators.is_empty() {
            return Err(PlatformError::Unsupported(
                "no modifier-only shortcuts are configured, so there is nothing to hook".to_owned(),
            ));
        }

        let mut watched = Vec::with_capacity(accelerators.len());
        for accelerator in accelerators {
            watched.push(
                Watched::parse(accelerator)
                    .map_err(|e| PlatformError::Unsupported(e.to_string()))?,
            );
        }

        #[cfg(target_os = "windows")]
        {
            let inner = windows_impl::install(HookWatcher::new(watched), on_fire)?;
            Ok(Self {
                inner,
                watching: accelerators.to_vec(),
            })
        }

        #[cfg(target_os = "macos")]
        {
            let inner = macos_impl::install(HookWatcher::new(watched), on_fire)?;
            Ok(Self {
                inner,
                watching: accelerators.to_vec(),
            })
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = (watched, on_fire);
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
    use std::cell::OnceCell;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::thread;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageA, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
        WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
        WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
    };

    use super::{Edge, Fired, HookWatcher, PlatformError};
    use crate::keys::{Key, MouseButton};

    // Low-level callbacks run on the installing thread. Keeping their state
    // there makes every installation independent and drops the callback and
    // pending gesture when that thread exits, including failed installations.
    struct HookState {
        watcher: Mutex<HookWatcher>,
        on_fire: Box<dyn Fn(usize, Edge) + Send>,
    }

    thread_local! {
        static STATE: OnceCell<HookState> = const { OnceCell::new() };
    }

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

        STATE.with(|slot| {
            if let Some(state) = slot.get() {
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
                    WM_KEYUP | WM_SYSKEYUP => watcher.key_up(key),
                    _ => Fired::Nothing,
                };

                // Dropped before the callback: `on_fire` must not be able to
                // re-enter the watcher and deadlock the input path.
                drop(watcher);

                report(state, fired);
            }
        });

        // Always. The shortcut still reaches the rest of the system; nothing is
        // swallowed. See the module documentation.
        // SAFETY: the next hook in the chain, with our arguments unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Which side button `mouseData` names, if it names one.
    ///
    /// **Measured, not read off a header.** `mousehook_probe` injects
    /// `XBUTTON1` and `XBUTTON2` and reports `mouseData` arriving as
    /// `0x0001_0000` and `0x0002_0000`: the button number is in the *high*
    /// word. `SendInput` takes the same two numbers in the *low* word, and that
    /// asymmetry is the whole reason this was probed rather than reasoned out —
    /// a decoder that read the low word would find zero for every click and
    /// report a mouse that never has side buttons.
    const fn button_of(mouse_data: u32) -> Option<MouseButton> {
        match ((mouse_data >> 16) & 0xFFFF) as u16 {
            XBUTTON1 => Some(MouseButton::Four),
            XBUTTON2 => Some(MouseButton::Five),
            // A third X button is possible in the API and has never been seen.
            // Ignored rather than guessed at: reporting it as button 5 would be
            // inventing hardware.
            _ => None,
        }
    }

    /// The mouse hook procedure.
    ///
    /// Installed only when a mouse button is bound, and it still returns
    /// immediately for everything that is not an X-button message — this runs
    /// for every mouse *move* on the machine, which is the highest-frequency
    /// event Windows has.
    ///
    /// It never inspects the pointer position. `MSLLHOOKSTRUCT` carries where
    /// the mouse is, on every event, in every application; nothing here reads
    /// `pt`, for the same reason nothing in the keyboard procedure reads which
    /// key was pressed.
    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            // SAFETY: passing the arguments straight on, per the API contract.
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        let message = wparam.0 as u32;
        if message == WM_XBUTTONDOWN || message == WM_XBUTTONUP {
            STATE.with(|slot| {
                if let Some(state) = slot.get() {
                    // SAFETY: for a WH_MOUSE_LL hook with a non-negative code,
                    // lparam points at an MSLLHOOKSTRUCT for the lifetime of this
                    // call.
                    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };

                    if let Some(button) = button_of(info.mouseData) {
                        let mut watcher = state
                            .watcher
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);

                        let fired = if message == WM_XBUTTONDOWN {
                            watcher.button_down(button)
                        } else {
                            watcher.button_up(button)
                        };

                        drop(watcher);
                        report(state, fired);
                    }
                }
            });
        }

        // Always, and for the same reason the keyboard hook does: the click
        // still reaches whatever is under the pointer. On these two buttons
        // that means a browser still goes Back or Forward, which is stated on
        // the settings screen rather than left to be discovered.
        // SAFETY: the next hook in the chain, with our arguments unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Hand one decision to the caller, on the hook thread.
    ///
    /// Shared by both procedures so the two cannot come to differ about what
    /// counts as worth reporting.
    fn report(state: &HookState, fired: Fired) {
        match fired {
            Fired::Pressed(index) => (state.on_fire)(index, Edge::Press),
            Fired::Released(index) => (state.on_fire)(index, Edge::Release),
            Fired::Activated(index) => (state.on_fire)(index, Edge::Activated),
            Fired::Nothing => {}
        }
    }

    pub fn install<F>(watcher: HookWatcher, on_fire: F) -> Result<Installed, PlatformError>
    where
        F: Fn(usize, Edge) + Send + 'static,
    {
        // Read before the watcher is moved into its thread: whether a mouse
        // hook is needed is a property of what is being watched, and asking
        // afterwards would mean reaching back through a lock from the thread
        // that is about to install it.
        let needs_mouse = watcher.watches_a_button();

        // The hook has to be installed on a thread with a message loop, because
        // Windows delivers low-level hook callbacks by dispatching to the
        // installing thread's queue. Installing it on the main thread would put
        // every keystroke on the machine behind the UI thread.
        let (ready, started) = mpsc::channel::<Result<u32, String>>();

        let join = thread::Builder::new()
            .name("kw-keyhook".to_owned())
            .spawn(move || {
                STATE.with(|slot| {
                    let _ = slot.set(HookState {
                        watcher: Mutex::new(watcher),
                        on_fire: Box::new(on_fire),
                    });
                });
                // Create the queue before publishing the thread ID so an
                // immediate drop can always post WM_QUIT successfully.
                let mut queued = MSG::default();
                // SAFETY: a valid MSG pointer; this creates this thread's queue.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                        &mut queued,
                        None,
                        0,
                        0,
                        windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE,
                    );
                }

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

                // The second hook, on this thread, and only when something is
                // bound to a mouse button. A failure here is fatal to the whole
                // installation rather than half-succeeding: a caller told the
                // hook installed, whose mouse binding silently does nothing, is
                // the "half a path" defect this module's history is made of.
                let mouse: Option<HHOOK> = if needs_mouse {
                    // SAFETY: as above, for WH_MOUSE_LL.
                    match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) } {
                        Ok(mouse) => Some(mouse),
                        Err(err) => {
                            // SAFETY: unhooking the keyboard hook this thread
                            // just installed, before giving up on both.
                            unsafe {
                                let _ = UnhookWindowsHookEx(hook);
                            }
                            let _ = ready.send(Err(format!("the mouse hook was refused: {err}")));
                            return;
                        }
                    }
                } else {
                    None
                };

                // SAFETY: no arguments.
                let id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
                if ready.send(Ok(id)).is_err() {
                    // Nobody is waiting, so nobody owns these hooks.
                    // SAFETY: unhooking hooks this thread installed.
                    unsafe {
                        let _ = UnhookWindowsHookEx(hook);
                        if let Some(mouse) = mouse {
                            let _ = UnhookWindowsHookEx(mouse);
                        }
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
                    if let Some(mouse) = mouse {
                        let _ = UnhookWindowsHookEx(mouse);
                    }
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
                "Windows refused an input hook: {reason}. This usually means another \
                 application at a higher integrity level owns the input path."
            ))),
            Err(_) => Err(PlatformError::Unsupported(
                "the hook thread stopped before reporting whether it installed".to_owned(),
            )),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::cell::OnceCell;
    use std::ffi::c_void;
    use std::ptr::{null_mut, NonNull};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
    use objc2_core_graphics::{
        CGEvent, CGEventField, CGEventMask, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventTapProxy, CGEventType,
    };

    use super::{Edge, Fired, HookWatcher, PlatformError};
    use crate::macos_input::{button_of, key_of, FlagTracker, Transition};

    /// The tap callback's state, on the thread that installed it.
    ///
    /// Thread-local for the reason `windows_impl` gives: it makes every
    /// installation independent and drops with the thread, including a failed
    /// one.
    struct TapState {
        watcher: Mutex<HookWatcher>,
        /// Which sided modifiers are down. Separate from `watcher` because
        /// macOS reports modifiers through `flagsChanged`, which carries no
        /// direction -- see [`crate::macos_input::FlagTracker`].
        flags: Mutex<FlagTracker>,
        /// The tap itself, so the callback can re-enable it after the system
        /// disables it for being slow. Held here rather than passed through
        /// `user_info` because the path that needs it is the one path with no
        /// event to read it from.
        tap: OnceCell<CFRetained<CFMachPort>>,
        on_fire: Box<dyn Fn(usize, Edge) + Send>,
    }

    thread_local! {
        static STATE: OnceCell<TapState> = const { OnceCell::new() };
    }

    /// A running tap, and the thread whose run loop delivers to it.
    pub struct Installed {
        /// Set before the run loop is asked to stop, and read by the thread on
        /// both sides of `CFRunLoop::run`.
        ///
        /// The flag as well as the stop, because `run` has not necessarily
        /// been entered when `drop` fires: a `stop` arriving in that window is
        /// simply discarded by Core Foundation, and the thread would then block
        /// forever on a loop nobody can reach. Checked before and after, so
        /// whichever order the two happen in the loop exits.
        stop: Arc<AtomicBool>,
        /// The thread's run loop, so `drop` can stop it.
        run_loop: SendRunLoop,
        join: Option<thread::JoinHandle<()>>,
    }

    /// A `CFRunLoop` handle that may cross threads.
    ///
    /// Wrapped so the justification sits on the type rather than in a comment
    /// at one call site: Core Foundation documents `CFRunLoop` as the only
    /// thread-safe CF class, and stopping one from another thread is the
    /// documented way to end it. Nothing else about the tap is sent anywhere --
    /// the `CFMachPort` and the run-loop source are created, used and dropped
    /// on the tap thread.
    struct SendRunLoop(CFRetained<CFRunLoop>);

    // SAFETY: the only method ever called through this is `CFRunLoop::stop`,
    // and Core Foundation documents run loops as usable from any thread.
    unsafe impl Send for SendRunLoop {}

    impl std::fmt::Debug for Installed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Installed")
                .field("stopping", &self.stop.load(Ordering::SeqCst))
                .finish_non_exhaustive()
        }
    }

    impl Drop for Installed {
        fn drop(&mut self) {
            // Order matters: the flag first, so a stop that lands before the
            // loop is entered cannot be lost.
            self.stop.store(true, Ordering::SeqCst);
            self.run_loop.0.stop();
            if let Some(join) = self.join.take() {
                // Not waited on for long, for the reason `windows_impl` gives:
                // leaking a thread at shutdown beats hanging the application.
                let _ = join.join();
            }
        }
    }

    /// The bit for one event type in a `CGEventMask`.
    const fn mask_bit(event: CGEventType) -> CGEventMask {
        1u64 << (event.0 as u64)
    }

    /// Hand one decision to the caller, on the tap thread.
    ///
    /// Identical in shape to `windows_impl::report` and deliberately so: the
    /// two platforms must not come to differ about what counts as worth
    /// reporting, because `shortcuts::act` reads the result of both.
    fn report(state: &TapState, fired: Fired) {
        match fired {
            Fired::Pressed(index) => (state.on_fire)(index, Edge::Press),
            Fired::Released(index) => (state.on_fire)(index, Edge::Release),
            Fired::Activated(index) => (state.on_fire)(index, Edge::Activated),
            Fired::Nothing => {}
        }
    }

    /// The tap callback.
    ///
    /// Runs in the delivery path of every event on the machine, so it does as
    /// little as possible and **always returns the event unchanged**. Nothing
    /// is swallowed and nothing is rewritten: the tap is created `ListenOnly`
    /// so the window server would ignore a modification anyway, and the policy
    /// is the one `windows_impl` states -- an application that silently eats a
    /// system shortcut is indistinguishable from a broken keyboard.
    ///
    /// It never logs a key. See the module documentation.
    unsafe extern "C-unwind" fn callback(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: NonNull<CGEvent>,
        _user_info: *mut c_void,
    ) -> *mut CGEvent {
        STATE.with(|slot| {
            if let Some(state) = slot.get() {
                // SAFETY: a tap callback is handed a live event for the
                // duration of the call.
                let borrowed = unsafe { event.as_ref() };
                handle(state, event_type, borrowed);
            }
        });

        // Unchanged, always.
        event.as_ptr()
    }

    /// Decide what one event means, with the event already borrowed.
    ///
    /// Split from [`callback`] so the `unsafe` there stays down to the single
    /// dereference it needs. Every lock recovers from poisoning rather than
    /// panicking, because a panic here would unwind through a C callback.
    fn handle(state: &TapState, event_type: CGEventType, event: &CGEvent) {
        // The system disables a tap that takes too long, and disables it again
        // when the user's own input arrives during one. Both mean this tap has
        // missed events it will never see, so both are recovered the same way.
        if event_type == CGEventType::TapDisabledByTimeout
            || event_type == CGEventType::TapDisabledByUserInput
        {
            if let Some(tap) = state.tap.get() {
                CGEvent::tap_enable(tap, true);
            }
            state
                .flags
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            state
                .watcher
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .reset();
            return;
        }

        let mut watcher = state
            .watcher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let fired = match event_type {
            // A modifier press is not a key event on this platform. This arm
            // is the entire reason a tap can do what the cross-platform
            // shortcut API cannot.
            CGEventType::FlagsChanged => {
                let key = key_of(keycode(event));
                let transition = state
                    .flags
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .transition(key);
                match transition {
                    Transition::Pressed => watcher.key_down(key),
                    Transition::Released => watcher.key_up(key),
                }
            }
            // Ordinary keys matter only so rule 2 can see that one is down and
            // cancel a modifier-only candidate. Which key it was is never used
            // and never recorded.
            CGEventType::KeyDown => watcher.key_down(key_of(keycode(event))),
            CGEventType::KeyUp => watcher.key_up(key_of(keycode(event))),
            CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
                let number =
                    CGEvent::integer_value_field(Some(event), CGEventField::MouseEventButtonNumber);
                match (button_of(number), event_type) {
                    (Some(button), CGEventType::OtherMouseDown) => watcher.button_down(button),
                    (Some(button), _) => watcher.button_up(button),
                    (None, _) => Fired::Nothing,
                }
            }
            _ => Fired::Nothing,
        };

        // Dropped before the callback, so `on_fire` cannot re-enter the watcher
        // and deadlock the input path.
        drop(watcher);
        report(state, fired);
    }

    /// The physical key an event names.
    ///
    /// The side comes from here rather than from `CGEventGetFlags`, whose
    /// public masks are unsided -- see [`crate::macos_input::Transition`].
    fn keycode(event: &CGEvent) -> u32 {
        let raw = CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
        // A keycode is a `u16` in every Apple header that declares one; the
        // accessor is the generic integer one and so is typed `i64`. Clamped
        // rather than cast blindly: a negative or enormous value is not a key,
        // and `Key::Other` on a wrapped number would be a phantom key that
        // rule 2 could match.
        u32::try_from(raw).unwrap_or(u32::MAX)
    }

    pub fn install<F>(watcher: HookWatcher, on_fire: F) -> Result<Installed, PlatformError>
    where
        F: Fn(usize, Edge) + Send + 'static,
    {
        // Read before the watcher moves into its thread, for the reason
        // `windows_impl` gives.
        let needs_mouse = watcher.watches_a_button();

        // The tap has to be created on a thread with a run loop, because that
        // is how Core Graphics delivers to it -- the direct counterpart of a
        // Win32 low-level hook needing the installing thread's message queue.
        // On the main thread it would put every event on the machine behind the
        // UI thread.
        let (ready, started) = mpsc::channel::<Result<SendRunLoop, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);

        let join = thread::Builder::new()
            .name("kw-keyhook".to_owned())
            .spawn(move || {
                let outcome = STATE.with(|slot| {
                    let mut events = mask_bit(CGEventType::FlagsChanged)
                        | mask_bit(CGEventType::KeyDown)
                        | mask_bit(CGEventType::KeyUp);
                    if needs_mouse {
                        // Only when a button is bound, for the reason the
                        // module doc gives about the Windows mouse hook: a tap
                        // that asks for mouse events is in the path of every
                        // one of them, movement included.
                        events |= mask_bit(CGEventType::OtherMouseDown)
                            | mask_bit(CGEventType::OtherMouseUp);
                    }

                    // SAFETY: `callback` has the signature `CGEventTapCallBack`
                    // requires, and a null `user_info` is permitted -- state
                    // travels through the thread-local instead, so its type
                    // cannot be got wrong at the C boundary.
                    let tap = unsafe {
                        CGEvent::tap_create(
                            // HID rather than session: a session tap sees only
                            // what the window server has already routed, and a
                            // global shortcut has to fire whatever has focus.
                            CGEventTapLocation::HIDEventTap,
                            CGEventTapPlacement::HeadInsertEventTap,
                            // Listen only, so this tap cannot alter or drop an
                            // event even by mistake.
                            CGEventTapOptions::ListenOnly,
                            events,
                            Some(callback),
                            null_mut(),
                        )
                    };

                    // The one failure this API gives no reason for: a refused
                    // permission and a genuine error are the same null.
                    let Some(tap) = tap else {
                        return Err(match super::support() {
                            super::HookSupport::Unsupported { reason } => reason,
                            super::HookSupport::Available { .. } => {
                                "macOS refused to create an event tap although it reports \
                                 permission to observe input. Nothing further is known: \
                                 CGEventTapCreate returns null without a reason."
                                    .to_owned()
                            }
                        });
                    };

                    let source =
                        CFMachPort::new_run_loop_source(None, Some(&tap), 0).ok_or_else(|| {
                            "the event tap could not be turned into a run-loop source, so no \
                             event would ever have been delivered to it"
                                .to_owned()
                        })?;

                    let run_loop = CFRunLoop::current().ok_or_else(|| {
                        "this thread has no run loop, so an event tap cannot be attached to it"
                            .to_owned()
                    })?;
                    // SAFETY: reading a mode constant the framework exports.
                    let mode = unsafe { kCFRunLoopCommonModes };
                    run_loop.add_source(Some(&source), mode);
                    CGEvent::tap_enable(&tap, true);

                    let handle = SendRunLoop(run_loop);
                    let state = TapState {
                        watcher: Mutex::new(watcher),
                        flags: Mutex::new(FlagTracker::new()),
                        tap: OnceCell::new(),
                        on_fire: Box::new(on_fire),
                    };
                    if state.tap.set(tap).is_err() {
                        return Err("the event tap was set twice on one thread".to_owned());
                    }
                    if slot.set(state).is_err() {
                        return Err("this thread already has an event tap installed".to_owned());
                    }
                    Ok((handle, source))
                });

                match outcome {
                    Err(reason) => {
                        let _ = ready.send(Err(reason));
                    }
                    Ok((handle, source)) => {
                        let _ = ready.send(Ok(handle));
                        // Checked on both sides of `run`: see `Installed::stop`.
                        // `run` also returns on its own once every source is
                        // gone, which is why this is a loop rather than one
                        // call.
                        while !thread_stop.load(Ordering::SeqCst) {
                            CFRunLoop::run();
                        }
                        // Dropped on the thread that created it.
                        drop(source);
                    }
                }
            })
            .map_err(|e| {
                PlatformError::Unsupported(format!("the event tap thread could not start: {e}"))
            })?;

        match started.recv() {
            Ok(Ok(run_loop)) => Ok(Installed {
                stop,
                run_loop,
                join: Some(join),
            }),
            Ok(Err(reason)) => {
                let _ = join.join();
                Err(PlatformError::Unsupported(reason))
            }
            Err(_) => {
                let _ = join.join();
                Err(PlatformError::Unsupported(
                    "the event tap thread stopped before it reported whether it installed"
                        .to_owned(),
                ))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Settings replace the hook for both accelerator and mode changes.
    #[cfg(target_os = "windows")]
    #[test]
    fn native_hook_can_be_replaced_after_immediate_drop() {
        for accelerator in ["Control+Super", "RightControl+RightShift", "Control+Super"] {
            let hook = KeyHook::install(&[accelerator.to_owned()], |_, _| {})
                .expect("each settings update must install a fresh hook");
            drop(hook);
        }
    }

    /// The support report matches the platform this is built for.
    ///
    /// Three answers now rather than two (KNEG-62). Windows is unconditional;
    /// macOS depends on a permission and so cannot be asserted either way from
    /// a `cfg`, which is the point of asking `CGPreflightListenEventAccess`
    /// rather than assuming; everything else has no mechanism at all.
    #[test]
    fn support_matches_the_platform() {
        let support = support();

        if cfg!(target_os = "windows") {
            assert!(
                support.is_available(),
                "Windows has the hook unconditionally"
            );
        } else if cfg!(target_os = "macos") {
            // Either answer is correct. What must hold is that a refusal is
            // about the permission and says how to grant it, rather than
            // claiming macOS cannot do this at all -- which is what it used to
            // say and is no longer true.
            if let HookSupport::Unsupported { reason } = &support {
                assert!(reason.contains("Accessibility"), "{reason}");
                assert!(reason.contains("System Settings"), "{reason}");
            }
        } else {
            assert!(!support.is_available(), "no mechanism exists here");
            let HookSupport::Unsupported { reason } = &support else {
                unreachable!("just asserted it is unavailable")
            };
            // A reason has to name what would be needed, or it is not a reason.
            assert!(reason.contains("Wayland"), "{reason}");
        }
    }

    /// Nothing to watch is refused rather than installed.
    ///
    /// A hook in the input path of every keystroke on the machine, watching
    /// nothing, is pure cost.
    #[test]
    fn installing_with_no_chords_is_refused() {
        let err = KeyHook::install(&[], |_, _| {}).expect_err("must refuse");
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
            KeyHook::install(&["Control+Shift+D".to_owned()], |_, _| {}).expect_err("must refuse");
        assert!(format!("{err}").contains("not a modifier"), "{err}");
    }
}
