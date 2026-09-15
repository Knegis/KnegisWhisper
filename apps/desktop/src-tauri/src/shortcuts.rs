//! Global shortcut registration.
//!
//! One central handler, as Pluely does — it dispatches by action id and forwards
//! anything it does not recognise rather than guessing. That shape is worth
//! copying; what is not worth copying is registering blindly.
//!
//! Registration here reports per-accelerator outcomes. An accelerator that the
//! platform refuses (already taken by another application, most commonly) comes
//! back as [`Registration::Rejected`] with the reason, so the settings screen can
//! say "Alt+D is in use by something else" instead of leaving a hotkey that
//! silently does nothing.

use std::str::FromStr as _;

use kw_platform::hotkey::{Binding, HotkeyAction, HotkeyMode};
use kw_platform::keyhook::Edge;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, Shortcut, ShortcutState};
use ts_rs::TS;

/// The event emitted when a hotkey fires.
pub const HOTKEY_EVENT: &str = "hotkey";

pub fn capture_active(app: &AppHandle) -> bool {
    app.try_state::<std::sync::Arc<Hooks>>()
        .is_some_and(|hooks| {
            hooks
                .capture_paused
                .load(std::sync::atomic::Ordering::SeqCst)
        })
}

/// Temporarily release global shortcuts so the focused capture input receives
/// them. A lease restores them even if the frontend closes or crashes.
#[tauri::command]
pub async fn hotkey_capture_start(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<u64, String> {
    if window.label() != "main" || !window.is_visible().map_err(|e| e.to_string())? {
        return Err("Open the shortcut settings window to record a shortcut.".into());
    }
    let hooks = app.state::<std::sync::Arc<Hooks>>();
    if hooks.portal_enabled {
        return Err(
            "Use your desktop's shortcut settings while the shortcut portal is enabled.".into(),
        );
    }
    let bindings = crate::commands::load_bindings(&app.state::<crate::state::AppState>()).await?;
    let token = {
        let mut capture = hooks.capture.lock().map_err(|e| e.to_string())?;
        if capture.is_some() {
            return Err("A shortcut is already being recorded.".into());
        }
        hooks
            .capture_paused
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Err(error) = app.global_shortcut().unregister_all() {
            hooks
                .capture_paused
                .store(false, std::sync::atomic::Ordering::SeqCst);
            register_all(&app, &bindings, &hooks);
            return Err(error.to_string());
        }
        crate::dictation::release_the_hold();
        *hooks
            .installed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let token = hooks
            .capture_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        *capture = Some((token, bindings));
        token
    };
    let expiry_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        if let Err(error) = finish_capture(&expiry_app, token) {
            tracing::warn!(%error, "could not restore shortcuts after capture timeout");
        }
    });
    Ok(token)
}

fn finish_capture(app: &AppHandle, token: u64) -> Result<(), String> {
    let hooks = app.state::<std::sync::Arc<Hooks>>();
    let mut capture = hooks.capture.lock().map_err(|e| e.to_string())?;
    if capture.as_ref().map(|(current, _)| *current) != Some(token) {
        return Ok(());
    }
    if let Some((_, bindings)) = capture.take() {
        let reports = register_all(app, &bindings, &hooks);
        hooks
            .capture_paused
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if reports
            .iter()
            .flat_map(|r| &r.results)
            .any(|r| matches!(r, Registration::Rejected { .. }))
        {
            return Err(
                "Some shortcuts could not be restored. Check their status in Shortcuts.".into(),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub fn hotkey_capture_end(
    app: AppHandle,
    window: tauri::WebviewWindow,
    token: u64,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Shortcut capture belongs to the settings window.".into());
    }
    finish_capture(&app, token)
}

/// What happened when we tried to register one accelerator.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Registration {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Portal {
        accelerator: String,
        granted: Option<String>,
    },
    Registered {
        accelerator: String,
    },
    /// Live through the keyboard hook rather than the system shortcut register.
    ///
    /// A separate state from `Registered` because the two behave differently in
    /// a way the user can notice: a hooked chord does not take the keystroke
    /// away from other applications, so `Control+Super` still opens whatever
    /// Windows opens. Reporting it as plain "registered" would be a small lie
    /// that surfaces the first time somebody wonders why the Start menu appeared.
    Hooked {
        accelerator: String,
    },
    /// The platform refused it. Usually another application already owns it.
    Rejected {
        accelerator: String,
        reason: String,
    },
    /// Not registrable in principle — modifier-only where there is no hook, or
    /// malformed.
    NotRegistrable {
        accelerator: String,
        reason: String,
    },
    /// Nothing in the application handles this action, so the accelerator was
    /// deliberately never offered to the platform.
    ///
    /// A separate state from `NotRegistrable` because the cause differs and so
    /// does the fix. `NotRegistrable` is about the *accelerator* — modifier-only,
    /// or malformed — and the user can fix it by typing a different one. This is
    /// about the *action*: the accelerator is faultless and there is nothing to
    /// type that would help, because the feature does not exist yet.
    ///
    /// It is also why the chord is left alone rather than claimed. Registering a
    /// global shortcut for an action nothing handles takes those keys away from
    /// every other application in exchange for nothing at all.
    Unhandled {
        accelerator: String,
        reason: String,
    },
}

impl Registration {
    /// Whether this accelerator will actually fire *and* reach a handler.
    ///
    /// Both mechanisms count, and only those two. The distinction between the
    /// register and the hook is for the user to read, not for the "does my
    /// hotkey work" question to hinge on — but `Unhandled` is not a third
    /// mechanism, it is the absence of one, and counting it here is precisely
    /// the lie this state was added to end.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(
            self,
            Self::Registered { .. } | Self::Hooked { .. } | Self::Portal { .. }
        )
    }
}

