//! The XDG global-shortcuts portal: what it will take, and what it refuses.
//!
//! The Linux half of KNEG-62. It is **not** a hook and is not the counterpart
//! of [`crate::keyhook`], which is the thing most likely to be misread about
//! this module. A `WH_KEYBOARD_LL` hook and a `CGEventTap` observe every event
//! on the machine and decide for themselves what counts; the portal is a
//! *registration* API — the application asks the compositor for a shortcut and
//! the compositor either grants it, gives the user a chance to change it, or
//! says no. So it can never do the one thing the hooks exist for: a bare pair
//! of modifiers with no key in it.
//!
//! That asymmetry is the reason this is a separate module rather than a third
//! arm of `keyhook::support`. Putting it there would have made
//! `HookSupport::Available` true on Wayland for a mechanism that cannot watch
//! `Control+Super`, and the whole point of that type is to tell the user which
//! of their shortcuts actually work.
//!
//! # Epistemic status, stated plainly
//!
//! Three different things live here and they are **not** equally trustworthy:
//!
//! * [`trigger_for`] and [`PortalRefusal`] are pure functions over strings,
//!   unit-tested on every platform including the one this was written on. The
//!   trigger *syntax* they produce is read from the portal specification and
//!   has never been handed to a real portal.
//! * The Linux client has a private-bus regression test, but a real compositor
//!   still needs to validate its consent UI, grants and activation delivery.
//! * The desktop consumes it only when `hotkey.usePortal` is enabled and the
//!   application restarts. The existing plugin remains the default backend.
//!
//! # What "per compositor" means here
//!
//! There is one portal interface and several implementations behind it, and
//! they disagree about what they will grant. `docs/PLATFORM_SUPPORT.md` keeps a
//! row per compositor for that reason: an accelerator this module formats
//! correctly can still be refused by GNOME and accepted by KDE, and the portal
//! reports the *granted* trigger back rather than the requested one — which is
//! why [`Bound`] carries what the compositor actually gave, not what was asked
//! for.

use std::fmt;

/// Why the portal cannot be asked for a particular accelerator.
///
/// A refusal before the D-Bus call rather than after: every one of these is
/// knowable from the accelerator alone, and finding out from a compositor
/// would mean an error the user sees seconds later with no explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalRefusal {
    /// A bare pair of modifiers. The portal's trigger grammar has no way to
    /// express it, so this is not a limitation of one compositor.
    ModifierOnly {
        /// The accelerator as the user typed it.
        accelerator: String,
    },
    /// A modifier on one particular side. The grammar has `CTRL`, and nothing
    /// that distinguishes left from right.
    SidedModifier {
        accelerator: String,
        /// The spelling that named a side, so the message can quote it.
        spelling: String,
    },
    /// A mouse button. The portal is a keyboard-shortcut interface.
    MouseButton { accelerator: String },
    /// No key at all after the modifiers were taken off, or none this
    /// vocabulary can name.
    Unnameable {
        accelerator: String,
        reason: &'static str,
    },
}

impl fmt::Display for PortalRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModifierOnly { accelerator } => write!(
                f,
                "{accelerator} is modifier-only, and the desktop portal's shortcut grammar has no \
                 way to express a chord with no key in it. On Windows and macOS a low-level input \
                 hook handles this; there is no equivalent here, so add an ordinary key"
            ),
            Self::SidedModifier {
                accelerator,
                spelling,
            } => write!(
                f,
                "{accelerator} names a particular side ({spelling}), and the desktop portal's \
                 grammar has only CTRL, ALT, SHIFT and LOGO with no side to them. Drop the side, \
                 or use a different key"
            ),
            Self::MouseButton { accelerator } => write!(
                f,
                "{accelerator} is a mouse button, and the desktop portal registers keyboard \
                 shortcuts only"
            ),
            Self::Unnameable {
                accelerator,
                reason,
            } => write!(
                f,
                "{accelerator} cannot be sent to the desktop portal: {reason}"
            ),
        }
    }
}

impl std::error::Error for PortalRefusal {}

/// The four modifier names the portal grammar has, in the order a trigger
/// spells them.
///
/// `LOGO` is the portal's name for what this project calls Super and macOS
/// calls Command. Written out rather than derived by a rule, because "Super"
/// and "LOGO" have no letters in common and a rule would be a lie about how
/// the mapping works.
const PORTAL_MODIFIERS: [&str; 4] = ["CTRL", "SHIFT", "ALT", "LOGO"];

/// Every spelling this project accepts for a portal modifier.
///
/// **One table, not a primary set plus an alias set.** Two tables is exactly
/// how a spelling comes to be accepted by one reader and refused by another —
/// the failure `keys::SIDED_MODIFIERS` documents at length, and the reason
/// KNEG-35 shipped a hookable chord no user could reach. The aliases are here
/// because `Meta`, `Win`, `Windows`, `Cmd` and `Command` all reach a user's
/// fingers from somewhere, and refusing a spelling the settings screen itself
/// displays would be a refusal they cannot act on.
const MODIFIER_SPELLINGS: [(&str, &str); 11] = [
    ("control", "CTRL"),
    ("ctrl", "CTRL"),
    ("shift", "SHIFT"),
    ("alt", "ALT"),
    ("option", "ALT"),
    ("super", "LOGO"),
    ("meta", "LOGO"),
    ("win", "LOGO"),
    ("windows", "LOGO"),
    ("cmd", "LOGO"),
    ("command", "LOGO"),
];

/// The portal's trigger string for one of this project's accelerators.
///
/// # Errors
///
/// Every [`PortalRefusal`], each of which is a property of the accelerator and
/// not of the machine — so the answer is the same on GNOME, KDE and Hyprland.
/// A trigger this returns may still be refused by a particular compositor;
/// that is a different answer and arrives over D-Bus.
pub fn trigger_for(accelerator: &str) -> Result<String, PortalRefusal> {
    let owned = accelerator.to_owned();

    if crate::keys::names_a_mouse_button(accelerator) {
        return Err(PortalRefusal::MouseButton { accelerator: owned });
    }

    let mut modifiers = Vec::new();
    let mut key = None;

    for part in accelerator.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return Err(PortalRefusal::Unnameable {
                accelerator: owned,
                reason: "it has an empty segment, so one of its keys is missing",
            });
        }
        let lower = part.to_ascii_lowercase();

        if crate::keys::sided_modifier(&lower).is_some() && unsided_name(&lower).is_none() {
            return Err(PortalRefusal::SidedModifier {
                accelerator: owned,
                spelling: part.to_owned(),
            });
        }

        if let Some(name) = unsided_name(&lower) {
            // Repeats are dropped rather than refused: `Control+Control+D` is
            // a strange thing to type and means something unambiguous.
            if !modifiers.contains(&name) {
                modifiers.push(name);
            }
            continue;
        }

        let Some(named) = portal_key(&lower) else {
            return Err(PortalRefusal::Unnameable {
                accelerator: owned,
                reason: "only letters and digits are recognised as the key of a portal trigger \
                         in this build; function keys and punctuation are not mapped yet",
            });
        };
        if key.replace(named).is_some() {
            return Err(PortalRefusal::Unnameable {
                accelerator: owned,
                reason: "it has two ordinary keys in it, and a trigger takes one",
            });
        }
    }

    let Some(key) = key else {
        return Err(PortalRefusal::ModifierOnly { accelerator: owned });
    };

    // Modifiers first and in the grammar's own order, so two spellings of one
    // chord produce one trigger. A compositor comparing strings would
    // otherwise treat `Shift+Control+D` and `Control+Shift+D` as two
    // different shortcuts competing for the same keys.
    let mut ordered: Vec<&str> = PORTAL_MODIFIERS
        .iter()
        .filter_map(|portal| modifiers.contains(portal).then_some(*portal))
        .collect();
    ordered.push(&key);
    Ok(ordered.join("+"))
}

/// The portal modifier name for an unsided spelling, if that is what this is.
fn unsided_name(lower: &str) -> Option<&'static str> {
    MODIFIER_SPELLINGS
        .iter()
        .find_map(|(ours, portal)| (*ours == lower).then_some(*portal))
}

/// The portal's name for one ordinary key, if this build can name it.
///
/// Letters and digits go down to lower case, which is what the grammar's
/// keysym names use. Anything else returns `None` rather than being passed
/// through: a trigger the portal cannot parse is rejected by the compositor
/// with no route back to the user, so a name this function does not know is
/// better reported before the call.
///
/// It returns an `Option` and not a [`PortalRefusal`] deliberately. It sees one
/// segment and not the accelerator, and a refusal built here quoted `f5` at a
/// user who had typed `Control+F5` — unactionable the moment more than one
/// shortcut is configured. The caller has the whole string and builds the
/// message; `every_refusal_names_the_accelerator_it_is_about` is what caught
/// that.
fn portal_key(lower: &str) -> Option<String> {
    let mut chars = lower.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_string()),
        _ => None,
    }
}

/// One shortcut as the compositor actually granted it.
///
/// Both triggers are kept, because the portal is entitled to bind something
/// other than what was asked for — the user may be offered the choice, and
/// GNOME has its own shortcut settings panel that overrides it. A screen that
/// showed only the requested trigger would be naming a key that does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The index into the accelerator list this was asked for, so a caller
    /// does not have to match on the id strings that come back over D-Bus.
    pub index: usize,
    /// The accelerator as the user spelled it, for showing beside a granted
    /// trigger that differs from it.
    pub accelerator: String,
    /// The trigger this client asked the portal for, in the portal's grammar.
    pub requested: String,
    /// What the compositor says it registered, if it said. `None` means the
    /// portal returned the shortcut with no trigger described, which the
    /// specification permits and which means "bound, and the user can see it in
    /// their own settings" — not "failed".
    pub granted: Option<String>,
}

/// The D-Bus half: **believed, not confirmed.**
///
/// No session bus, no `xdg-desktop-portal` and no compositor has answered a
/// single call in here. It compiles for `x86_64-unknown-linux-gnu` and that is
/// the entire body of evidence, which is why every claim in this module is
/// written as what the specification says rather than as what happens. Treat it
/// the way `crates/meeting-core`'s realtime clients are treated: a clean-room
/// implementation from published documentation, waiting for the one run that
/// would turn it into knowledge (KNEG-61).
///
/// The two-stage shape is the portal's own, not a choice made here. A method
/// call returns a *request* object path immediately and the real answer arrives
/// later as a `Response` signal on that path, so the signal is subscribed
/// before the reply is read — a portal that answers instantly must not answer
/// into a stream nobody is reading.
#[cfg(target_os = "linux")]
pub mod linux_impl {
    use std::collections::HashMap;