/// The outcome of registering every configured hotkey.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationReport {
    pub action: HotkeyAction,
    pub results: Vec<Registration>,
}

impl RegistrationReport {
    /// Whether this action ended up with a working hotkey.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.results.iter().any(Registration::succeeded)
    }
}

/// The payload delivered to the frontend when a hotkey fires.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyFired {
    pub action: HotkeyAction,
    pub accelerator: String,
}

/// What handles an action when its hotkey fires, or why nothing does.
///
/// The one place that knows, and that is the point. Both the registration path
/// and the fire path read this function, and the defect it replaces was those
/// two disagreeing: the dispatch was `if action == HotkeyAction::Dictation`, so
/// the other four actions registered a real OS shortcut, took the chord away
/// from every other application, and then fell off the end of an `if`. A user
/// could bind chat agent, press it, and get silence.
///
/// Naming the handler rather than holding it is what keeps the two in step: this
/// enum has one variant per handler, so every `match` on it — the register path
/// and the hook path both — stops compiling the moment a variant is added. The
/// compiler enforces at each site what a boolean could only promise.
///
/// It is data and not a `fn(&AppHandle)` for a measured reason. A unit test that
/// merely took the address of the handler made the whole test binary
/// unloadable: keeping the pointer alive keeps the Tauri runtime it reaches
/// alive, and a `cargo test` executable has no WebView2 beside it, so the
/// process died at load with `STATUS_ENTRYPOINT_NOT_FOUND` before a single test
/// ran. `let _f: fn(&AppHandle) = crate::dictation::on_hotkey;` in a test is
/// enough to reproduce it. So the decision is data, testable on its own, and the
/// pointer is never named anywhere but the two call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dispatch {
    /// Handled in Rust, so it works whether or not a window is listening.
    Dictation,
    /// Nothing handles it yet, and this is the sentence saying so.
    Missing(&'static str),
}

const fn dispatch(action: HotkeyAction) -> Dispatch {
    match action {
        // In Rust rather than the frontend. The dashboard can be hidden, closed
        // to the tray, or still starting up, and a hotkey that only works while
        // a window is listening is not a global hotkey.
        HotkeyAction::Dictation => Dispatch::Dictation,
        HotkeyAction::ChatAgent => Dispatch::Missing("The chat agent is not built yet."),
        HotkeyAction::VoiceAgent => Dispatch::Missing("The voice agent is not built yet."),
        HotkeyAction::Translation => Dispatch::Missing("Translation is not built yet."),
        HotkeyAction::Meeting => Dispatch::Missing("Meeting capture is not built yet."),
    }
}

/// What one edge of one accelerator should do, given the action's mode.
///
/// **The whole of push-to-talk, and pure.** Everything else this feature needed
/// was plumbing — a second hook, a second settings key, a release the watcher
/// could report — and this is the only place a decision is made. It is a `const
/// fn` over two small enums so it can be tested without a Tauri runtime, a
/// hook, or a microphone, which is the split the rest of this module already
/// draws between `dispatch` and the handler it names.
///
/// The four cases are written out rather than reduced to "act unless it is a
/// release in toggle mode", so that adding a third mode has to be thought about
/// here rather than falling into whichever arm the wildcard covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Start if idle, finish if recording. What a press has always done.
    Toggle,
    /// Start, and finish again when the key comes up.
    BeginHold,
    /// The key came up: whatever `BeginHold` started should end.
    EndHold,
    /// Nothing at all.
    Nothing,
}

fn hooked_act(edge: Edge, mode: HotkeyMode, accelerator: &str) -> Act {
    if mode == HotkeyMode::Toggle
        && !kw_platform::keys::names_a_mouse_button(accelerator)
        && edge == Edge::Press
    {
        Act::Nothing
    } else {
        act(edge, mode)
    }
}

const fn act(edge: Edge, mode: HotkeyMode) -> Act {
    match (edge, mode) {
        // Unchanged, and it has to be: this is the arm the shipped default
        // takes, and KNEG-115 is about not deciding that one in passing.
        (Edge::Press, HotkeyMode::Toggle) => Act::Toggle,
        (Edge::Press, HotkeyMode::Hold) => Act::BeginHold,
        (Edge::Release, HotkeyMode::Hold) => Act::EndHold,
        // Under a toggle the key coming up means nothing, and has to keep
        // meaning nothing: acting on it would double every press.
        (Edge::Release, HotkeyMode::Toggle) => Act::Nothing,
        (Edge::Activated, HotkeyMode::Toggle) => Act::Toggle,
        (Edge::Activated, HotkeyMode::Hold) => Act::Nothing,
    }
}

/// Whether this edge does anything at all.
///
/// Used to decide whether to emit the `hotkey` event as well as to dispatch, so
/// the event means "the hotkey did something" rather than "a key moved". Under a
/// toggle, the release does not appear at all — it never did, and a screen
/// suddenly showing two "last fired" lines per press would read as a regression.
const fn does_something(action: Act) -> bool {
    !matches!(action, Act::Nothing)
}

/// Why nothing handles this action, or `None` if something does.
///
/// Exposed because the settings screen has to say it once per *action*: "nothing
/// handles the chat agent" is a fact about the action, and repeating it under
/// each of its accelerators would just be repeating it.
#[must_use]
pub const fn unhandled_reason(action: HotkeyAction) -> Option<&'static str> {
    match dispatch(action) {
        Dispatch::Dictation => None,
        Dispatch::Missing(why) => Some(why),
    }
}

/// Register every accelerator in `bindings`.
///
/// Unregisters everything first, so calling this after a settings change is the
/// whole update — there is no incremental diff to get wrong.
pub fn register_all(
    app: &AppHandle,
    bindings: &[Binding],
    hooks: &Hooks,
) -> Vec<RegistrationReport> {
    #[cfg(target_os = "linux")]
    if hooks.portal_enabled {
        return portal_runtime::register(app, bindings, hooks);
    }
    let manager = app.global_shortcut();

    if let Err(err) = manager.unregister_all() {
        // Not fatal: on first run there is nothing registered.
        tracing::debug!(error = %err, "nothing to unregister");
    }

    // Modifier-only chords go to the keyboard hook, which the system shortcut
    // register cannot take. Installed once for all of them, because Windows
    // allows one low-level hook per process.
    let hooked = hooks.install(app, bindings);

    let mut reports = Vec::with_capacity(bindings.len());

    for binding in bindings {
        let mut results = Vec::new();

        // An action nothing handles is not offered to the platform at all, and
        // every one of its accelerators says why. This comes before the
        // classifier because the classification does not matter here: a
        // perfectly good accelerator for an action with no handler is still
        // dead, and registering it would claim the chord from other
        // applications in exchange for nothing.
        if let Some(reason) = unhandled_reason(binding.action) {
            for accelerator in &binding.accelerators {
                results.push(Registration::Unhandled {
                    accelerator: accelerator.text().to_owned(),
                    reason: format!(
                        "{reason} The shortcut is deliberately not registered, so other \
                         applications can still use the same keys."
                    ),
                });
            }
            reports.push(RegistrationReport {
                action: binding.action,
                results,
            });
            continue;
        }

        // Everything the classifier already rejected, reported as-is rather
        // than attempted and failed.
        for accelerator in &binding.accelerators {
            match accelerator {
                kw_platform::hotkey::Accelerator::Standard { .. } => {}
                // Live through the hook rather than the register.
                kw_platform::hotkey::Accelerator::ModifierOnly { accelerator, .. }
                    if hooked.contains(accelerator) =>
                {
                    results.push(Registration::Hooked {
                        accelerator: accelerator.clone(),
                    });
                }
                other => results.push(Registration::NotRegistrable {
                    accelerator: other.text().to_owned(),
                    reason: other.explain(),
                }),
            }
        }

        for accelerator in binding.registrable() {
            results.push(register_one(app, binding.action, accelerator, binding.mode));
        }

        reports.push(RegistrationReport {
            action: binding.action,
            results,
        });
    }

    for report in &reports {
        if report.is_live() {
            continue;
        }

        // An action nothing handles is a designed state, not a fault to
        // investigate. Warning about it would put four warnings in every log on
        // every launch, all saying what the plan already says, which is how a
        // log stops being read. The condition is read from `handler` rather than
        // inferred from the results, so it cannot disagree with what was done.
        if unhandled_reason(report.action).is_some() {
            tracing::debug!(
                action = %report.action,
                "nothing handles this action, so its hotkey was not registered"
            );
            continue;
        }

        // Every reason, not just the fact. A count told me all five actions
        // had failed and nothing about why, which is useless: "the platform
        // refused it because another application owns it" and "this is two
        // modifiers and no key" need completely different fixes, and only one
        // of them is the user's to make.
        for result in &report.results {
            match result {
                Registration::Registered { .. }
                | Registration::Hooked { .. }
                | Registration::Portal { .. } => {}
                Registration::Rejected {
                    accelerator,
                    reason,
                } => tracing::warn!(
                    action = %report.action,
                    %accelerator,
                    %reason,
                    "the platform refused this accelerator"
                ),
                Registration::NotRegistrable {
                    accelerator,
                    reason,
                }
                | Registration::Unhandled {
                    accelerator,
                    reason,
                } => tracing::warn!(
                    action = %report.action,
                    %accelerator,
                    %reason,
                    "this accelerator cannot be registered at all"
                ),
            }
        }
        tracing::warn!(
            action = %report.action,
            "no working hotkey for this action"
        );
    }

    hooks.set_reports(reports.clone());
    reports
}