    use zbus::export::futures_util::StreamExt as _;
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
    use zbus::{proxy, Connection};

    use super::{trigger_for, Bound, PortalRefusal};

    /// The portal's success code.
    const RESPONSE_SUCCESS: u32 = 0;
    /// The user dismissed the dialog. Not an error: a person declining to grant
    /// a shortcut is an answer, and reporting it as a failure would invite a
    /// retry loop against someone who has said no.
    const RESPONSE_CANCELLED: u32 = 1;

    #[proxy(
        interface = "org.freedesktop.portal.GlobalShortcuts",
        default_service = "org.freedesktop.portal.Desktop",
        default_path = "/org/freedesktop/portal/desktop"
    )]
    trait GlobalShortcuts {
        #[zbus(property)]
        fn version(&self) -> zbus::Result<u32>;
        #[zbus(signal)]
        fn shortcuts_changed(
            &self,
            session_handle: OwnedObjectPath,
            shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
        ) -> zbus::Result<()>;

        /// Returns the *request* path, not the session. The session handle
        /// arrives in the `Response` results under `session_handle`.
        fn create_session(
            &self,
            options: HashMap<&str, Value<'_>>,
        ) -> zbus::Result<OwnedObjectPath>;

        /// `shortcuts` is the specification's `a(sa{sv})`: an id, and a vardict
        /// carrying `description` and `preferred_trigger`.
        fn bind_shortcuts(
            &self,
            session_handle: &OwnedObjectPath,
            shortcuts: &[(String, HashMap<String, OwnedValue>)],
            parent_window: &str,
            options: HashMap<&str, Value<'_>>,
        ) -> zbus::Result<OwnedObjectPath>;

        #[zbus(signal)]
        fn activated(
            &self,
            session_handle: OwnedObjectPath,
            shortcut_id: String,
            timestamp: u64,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::Result<()>;

        #[zbus(signal)]
        fn deactivated(
            &self,
            session_handle: OwnedObjectPath,
            shortcut_id: String,
            timestamp: u64,
            options: HashMap<String, OwnedValue>,
        ) -> zbus::Result<()>;
    }

    #[proxy(
        interface = "org.freedesktop.portal.Request",
        default_service = "org.freedesktop.portal.Desktop"
    )]
    trait Request {
        #[zbus(signal)]
        fn response(&self, response: u32, results: HashMap<String, OwnedValue>)
            -> zbus::Result<()>;
    }

    /// What went wrong reaching or using the portal.
    ///
    /// Separate from [`PortalRefusal`], which is about an accelerator and gives
    /// the same answer on every machine. Every variant here is about *this*
    /// machine: no portal, too old a portal, or a compositor that said no.
    #[derive(Debug)]
    pub enum PortalError {
        /// No session bus, or nothing implementing the interface on it. The
        /// likely cause is a desktop with no `xdg-desktop-portal`, or one older
        /// than 1.17, which is when `GlobalShortcuts` was added.
        Unreachable(zbus::Error),
        /// A portal is running, but nothing on it implements `GlobalShortcuts`:
        /// reading the interface's `version` was answered as a missing
        /// interface. That is `xdg-desktop-portal` with no backend for it, as
        /// on a desktop whose compositor has none (KNEG-490).
        NoInterface(zbus::Error),
        /// The portal answered, and said no.
        Refused { response: u32 },
        /// The user closed the dialog.
        Cancelled,
        /// The portal answered in a shape this client does not recognise.
        /// Reported rather than defaulted: a missing `session_handle` would
        /// mean every later call was made against nothing.
        Unexpected(&'static str),
        /// Nothing in the list could be expressed as a trigger, with the first
        /// reason as the explanation.
        NothingBindable(PortalRefusal),
    }

    impl std::fmt::Display for PortalError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Unreachable(e) => write!(
                    f,
                    "the desktop portal could not be reached, so global shortcuts cannot be \
                     registered on this desktop ({e}). GlobalShortcuts needs xdg-desktop-portal \
                     1.17 or newer and a compositor backend that implements it"
                ),
                Self::NoInterface(e) => write!(
                    f,
                    "the desktop portal is running but offers no global shortcuts on this \
                     desktop ({e}). GlobalShortcuts needs a compositor backend that implements it"
                ),
                Self::Refused { response } => write!(
                    f,
                    "the desktop portal refused to register the shortcuts (response {response})"
                ),
                Self::Cancelled => write!(f, "the shortcut request was dismissed"),
                Self::Unexpected(what) => write!(
                    f,
                    "the desktop portal answered in a shape this build does not recognise: {what}"
                ),
                Self::NothingBindable(refusal) => write!(f, "{refusal}"),
            }
        }
    }

    impl PortalError {
        /// Only transport/interface absence permits falling back, never consent denial.
        pub fn can_fallback(&self) -> bool {
            match self {
                Self::Unreachable(zbus::Error::MethodError(name, _, _)) => matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.ServiceUnknown"
                        | "org.freedesktop.DBus.Error.NameHasNoOwner"
                        | "org.freedesktop.DBus.Error.UnknownInterface"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                ),
                Self::Unreachable(zbus::Error::Address(_))
                | Self::Unreachable(zbus::Error::InputOutput(_))
                | Self::NoInterface(_) => true,
                _ => false,
            }
        }

        /// What a failed read of the interface's `version` means. It is the
        /// first thing [`bind`] asks, so it is where a portal without
        /// `GlobalShortcuts` shows. GDBus, which `xdg-desktop-portal` uses,
        /// answers a property read on an interface it does not export with
        /// `InvalidArgs` ("No such interface"), not `UnknownInterface`. That was
        /// measured on 2026-10-23 against a GDBus service, after hosted CI's
        /// runner activated its own portal this way (KNEG-490). zbus's servers
        /// answer `UnknownInterface`, and a read of a missing property
        /// `UnknownProperty`. Only this read is classified so: the same names
        /// from a later call mean something else.
        fn from_version_read(error: zbus::Error) -> Self {
            match &error {
                zbus::Error::MethodError(name, _, _)
                    if matches!(
                        name.as_str(),
                        "org.freedesktop.DBus.Error.InvalidArgs"
                            | "org.freedesktop.DBus.Error.UnknownInterface"
                            | "org.freedesktop.DBus.Error.UnknownProperty"
                    ) =>
                {
                    Self::NoInterface(error)
                }
                _ => Self::Unreachable(error),
            }
        }
    }

    impl std::error::Error for PortalError {}

    impl From<zbus::Error> for PortalError {
        fn from(e: zbus::Error) -> Self {
            Self::Unreachable(e)
        }
    }

    /// A live portal session, the shortcuts it holds, and the connection they
    /// came over.
    ///
    /// The connection is kept because the session dies with it: the portal ends
    /// a session when its owner disconnects, so dropping the connection is how
    /// the shortcuts are released, and holding one without the other would
    /// leave a session nothing can close.
    pub struct PortalSession {
        connection: Connection,
        session: OwnedObjectPath,
        bound: Vec<Bound>,
    }

    impl std::fmt::Debug for PortalSession {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PortalSession")
                .field("session", &self.session)
                .field("bound", &self.bound)
                .finish_non_exhaustive()
        }
    }

    impl PortalSession {
        /// What the compositor granted, per accelerator it accepted.
        #[must_use]
        pub fn bound(&self) -> &[Bound] {
            &self.bound
        }

        /// Deliver activations until the connection ends.
        ///
        /// `on_fire` is handed the index into the original accelerator list and
        /// which edge it was, exactly as [`crate::keyhook::KeyHook`] does — so
        /// `shortcuts::act` can read one shape whichever mechanism produced it.
        /// Press and release come from separate portal signals rather than from
        /// one event with a direction, which is why there is no equivalent of
        /// the macOS flag tracking here.
        ///
        /// # Errors
        ///
        /// [`PortalError::Unreachable`] if the signal subscriptions cannot be
        /// made. A signal that fails to decode is skipped rather than ending
        /// the loop: one malformed activation must not take the other
        /// shortcuts down with it.
        pub async fn deliver<F>(&self, on_fire: F) -> Result<(), PortalError>
        where
            F: Fn(usize, crate::keyhook::Edge) + Send,
        {
            self.deliver_with_changes(on_fire, |_| {}).await
        }

        pub async fn deliver_with_changes<F, C>(
            &self,
            on_fire: F,
            on_change: C,
        ) -> Result<(), PortalError>
        where
            F: Fn(usize, crate::keyhook::Edge) + Send,
            C: Fn(&[Bound]) + Send,
        {
            let portal = GlobalShortcutsProxy::new(&self.connection).await?;
            let mut activated = portal.receive_activated().await?;
            let mut deactivated = portal.receive_deactivated().await?;
            let mut changed = portal.receive_shortcuts_changed().await?;
            let session = zbus::Proxy::new(
                &self.connection,
                "org.freedesktop.portal.Desktop",
                self.session.as_str(),
                "org.freedesktop.portal.Session",
            )
            .await?;
            let mut closed = session.receive_signal("Closed").await?;
            let bus = zbus::fdo::DBusProxy::new(&self.connection).await?;
            let mut owners = bus.receive_name_owner_changed().await?;
            // Subscribe before checking ownership so a disappearing service
            // cannot leave a session apparently registered forever.
            let name = zbus::names::BusName::try_from("org.freedesktop.portal.Desktop")
                .map_err(zbus::Error::from)?;
            if !bus.name_has_owner(name).await.map_err(zbus::Error::from)? {
                return Err(PortalError::Unexpected("the shortcut portal exited"));
            }
            let mut current = self.bound.clone();
            loop {
                let event = tokio::select! {
                    signal = activated.next() => match signal {
                        Some(signal) => match signal.args() {
                            Ok(args) if args.session_handle == self.session => Some((args.shortcut_id.clone(), crate::keyhook::Edge::Press)),
                            _ => None,
                        },
                        None => break,
                    },
                    signal = deactivated.next() => match signal {
                        Some(signal) => match signal.args() {
                            Ok(args) if args.session_handle == self.session => Some((args.shortcut_id.clone(), crate::keyhook::Edge::Release)),
                            _ => None,
                        },
                        None => break,
                    },
                    signal = changed.next() => {
                        let Some(signal) = signal else { break; };
                        let (handle, changes) = signal.message().body().deserialize::<(OwnedObjectPath, Vec<(String, HashMap<String, OwnedValue>)>)>()?;
                        if handle == self.session {
                            for (id, values) in changes {
                                if let Some(bound) = current.iter_mut().find(|b| id_for(b.index) == id) {
                                    bound.granted = values.get("trigger_description").and_then(|v| <&str>::try_from(v).ok()).map(str::to_owned);
                                }
                            }
                            on_change(&current);
                        }
                        None
                    },
                    signal = owners.next() => {
                        let Some(signal) = signal else { break; };
                        if let Ok(args) = signal.args() {
                            if args.name.as_str() == "org.freedesktop.portal.Desktop" {
                                break;
                            }
                        }
                        None
                    },
                    _ = closed.next() => break,
                };
                if let Some((id, edge)) = event {
                    if let Some(index) = self.index_of(&id) {
                        on_fire(index, edge);
                    }
                }
            }
            Err(PortalError::Unexpected("the shortcut session closed"))
        }

        /// Which accelerator a portal shortcut id refers to.
        ///
        /// Resolved against what was actually bound rather than by parsing the
        /// id, so an id this session did not ask for is ignored instead of
        /// being turned into an index that would fire the wrong action.
        fn index_of(&self, id: &str) -> Option<usize> {
            self.bound
                .iter()
                .find(|bound| id_for(bound.index) == id)
                .map(|bound| bound.index)
        }
    }

    /// Ask the portal to register `accelerators`, and report what it granted.
    ///
    /// `descriptions` are what the compositor shows in its own shortcut
    /// settings, positionally matched to `accelerators`; a short one names the
    /// action rather than the application, since the user already knows which
    /// application is asking.
    ///
    /// Accelerators the grammar cannot express are **skipped, not fatal** — one
    /// modifier-only shortcut in a list of five must not cost the other four —
    /// and each is returned with its reason so the caller can say why on
    /// screen. Only when nothing at all is bindable does this fail, because
    /// then there is nothing to report except the reason.
    ///
    /// # Errors
    ///
    /// [`PortalError`], every variant of which is about this machine rather
    /// than about the accelerators.
    pub async fn bind(
        accelerators: &[String],
        descriptions: &[String],
    ) -> Result<(PortalSession, Vec<(usize, PortalRefusal)>), PortalError> {
        let mut wanted = Vec::new();
        let mut refused = Vec::new();
        for (index, accelerator) in accelerators.iter().enumerate() {
            match trigger_for(accelerator) {
                Ok(trigger) => wanted.push((index, accelerator.clone(), trigger)),
                Err(refusal) => refused.push((index, refusal)),
            }
        }

        if wanted.is_empty() {
            return Err(refused.into_iter().next().map_or(
                PortalError::Unexpected("no accelerators were given"),
                |(_, refusal)| PortalError::NothingBindable(refusal),
            ));
        }

        let connection = Connection::session().await?;
        let portal = GlobalShortcutsProxy::new(&connection).await?;
        let _version = portal
            .version()
            .await
            .map_err(PortalError::from_version_read)?;

        let mut create_options = HashMap::new();
        let handle_token = token();
        let session_token = token();
        create_options.insert("handle_token", Value::from(handle_token.as_str()));
        create_options.insert("session_handle_token", Value::from(session_token.as_str()));

        let request_proxy = response_proxy(&connection, &handle_token).await?;
        let response = request_proxy.receive_response().await?;
        let request = portal.create_session(create_options).await?;
        if request.as_str() != request_proxy.inner().path().as_str() {
            return Err(PortalError::Unexpected("unexpected create request path"));
        }
        let results = await_response(response).await?;
        let session =
            results
                .get("session_handle")
                .and_then(session_path)
                .ok_or(PortalError::Unexpected(
                "the session was created without a session_handle, so no shortcut could be bound \
                 to it",
            ))?;

        let shortcuts: Vec<(String, HashMap<String, OwnedValue>)> = wanted
            .iter()
            .map(|(index, accelerator, trigger)| {
                let mut vardict = HashMap::new();
                let description = descriptions.get(*index).unwrap_or(accelerator);
                if let Ok(value) = OwnedValue::try_from(Value::from(description.clone())) {
                    vardict.insert("description".to_owned(), value);
                }
                if let Ok(value) = OwnedValue::try_from(Value::from(trigger.clone())) {
                    // *Preferred*, not required. The compositor may bind
                    // something else, which is why `Bound` carries both.
                    vardict.insert("preferred_trigger".to_owned(), value);
                }
                (id_for(*index), vardict)
            })
            .collect();

        let mut bind_options = HashMap::new();
        let bind_token = token();
        bind_options.insert("handle_token", Value::from(bind_token.as_str()));

        // An empty parent window: this is not a dialog raised from one of our
        // windows, and the portal takes "" for "no parent".
        let bind_proxy = response_proxy(&connection, &bind_token).await?;
        let response = bind_proxy.receive_response().await?;
        let request = portal
            .bind_shortcuts(&session, &shortcuts, "", bind_options)
            .await?;
        if request.as_str() != bind_proxy.inner().path().as_str() {
            return Err(PortalError::Unexpected("unexpected bind request path"));
        }
        let mut results = await_response(response).await?;
        let granted = granted_triggers(&mut results);

        let bound = wanted
            .into_iter()
            .filter(|(index, _, _)| granted.contains_key(&id_for(*index)))
            .map(|(index, accelerator, trigger)| Bound {
                index,
                accelerator,
                requested: trigger,
                granted: granted.get(&id_for(index)).cloned().flatten(),
            })
            .collect();

        Ok((
            PortalSession {
                connection,
                session,
                bound,
            },
            refused,
        ))
    }

    /// Subscribe to a request's `Response`, then read it.
    ///
    /// Subscribing before reading is the whole reason this is a function: the
    /// specification is explicit that the reply may already be in flight when
    /// the method call returns, so doing it in two steps at each call site is
    /// how the race gets reintroduced.
    ///
    /// The body is deserialized rather than read through the generated
    /// `args()` accessor because `OwnedValue` is deliberately not `Clone` in
    /// zvariant — a value may carry a file descriptor, which cannot be
    /// duplicated infallibly — so borrowed args cannot be handed back out of
    /// this function. Deserializing gives an owned map directly.
    async fn response_proxy<'a>(
        connection: &'a Connection,
        token: &str,
    ) -> Result<RequestProxy<'a>, PortalError> {
        let sender = connection
            .unique_name()
            .ok_or(PortalError::Unexpected("no bus name"))?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        Ok(RequestProxy::builder(connection)
            .path(path)?
            .build()
            .await?)
    }

    async fn await_response(
        mut stream: ResponseStream<'_>,
    ) -> Result<HashMap<String, OwnedValue>, PortalError> {
        let Some(signal) = stream.next().await else {
            return Err(PortalError::Unexpected(
                "the request closed without answering",
            ));
        };
        let (response, results) = signal
            .message()
            .body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()?;
        match response {
            RESPONSE_SUCCESS => Ok(results),
            RESPONSE_CANCELLED => Err(PortalError::Cancelled),
            response => Err(PortalError::Refused { response }),
        }
    }

    /// The session path, whichever of the two permitted types it arrived as.
    ///
    /// The specification has carried `session_handle` as an object path `o` and,
    /// in earlier revisions, as a plain string `s`. Both are accepted because a
    /// portal that sends the older type would otherwise look like a portal that
    /// created no session at all — a failure mode that is indistinguishable, on
    /// screen, from the interface being missing.
    fn session_path(value: &OwnedValue) -> Option<OwnedObjectPath> {
        if let Ok(path) = <&ObjectPath<'_>>::try_from(value) {
            return Some(OwnedObjectPath::from(path.to_owned()));
        }
        <&str>::try_from(value)
            .ok()
            .and_then(|raw| ObjectPath::try_from(raw.to_owned()).ok())
            .map(OwnedObjectPath::from)
    }

    /// The trigger the portal says it registered, per shortcut id.
    ///
    /// The results carry `shortcuts` as `a(sa{sv})` again, with
    /// `trigger_description` in each vardict. A shortcut absent from the reply,
    /// or one with no description, is left as granted-but-undescribed rather
    /// than treated as failed: the specification permits it, and calling it a
    /// failure would tell the user a working shortcut is broken.
    fn granted_triggers(
        results: &mut HashMap<String, OwnedValue>,
    ) -> HashMap<String, Option<String>> {
        let mut found = HashMap::new();
        // Removed rather than borrowed: `Vec::try_from` needs the owned value,
        // and `OwnedValue` is not `Clone`.
        let Some(value) = results.remove("shortcuts") else {
            return found;
        };
        if let Ok(list) = Vec::<(String, HashMap<String, OwnedValue>)>::try_from(value) {
            for (id, vardict) in list {
                let description = vardict
                    .get("trigger_description")
                    .and_then(|value| <&str>::try_from(value).ok())
                    .map(str::to_owned);
                found.insert(id, description);
            }
        }
        found
    }

    /// The id this client uses for the accelerator at `index`.
    ///
    /// Positional and stable within one session, because the portal identifies
    /// a shortcut by this string in every later signal. Deliberately not the
    /// accelerator itself: the specification restricts the character set, and a
    /// chord spelling with a `+` in it is exactly what a backend is entitled to
    /// reject.
    fn id_for(index: usize) -> String {
        format!("kw-shortcut-{index}")
    }

    /// A fresh handle token.
    ///
    /// The token predicts the request path, allowing a response subscription
    /// before sending the method call. The returned path is checked against it.
    /// A fresh token prevents collisions between simultaneous requests.
    fn token() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!("kw{}", NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod portal_bus_tests {
    use super::linux_impl;
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    };
    use zbus::{
        zvariant::{OwnedObjectPath, OwnedValue},
        Connection,
    };

    struct Portal {
        response: Arc<AtomicU32>,
    }
    /// A desktop portal object with no GlobalShortcuts on it: another of the
    /// portal's interfaces, and nothing else (KNEG-490).
    struct OtherPortal;
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl OtherPortal {
        #[zbus(property)]
        fn version(&self) -> u32 {
            2
        }
    }
    fn string(value: &str) -> OwnedValue {
        OwnedValue::try_from(zbus::zvariant::Value::from(value.to_owned())).unwrap()
    }
    fn request_path(
        options: &HashMap<String, OwnedValue>,
        header: &zbus::message::Header<'_>,
    ) -> OwnedObjectPath {
        let sender = header
            .sender()
            .unwrap()
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
        format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
            .try_into()
            .unwrap()
    }
    const SESSION: &str = "/org/freedesktop/portal/desktop/session/test/session";
    const DESKTOP: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

    #[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
    impl Portal {
        #[zbus(property)]
        fn version(&self) -> u32 {
            1
        }
        async fn create_session(
            &self,
            options: HashMap<String, OwnedValue>,
            #[zbus(connection)] connection: &Connection,
            #[zbus(header)] header: zbus::message::Header<'_>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let path = request_path(&options, &header);
            let results = HashMap::from([("session_handle".to_owned(), string(SESSION))]);
            // Intentionally emit before returning: clients must subscribe first.
            connection
                .emit_signal(
                    None::<&str>,
                    path.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(0u32, results),
                )
                .await
                .unwrap();
            Ok(path)
        }
        async fn bind_shortcuts(
            &self,
            _session_handle: OwnedObjectPath,
            _shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
            _parent_window: String,
            options: HashMap<String, OwnedValue>,
            #[zbus(connection)] connection: &Connection,
            #[zbus(header)] header: zbus::message::Header<'_>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let path = request_path(&options, &header);
            let shortcuts = vec![(
                "kw-shortcut-0".to_owned(),
                HashMap::from([("trigger_description".to_owned(), string("Ctrl+Shift+D"))]),
            )];
            let results = HashMap::from([(
                "shortcuts".to_owned(),
                OwnedValue::try_from(zbus::zvariant::Value::from(shortcuts)).unwrap(),
            )]);
            connection
                .emit_signal(
                    None::<&str>,
                    path.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(self.response.load(Ordering::SeqCst), results),
                )
                .await
                .unwrap();
            Ok(path)
        }
    }

    #[tokio::test]
    #[ignore = "requires dbus-run-session; run explicitly in Linux CI"]
    async fn portal_bus_round_trip() {
        // First, a portal whose object has no GlobalShortcuts, only Settings. That is
        // `xdg-desktop-portal` with no backend for it, and the plugin is the fallback there
        // (KNEG-490). Here, before the fake below takes the name: once it is released at the end,
        // a bus that can activate a real portal hands the name to that one for good.
        let bindings = vec![
            "Control+D".to_owned(),
            "Control+E".to_owned(),
            "Control+Super".to_owned(),
        ];
        let other = zbus::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.portal.Desktop")
            .unwrap()
            .serve_at(DESKTOP, OtherPortal)
            .unwrap()
            .build()
            .await
            .unwrap();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            linux_impl::bind(&bindings, &bindings),
        )
        .await
        .unwrap()
        .err()
        .unwrap();
        assert!(
            matches!(error, linux_impl::PortalError::NoInterface(_)),
            "a portal without GlobalShortcuts is NoInterface, got {error}"
        );
        assert!(error.can_fallback(), "{error}");
        other
            .release_name("org.freedesktop.portal.Desktop")
            .await
            .unwrap();
        drop(other);

        let mode = Arc::new(AtomicU32::new(0));
        let service = zbus::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.portal.Desktop")
            .unwrap()
            .serve_at(
                DESKTOP,
                Portal {
                    response: Arc::clone(&mode),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let (session, refused) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            linux_impl::bind(&bindings, &bindings),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(refused.len(), 1);
        assert_eq!(
            session.bound().len(),
            1,
            "an omitted shortcut is not a successful grant"
        );
        assert_eq!(
            session
                .bound()
                .first()
                .expect("one granted shortcut")
                .granted
                .as_deref(),
            Some("Ctrl+Shift+D")
        );
        let edges = Arc::new(Mutex::new(Vec::new()));
        let grants = Arc::new(Mutex::new(Vec::new()));
        let output = Arc::clone(&edges);
        let changed = Arc::clone(&grants);
        let delivery = tokio::spawn(async move {
            session
                .deliver_with_changes(
                    move |index, edge| output.lock().unwrap().push((index, edge)),
                    move |bound| *changed.lock().unwrap() = bound.to_vec(),
                )
                .await
        });
        // Subscribe-ready is observed through repeated test signals, with a hard deadline.
        let handle: OwnedObjectPath = SESSION.try_into().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while edges.lock().unwrap().is_empty() {
                service
                    .emit_signal(
                        None::<&str>,
                        DESKTOP,
                        INTERFACE,
                        "Activated",
                        &(
                            handle.clone(),
                            "kw-shortcut-0",
                            1u64,
                            HashMap::<String, OwnedValue>::new(),
                        ),
                    )
                    .await
                    .unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        service
            .emit_signal(
                None::<&str>,
                DESKTOP,
                INTERFACE,
                "Deactivated",
                &(
                    handle.clone(),
                    "kw-shortcut-0",
                    2u64,
                    HashMap::<String, OwnedValue>::new(),
                ),
            )
            .await
            .unwrap();
        let shortcuts = vec![(
            "kw-shortcut-0".to_owned(),
            HashMap::from([("trigger_description".to_owned(), string("Alt+D"))]),
        )];
        service
            .emit_signal(
                None::<&str>,
                DESKTOP,
                INTERFACE,
                "ShortcutsChanged",
                &(handle.clone(), shortcuts),
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if !grants.lock().unwrap().is_empty()
                    && edges
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(_, e)| *e == crate::keyhook::Edge::Release)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            grants
                .lock()
                .unwrap()
                .first()
                .expect("a changed shortcut grant")
                .granted
                .as_deref(),
            Some("Alt+D")
        );
        service
            .emit_signal(
                None::<&str>,
                SESSION,
                "org.freedesktop.portal.Session",
                "Closed",
                &HashMap::<String, OwnedValue>::new(),
            )
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), delivery)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        mode.store(1, Ordering::SeqCst);
        let error = linux_impl::bind(&bindings, &bindings).await.err().unwrap();
        assert!(matches!(error, linux_impl::PortalError::Cancelled));
        assert!(!error.can_fallback());
        mode.store(2, Ordering::SeqCst);
        let error = linux_impl::bind(&bindings, &bindings).await.err().unwrap();
        assert!(matches!(error, linux_impl::PortalError::Refused { .. }));
        assert!(!error.can_fallback());
        mode.store(0, Ordering::SeqCst);
        let (session, _) = linux_impl::bind(&bindings, &bindings).await.unwrap();
        let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&ready);
        let delivery = tokio::spawn(async move {
            session
                .deliver(move |_, _| {
                    observed.store(true, Ordering::SeqCst);
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !ready.load(Ordering::SeqCst) {
                service
                    .emit_signal(
                        None::<&str>,
                        DESKTOP,
                        INTERFACE,
                        "Activated",
                        &(
                            handle.clone(),
                            "kw-shortcut-0",
                            1u64,
                            HashMap::<String, OwnedValue>::new(),
                        ),
                    )
                    .await
                    .unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        service
            .release_name("org.freedesktop.portal.Desktop")
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), delivery)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            linux_impl::bind(&bindings, &bindings),
        )
        .await
        .unwrap()
        .err()
        .unwrap();
        assert!(
            error.can_fallback(),
            "an absent portal allows plugin fallback, got {error}"
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_chord_becomes_a_portal_trigger() {
        assert_eq!(trigger_for("Control+Shift+D").unwrap(), "CTRL+SHIFT+d");
        assert_eq!(trigger_for("Control+D").unwrap(), "CTRL+d");
        assert_eq!(trigger_for("Alt+1").unwrap(), "ALT+1");
    }

    #[test]
    fn super_is_logo_because_the_grammar_says_so() {
        // The one mapping nobody would guess, and the one most likely to be
        // silently wrong: a compositor handed "SUPER+d" refuses the whole
        // trigger, and the user is told their shortcut did not register with
        // no indication which word was the problem.
        assert_eq!(trigger_for("Super+D").unwrap(), "LOGO+d");
        assert_eq!(trigger_for("Control+Super+D").unwrap(), "CTRL+LOGO+d");
    }

    #[test]
    fn the_modifier_order_is_the_grammars_and_not_the_users() {
        // Two spellings of one chord must produce one trigger, or a compositor
        // comparing strings sees two shortcuts wanting the same keys.
        let a = trigger_for("Shift+Control+D").unwrap();
        let b = trigger_for("Control+Shift+D").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "CTRL+SHIFT+d");
    }

    #[test]
    fn the_alternative_spellings_of_super_all_arrive_at_logo() {
        for spelling in ["Super", "Meta", "Win", "Windows", "Cmd", "Command"] {
            assert_eq!(
                trigger_for(&format!("{spelling}+D")).unwrap(),
                "LOGO+d",
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_modifier_only_chord_is_refused_with_the_reason_and_the_way_out() {
        // The shipped default. This is the capability the portal cannot
        // provide, and the message has to say so without implying the
        // application is broken.
        let err = trigger_for("Control+Super").expect_err("must refuse");
        assert!(matches!(err, PortalRefusal::ModifierOnly { .. }));
        let said = err.to_string();
        assert!(said.contains("modifier-only"), "{said}");
        assert!(said.contains("add an ordinary key"), "{said}");
    }

    #[test]
    fn a_sided_modifier_is_refused_because_the_grammar_has_no_sides() {
        let err = trigger_for("RightControl+D").expect_err("must refuse");
        match &err {
            PortalRefusal::SidedModifier { spelling, .. } => {
                assert_eq!(spelling, "RightControl");
            }
            other => panic!("{other:?}"),
        }
        let said = err.to_string();
        assert!(said.contains("Drop the side"), "{said}");
    }

    #[test]
    fn a_mouse_button_is_refused_as_a_keyboard_interface() {
        let err = trigger_for("Mouse4").expect_err("must refuse");
        assert!(matches!(err, PortalRefusal::MouseButton { .. }));
        assert!(err.to_string().contains("keyboard shortcuts only"), "{err}");
    }

    #[test]
    fn two_ordinary_keys_are_refused_rather_than_one_being_dropped() {
        let err = trigger_for("Control+D+E").expect_err("must refuse");
        assert!(matches!(err, PortalRefusal::Unnameable { .. }));
        assert!(err.to_string().contains("takes one"), "{err}");
    }

    #[test]
    fn an_unmapped_key_is_refused_here_rather_than_by_the_compositor() {
        // A trigger the portal cannot parse comes back as a failure with no
        // route to the user, so a key this build does not know is reported
        // before the call rather than after it.
        let err = trigger_for("Control+F5").expect_err("must refuse");
        assert!(matches!(err, PortalRefusal::Unnameable { .. }));
        assert!(err.to_string().contains("function keys"), "{err}");
    }

    #[test]
    fn an_empty_segment_is_refused() {
        let err = trigger_for("Control++D").expect_err("must refuse");
        assert!(err.to_string().contains("missing"), "{err}");
    }

    #[test]
    fn a_repeated_modifier_is_one_modifier() {
        assert_eq!(trigger_for("Control+Control+D").unwrap(), "CTRL+d");
    }

    #[test]
    fn every_refusal_names_the_accelerator_it_is_about() {
        // A message that does not quote what the user typed is a message they
        // cannot act on when several shortcuts are configured.
        for accelerator in ["Control+Super", "RightControl+D", "Mouse4", "Control+F5"] {
            let said = trigger_for(accelerator)
                .expect_err("must refuse")
                .to_string();
            assert!(said.contains(accelerator), "{accelerator}: {said}");
        }
    }
}