/// Owns the keyboard hook across re-registrations.
///
/// Windows allows one low-level hook per process, so this cannot be a local in
/// `register_all` — a settings change would try to install a second one and be
/// refused. Held in application state instead, and replaced rather than added to.
#[derive(Default)]
pub struct Hooks {
    capture: std::sync::Mutex<Option<(u64, Vec<Binding>)>>,
    capture_sequence: std::sync::atomic::AtomicU64,
    capture_paused: std::sync::atomic::AtomicBool,
    pub portal_enabled: bool,
    pub reports: std::sync::Arc<std::sync::Mutex<Vec<RegistrationReport>>>,
    #[cfg(target_os = "linux")]
    portal_task: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    #[cfg(target_os = "linux")]
    portal_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    #[cfg(target_os = "linux")]
    portal_gate: std::sync::Arc<std::sync::Mutex<()>>,
    installed: std::sync::Mutex<Option<kw_platform::keyhook::KeyHook>>,
}

#[cfg(target_os = "linux")]
impl Drop for Hooks {
    fn drop(&mut self) {
        self.portal_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(task) = self
            .portal_task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let watching = self
            .installed
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|h| h.watching().to_vec()));
        f.debug_struct("Hooks")
            .field("watching", &watching)
            .finish()
    }
}

impl Hooks {
    pub fn new(portal_enabled: bool) -> Self {
        Self {
            capture: Default::default(),
            capture_sequence: Default::default(),
            capture_paused: Default::default(),
            portal_enabled,
            reports: Default::default(),
            installed: Default::default(),
            #[cfg(target_os = "linux")]
            portal_task: Default::default(),
            #[cfg(target_os = "linux")]
            portal_generation: Default::default(),
            #[cfg(target_os = "linux")]
            portal_gate: Default::default(),
        }
    }

    /// Release the portal connection before desktop shutdown.
    #[cfg(target_os = "linux")]
    pub fn stop_portal(&self) {
        let _guard = self
            .portal_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.portal_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(task) = self
            .portal_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
        crate::dictation::release_the_hold();
    }

    pub fn reports(&self) -> Vec<RegistrationReport> {
        self.reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set_reports(&self, reports: Vec<RegistrationReport>) {
        *self
            .reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = reports;
    }

    /// Install a hook for every modifier-only accelerator in `bindings`.
    ///
    /// Returns the accelerators that are now live through it. An accelerator
    /// that is not in the returned set is not hooked, whatever the reason, so a
    /// caller cannot report one as working on the strength of having asked.
    fn install(&self, app: &AppHandle, bindings: &[Binding]) -> std::collections::BTreeSet<String> {
        use std::collections::BTreeSet;

        // One flat list, paired with the action and mode it belongs to, because
        // the hook reports an index rather than a name.
        let mut wanted: Vec<(HotkeyAction, String, HotkeyMode)> = Vec::new();
        for binding in bindings {
            // An action nothing handles gets no hook either. A hook watching a
            // chord for a dead action costs a comparison on every keystroke on
            // the machine and delivers an event nothing acts on.
            if unhandled_reason(binding.action).is_some() {
                continue;
            }
            for accelerator in &binding.accelerators {
                // Both hooked kinds, in one list, in the order they were
                // configured. `KeyHook::install` reports the index into exactly
                // this list, so a mouse button and a chord cannot be told apart
                // by position here and do not need to be.
                match accelerator {
                    kw_platform::hotkey::Accelerator::ModifierOnly { accelerator, .. }
                    | kw_platform::hotkey::Accelerator::MouseButton { accelerator, .. } => {
                        wanted.push((binding.action, accelerator.clone(), binding.mode));
                    }
                    kw_platform::hotkey::Accelerator::Standard { .. }
                    | kw_platform::hotkey::Accelerator::Invalid { .. } => {}
                }
            }
        }

        // A push-to-talk hold in progress is ended before the hook that would
        // have ended it goes away. Its release is watched by the hook being
        // dropped on the next line, so it will never arrive -- somebody
        // changing a setting while holding the dictation key would otherwise be
        // left recording with nothing to stop it.
        crate::dictation::release_the_hold();

        // Drop and join the old hook before installing its replacement so
        // only the current binding and mode can dispatch input.
        let mut held = self
            .installed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *held = None;

        if wanted.is_empty() {
            return BTreeSet::new();
        }

        let accelerators: Vec<String> = wanted.iter().map(|(_, a, _)| a.clone()).collect();
        let app = app.clone();
        let routes = wanted.clone();

        match kw_platform::keyhook::KeyHook::install(&accelerators, move |index, edge| {
            if capture_active(&app) {
                return;
            }
            // Runs on the hook thread, which is in the input path of every
            // keystroke on the machine. Everything here has to return promptly.
            let Some((action, accelerator, mode)) = routes.get(index) else {
                return;
            };

            let what = hooked_act(edge, *mode, accelerator);
            if !does_something(what) {
                return;
            }

            let payload = HotkeyFired {
                action: *action,
                accelerator: accelerator.clone(),
            };
            if let Err(err) = app.emit(HOTKEY_EVENT, payload) {
                tracing::warn!(error = %err, "could not deliver a hooked hotkey");
            }

            // The same dispatch the register path runs. Emitting and stopping
            // is what this did when the hook first landed, which made
            // `Control+Super` announce itself in the interface and dictate
            // nothing -- the hook fired, so it looked verified, and the
            // handler was never reached. The handler spawns, so this returns
            // promptly enough for the input path.
            match dispatch(*action) {
                Dispatch::Dictation => crate::dictation::on_hotkey(&app, what),
                Dispatch::Missing(_) => {}
            }
        }) {
            Ok(hook) => {
                tracing::info!(?accelerators, "keyboard hook installed");
                *held = Some(hook);
                accelerators.into_iter().collect()
            }
            Err(err) => {
                // Reported, and the accelerators stay unhooked, so they come
                // back as `NotRegistrable` with a reason rather than as working.
                tracing::warn!(error = %err, ?accelerators, "could not install the keyboard hook");
                BTreeSet::new()
            }
        }
    }
}

/// Register one accelerator with the platform's own shortcut API.
///
/// **The release half here is believed, not measured.** The plugin delivers a
/// `Released` state and this routes it through the same [`act`] the hook path
/// uses, so push-to-talk on an ordinary accelerator like `Control+Shift+D`
/// should work — but nothing in this repository has run it. Reaching it needs a
/// built application with a WebView2 beside it and a person holding a key, which
/// is the same by-hand check `docs/PLATFORM_SUPPORT.md` records as outstanding
/// for the sided chords. The hook path, by contrast, is measured:
/// `cargo run -p kw-platform --example keyhook_probe` reports two presses and
/// two releases for two injected chords.
fn register_one(
    app: &AppHandle,
    action: HotkeyAction,
    accelerator: &str,
    mode: HotkeyMode,
) -> Registration {
    let shortcut = match Shortcut::from_str(accelerator) {
        Ok(s) => s,
        Err(err) => {
            return Registration::NotRegistrable {
                accelerator: accelerator.to_owned(),
                reason: format!("the shortcut plugin could not parse it: {err}"),
            }
        }
    };

    let app_for_handler = app.clone();
    let accelerator_owned = accelerator.to_owned();

    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if capture_active(&app_for_handler) {
                return;
            }
            // Which edge, rather than "press only". The unconditional filter
            // that used to live here was right for a toggle -- without it every
            // hotkey fired twice, starting and immediately restarting the
            // recording -- and it is exactly what push-to-talk needs *not* to
            // do. `act` is the one place that knows which, and the hook path
            // reads the same function.
            let edge = match event.state() {
                ShortcutState::Pressed => Edge::Press,
                ShortcutState::Released => Edge::Release,
            };

            let what = act(edge, mode);
            if !does_something(what) {
                return;
            }

            let payload = HotkeyFired {
                action,
                accelerator: accelerator_owned.clone(),
            };

            tracing::debug!(action = %action, ?edge, "hotkey fired");

            if let Err(err) = app_for_handler.emit(HOTKEY_EVENT, payload) {
                tracing::warn!(error = %err, "could not deliver the hotkey event");
            }

            // Dispatch through the same table that decided this accelerator was
            // worth registering at all. The event above is emitted either way,
            // so the UI can still show that a key fired.
            match dispatch(action) {
                Dispatch::Dictation => crate::dictation::on_hotkey(&app_for_handler, what),
                // Unreachable: an unhandled action is never registered, so
                // nothing can fire. Doing nothing is still the truthful
                // response, and this runs in the input path where a panic
                // would take the hotkey system down with it.
                Dispatch::Missing(_) => {}
            }
        });

    match result {
        Ok(()) => Registration::Registered {
            accelerator: accelerator.to_owned(),
        },
        Err(err) => Registration::Rejected {
            accelerator: accelerator.to_owned(),
            // Almost always "already registered by another application".
            reason: err.to_string(),
        },
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    #[test]
    fn recorded_standard_key_names_are_accepted_by_the_shortcut_parser() {
        use std::str::FromStr as _;
        for key in [
            "D",
            "1",
            "ArrowLeft",
            "Numpad1",
            "Space",
            "Tab",
            "Backquote",
            "BracketLeft",
            "NumpadEnter",
        ] {
            assert!(
                tauri_plugin_global_shortcut::Shortcut::from_str(&format!("Control+{key}")).is_ok(),
                "{key}"
            );
        }
    }
    use super::*;

    #[test]
    fn modifier_toggle_dispatch_waits_for_qualified_activation() {
        assert_eq!(
            hooked_act(Edge::Press, HotkeyMode::Toggle, "Control+Super"),
            Act::Nothing
        );
        assert_eq!(
            hooked_act(Edge::Release, HotkeyMode::Toggle, "Control+Super"),
            Act::Nothing
        );
        assert_eq!(
            hooked_act(Edge::Activated, HotkeyMode::Toggle, "Control+Super"),
            Act::Toggle
        );
        assert_eq!(
            hooked_act(Edge::Press, HotkeyMode::Toggle, "Mouse4"),
            Act::Toggle
        );
        assert_eq!(
            hooked_act(Edge::Press, HotkeyMode::Hold, "Control+Super"),
            Act::BeginHold
        );
        assert_eq!(
            hooked_act(Edge::Release, HotkeyMode::Hold, "Control+Super"),
            Act::EndHold
        );
        assert_eq!(
            hooked_act(Edge::Activated, HotkeyMode::Hold, "Control+Super"),
            Act::Nothing
        );
    }

    #[test]
    fn only_registered_counts_as_success() {
        assert!(Registration::Registered {
            accelerator: "Alt+D".to_owned()
        }
        .succeeded());
        assert!(!Registration::Rejected {
            accelerator: "Alt+D".to_owned(),
            reason: "taken".to_owned()
        }
        .succeeded());
        assert!(!Registration::NotRegistrable {
            accelerator: "Control+Super".to_owned(),
            reason: "modifier only".to_owned()
        }
        .succeeded());
    }

    #[test]
    fn nothing_handles_an_action_that_is_not_dictation() {
        // A deliberate tripwire. When the chat agent gains a handler this test
        // fails, which is the prompt to update the plan and PLATFORM_SUPPORT.md
        // in the same commit -- rather than leaving them describing a state that
        // has ended, which is how four dead hotkeys survived this long.
        let handled: Vec<HotkeyAction> = kw_platform::hotkey::ALL_ACTIONS
            .into_iter()
            .filter(|a| unhandled_reason(*a).is_none())
            .collect();
        assert_eq!(handled, vec![HotkeyAction::Dictation]);
    }

    #[test]
    fn every_unhandled_action_gives_a_whole_sentence() {
        // The reason is concatenated into a user-facing string, so a fragment or
        // a missing full stop lands in the interface as one.
        for action in kw_platform::hotkey::ALL_ACTIONS {
            let Some(reason) = unhandled_reason(action) else {
                continue;
            };
            assert!(reason.ends_with('.'), "{action}: {reason}");
            assert!(
                reason.starts_with(|c: char| c.is_uppercase()),
                "{action}: {reason}"
            );
        }
    }

    #[test]
    fn an_unhandled_action_is_dead_however_good_its_accelerator_is() {
        // `Control+Shift+Space` is perfectly registrable. The *action* is what is
        // dead, and this is the assertion that was false before: it registered,
        // reported success, and nothing happened when it fired.
        let report = RegistrationReport {
            action: HotkeyAction::ChatAgent,
            results: vec![Registration::Unhandled {
                accelerator: "Control+Shift+Space".to_owned(),
                reason: "The chat agent is not built yet.".to_owned(),
            }],
        };
        assert!(!report.is_live());
        assert!(!report.results[0].succeeded());
    }

    #[test]
    fn the_dispatch_and_the_registration_read_the_same_table() {
        // The pair this table exists to hold together. If dictation ever stopped
        // naming a handler, the registration path would go on offering it to the
        // platform -- which is the shape of the original defect in reverse.
        assert_eq!(dispatch(HotkeyAction::Dictation), Dispatch::Dictation);
        assert!(matches!(
            dispatch(HotkeyAction::Meeting),
            Dispatch::Missing(_)
        ));

        // And this is deliberately as far as a test goes. Naming the handler
        // itself -- `let _f: fn(&AppHandle) = crate::dictation::on_hotkey;` --
        // keeps the Tauri runtime alive in the test binary, which then cannot
        // load at all: every test in this crate dies at
        // STATUS_ENTRYPOINT_NOT_FOUND before any of them runs. Measured, not
        // guessed. Test the decision; leave the pointer to the call sites.
    }

    #[test]
    fn an_action_with_only_rejections_is_not_live() {
        let report = RegistrationReport {
            action: HotkeyAction::Dictation,
            results: vec![
                Registration::NotRegistrable {
                    accelerator: "Control+Super".to_owned(),
                    reason: "modifier only".to_owned(),
                },
                Registration::Rejected {
                    accelerator: "Alt+D".to_owned(),
                    reason: "already registered".to_owned(),
                },
            ],
        };
        assert!(!report.is_live());
    }

    #[test]
    fn one_success_among_failures_makes_the_action_live() {
        let report = RegistrationReport {
            action: HotkeyAction::Dictation,
            results: vec![
                Registration::NotRegistrable {
                    accelerator: "Control+Super".to_owned(),
                    reason: "modifier only".to_owned(),
                },
                Registration::Registered {
                    accelerator: "Alt+D".to_owned(),
                },
            ],
        };
        assert!(report.is_live());
    }

    #[test]
    fn the_shortcut_plugin_agrees_with_our_classifier_on_ordinary_accelerators() {
        // Guards against the classifier accepting a syntax the plugin rejects,
        // which would show as "registered" in settings and do nothing.
        for accelerator in [
            "Control+Shift+D",
            "Alt+D",
            "CommandOrControl+Space",
            "Control+Shift+M",
            "Control+Shift+T",
        ] {
            assert!(
                kw_platform::hotkey::classify(accelerator).is_registrable(),
                "our classifier rejected {accelerator}"
            );
            assert!(
                Shortcut::from_str(accelerator).is_ok(),
                "the plugin rejected {accelerator}, which our classifier accepted"
            );
        }
    }

    #[test]
    fn the_classifier_is_never_stricter_than_the_platform() {
        // The test above guards one direction: we must not accept what the
        // plugin refuses, or settings shows a registered hotkey that does
        // nothing. This guards the other, which cost a user a failed bench
        // check: `Ctrl+Shift+D` registers perfectly well, and we were calling it
        // invalid with the reason "it names 2 non-modifier keys" -- a message
        // that blames the spelling for our own missing alias.
        //
        // Casing is included because the plugin ignores it and we preserve
        // whatever was typed, so anyone typing naturally must still be right.
        for accelerator in [
            "Ctrl+Shift+D",
            "control+shift+d",
            "CONTROL+SHIFT+D",
            "Control+Shift+d",
            "Ctrl+Alt+Space",
        ] {
            assert!(
                Shortcut::from_str(accelerator).is_ok(),
                "the premise changed: the plugin now refuses {accelerator}"
            );
            assert!(
                kw_platform::hotkey::classify(accelerator).is_registrable(),
                "we refuse {accelerator}, which the platform registers"
            );
        }
    }

    #[test]
    fn the_plugin_also_refuses_modifier_only_accelerators() {
        // The premise behind reporting Control+Super as unregistrable. If the
        // plugin ever gains support for it, this test fails and tells us to
        // revisit the classifier rather than leaving a stale limitation.
        let modifier_only = Shortcut::from_str("Control+Super");
        assert!(
            modifier_only.is_err(),
            "the plugin now accepts modifier-only accelerators; revisit hotkey::classify"
        );
    }

    /* ------------------------------------------------------- push-to-talk */

    /// A toggle behaves exactly as it did: the press acts, the release does not.
    ///
    /// The arm that carries the shipped default. If push-to-talk ever changed
    /// what a toggle does, `Control+Super` would double-fire on every press —
    /// which is the defect the old unconditional `state() != Pressed` filter was
    /// there to prevent, and this is that filter's replacement being held to the
    /// same promise.
    #[test]
    fn a_toggle_acts_on_the_press_and_ignores_the_release() {
        assert_eq!(act(Edge::Press, HotkeyMode::Toggle), Act::Toggle);
        assert_eq!(act(Edge::Release, HotkeyMode::Toggle), Act::Nothing);
    }

    /// A hold acts on both edges, and they are different acts.
    ///
    /// `BeginHold` and `EndHold` are not two names for `Toggle`: the press has
    /// to arrange for the release to be waited on in the same task, because two
    /// separately spawned tasks have no order between them and the release would
    /// otherwise be free to run before the microphone had finished opening.
    #[test]
    fn a_hold_acts_on_both_edges_and_they_are_not_the_same_act() {
        assert_eq!(act(Edge::Press, HotkeyMode::Hold), Act::BeginHold);
        assert_eq!(act(Edge::Release, HotkeyMode::Hold), Act::EndHold);
        assert_ne!(act(Edge::Press, HotkeyMode::Hold), Act::Toggle);
    }

    /// Exactly one of the four cases does nothing, and it is the right one.
    ///
    /// Written as a sweep over every mode and edge rather than as three
    /// assertions, so a third `HotkeyMode` gets counted here the moment
    /// `ALL_MODES` grows -- the same tripwire shape as
    /// `nothing_handles_an_action_that_is_not_dictation`.
    #[test]
    fn only_a_release_under_a_toggle_does_nothing() {
        let mut idle = Vec::new();
        for mode in kw_platform::hotkey::ALL_MODES {
            for edge in [Edge::Press, Edge::Release] {
                let what = act(edge, mode);
                assert_eq!(does_something(what), what != Act::Nothing);
                if !does_something(what) {
                    idle.push((mode, edge));
                }
            }
        }
        assert_eq!(idle, vec![(HotkeyMode::Toggle, Edge::Release)]);
    }

    /// A press always does something, whatever the mode.
    ///
    /// The half a user would notice immediately: a mode that swallowed its own
    /// press would be a hotkey that does nothing at all, which is the state this
    /// whole module exists to stop reporting as success.
    #[test]
    fn no_mode_swallows_its_own_press() {
        for mode in kw_platform::hotkey::ALL_MODES {
            assert!(
                does_something(act(Edge::Press, mode)),
                "{mode} ignores its press"
            );
        }
    }

    #[test]
    fn events_serialize_with_the_keys_the_frontend_expects() {
        let json = serde_json::to_value(HotkeyFired {
            action: HotkeyAction::Dictation,
            accelerator: "Alt+D".to_owned(),
        })
        .unwrap();
        assert_eq!(json["action"], "dictation");
        assert_eq!(json["accelerator"], "Alt+D");
    }
}

#[cfg(target_os = "linux")]
#[path = "shortcut_portal.rs"]
mod portal_runtime;
