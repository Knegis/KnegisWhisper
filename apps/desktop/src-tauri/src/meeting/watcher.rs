//! Noticing a meeting is happening, and offering to record it.
//!
//! Lifted out of `meeting.rs` on 2026-09-01 (KNEG-89). Nothing changed in the
//! move.
//!
//! **Detection suggests; it never records.** There is no path from anything
//! here to [`super::MeetingRuntime::start`] that does not go through a click,
//! and that is the rule the whole module is shaped around rather than a caveat
//! attached to it.
//!
//! The bookkeeping is the interesting part and it is what the tests are about:
//! one offer per meeting, not one per poll; a meeting that ends and starts
//! again is a new one; recording a call must not suppress the next; and a call
//! beginning inside a client that was already open is still a new meeting. All
//! of it is a pure fold over what the process table said, so none of it needs a
//! process.

use kw_platform::meeting_detect::Detected;
use kw_platform::microphone::MicrophoneSupport;

use super::Offer;

/// What the detection fold asks the runtime to do with the offer window.
///
/// `None` is deliberately a named transition rather than `Option<Offer>`: the
/// runtime must distinguish "there is nothing new" from "the call behind the
/// visible offer ended, so clear it".
#[derive(Debug)]
pub enum Transition {
    Offer(Offer),
    Withdraw(String),
    None,
}

/// Decides what to offer, and remembers what it has already offered.
///
/// Pure, and separate from the polling for that reason: "does this prompt, and
/// has it prompted already" is the whole of the behaviour worth testing, and none
/// of it needs a process table.
#[derive(Debug, Default)]
pub struct Watcher {
    /// Providers already offered during their current call.
    ///
    /// Keyed on the provider rather than on a timestamp: an offer dismissed
    /// during a two-hour call must stay dismissed for that call, and must come
    /// back for the next one. A time-based suppression would either re-prompt
    /// mid-meeting or swallow the next meeting, depending which way it was tuned.
    offered: std::collections::BTreeSet<String>,
    /// The offer currently in the one notification window.
    ///
    /// Separate from `offered`: dismissing an offer clears the window but must
    /// keep that call suppressed, while a call ending must both withdraw its
    /// window and release its suppression. Kept whole because an emitted event
    /// cannot be recovered by a window that mounts after the broadcast; status
    /// reads this same offer back rather than inventing a second source.
    outstanding: Option<Offer>,
}

impl Watcher {
    /// What the offer window should do for this poll.
    ///
    /// Returns at most one transition. Two meetings at once is possible, but
    /// the window can represent only one; another provider waits until the
    /// outstanding offer is dismissed or withdrawn rather than overwriting it.
    pub fn consider(&mut self, detected: &[Detected], prompt: bool) -> Transition {
        // A client being open is not a live meeting. Retaining on every detected
        // provider was KNEG-134: Teams stays open all day, so the first call was
        // suppressed until the process exited instead of until that call ended.
        let live: std::collections::BTreeSet<String> = detected
            .iter()
            .filter(|d| d.in_call())
            .map(|d| d.provider.to_owned())
            .collect();
        self.offered.retain(|provider| live.contains(provider));

        let withdrawn = if self
            .outstanding
            .as_ref()
            .is_some_and(|offer| !live.contains(&offer.provider))
        {
            self.outstanding.take().map(|offer| offer.provider)
        } else {
            None
        };

        // Detection keeps folding while prompts are disabled so switching the
        // setting on halfway through a call does not immediately offer that old
        // call. If a visible offer existed when the setting was switched off,
        // withdraw it now.
        if !prompt {
            self.offered.extend(live);
            return if let Some(provider) = self
                .outstanding
                .take()
                .map(|offer| offer.provider)
                .or(withdrawn)
            {
                Transition::Withdraw(provider)
            } else {
                Transition::None
            };
        }

        // Clear the ended call before offering another one. This gives the
        // renderer one unambiguous event per poll and prevents an old window
        // from being silently overwritten with a different provider.
        if let Some(provider) = withdrawn {
            return Transition::Withdraw(provider);
        }

        if self.outstanding.is_some() {
            return Transition::None;
        }

        // Only a meeting actually in progress. Prompting on "the client is open"
        // would fire on every launch for most corporate machines -- and for
        // Teams, which has no in-call process, it would fire from login until
        // shutdown. What makes Teams offerable at all is the microphone signal
        // measured on 2026-09-01; see `meeting_detect`'s module documentation.
        //
        // And the first one *not already offered*, not simply the first. Finding
        // the first in-call provider and then discarding it for being a repeat
        // meant a second meeting running alongside the first was never offered at
        // all -- silently, which is the part that matters. A test caught it.
        let Some(candidate) = detected
            .iter()
            .find(|d| d.in_call() && !self.offered.contains(d.provider))
        else {
            return Transition::None;
        };

        let offer = Offer::from(candidate);
        self.offered.insert(offer.provider.clone());
        self.outstanding = Some(offer.clone());
        Transition::Offer(offer)
    }

    /// The user closed the offer without recording this call.
    ///
    /// Only the visible identity is cleared. `offered` keeps the call suppressed
    /// until detection sees it end, so dismiss does not become a five-second
    /// snooze button.
    pub fn dismiss(&mut self) {
        self.outstanding = None;
    }

    /// A recording replaced whatever the offer window was showing.
    ///
    /// This also covers a manual start for a different provider: the window is
    /// now the recording indicator, so no offer remains outstanding behind it.
    pub fn recording_started(&mut self) {
        self.outstanding = None;
    }

    /// Forget that a provider was offered, so it can be offered again.
    ///
    /// Used when a meeting *ends*: having recorded one Zoom call should not stop
    /// the next one being offered, even if Zoom never quit in between.
    pub fn forget(&mut self, provider: &str) {
        self.offered.remove(provider);
        if self
            .outstanding
            .as_ref()
            .is_some_and(|offer| offer.provider == provider)
        {
            self.outstanding = None;
        }
    }

    /// The offer a late-mounting notification window must read back.
    ///
    /// Events only reach listeners that already exist, while this window may be
    /// created hidden and mount on either side of the broadcast. Returning the
    /// retained offer makes that race recoverable without a second detection
    /// source or a later re-emission.
    #[must_use]
    pub fn outstanding_offer(&self) -> Option<&Offer> {
        self.outstanding.as_ref()
    }

    /// Whether `provider` has an outstanding offer.
    ///
    /// Test-only. The runtime never asks — it acts on what `consider` returns —
    /// and a `pub` method nothing in the binary calls is scaffolding. Kept
    /// because asserting that a suppression was *released* reads better than
    /// inferring it from a second call.
    #[cfg(test)]
    #[must_use]
    pub fn has_offered(&self, provider: &str) -> bool {
        self.offered.contains(provider)
    }

    #[cfg(test)]
    #[must_use]
    pub fn outstanding(&self) -> Option<&str> {
        self.outstanding_offer()
            .map(|offer| offer.provider.as_str())
    }
}

/// What the last detection poll saw.
///
/// A struct rather than the tuple this was, because it grew a third member and
/// `held.2` is not a name.
#[derive(Debug, Default)]
pub(super) struct WatchState {
    /// Which providers have already been offered.
    pub(super) watcher: Watcher,
    /// Everything running at the last poll, offerable or not.
    pub(super) detected: Vec<Detected>,
    /// Whether microphone activity could be read at the last poll.
    ///
    /// `None` before the first one. Not defaulted to "unsupported": that would
    /// be a measurement, and no poll has happened yet.
    pub(super) microphone: Option<MicrophoneSupport>,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    use kw_platform::meeting_detect::CallSignal;

    fn detected(provider: &'static str, in_call: bool) -> Detected {
        // `CallProcess` for the in-call cases because these tests are about the
        // offer bookkeeping and not about which signal fired. The signal that
        // gets Teams here at all is exercised in `kw_platform::meeting_detect`,
        // against the real rules.
        Detected::new(
            provider,
            "Something",
            format!("{provider}.exe"),
            if in_call {
                CallSignal::CallProcess
            } else {
                CallSignal::None
            },
        )
    }

    fn offer(watcher: &mut Watcher, detected: &[Detected]) -> Offer {
        match watcher.consider(detected, true) {
            Transition::Offer(offer) => offer,
            other => panic!("expected an offer, got {other:?}"),
        }
    }

    fn is_none(transition: &Transition) -> bool {
        matches!(transition, Transition::None)
    }

    #[test]
    fn an_offer_carries_the_signal_that_produced_it() {
        // The offer window says why it appeared, and "Teams is holding your
        // microphone" and "Zoom started a call process" are different claims
        // with different chances of being wrong. Losing the distinction between
        // `Detected` and `Offer` would leave the window with nothing to say.
        let mut watcher = Watcher::default();
        let mic = Detected::new(
            "teams",
            "Microsoft Teams",
            "ms-teams.exe".to_owned(),
            CallSignal::Microphone,
        );
        let offer = offer(&mut watcher, &[mic]);
        assert_eq!(offer.signal, CallSignal::Microphone);
        assert_eq!(offer.evidence, "ms-teams.exe");
    }

    #[test]
    fn an_outstanding_offer_keeps_the_offer_transition_for_readback() {
        // A window can mount after the event that made it visible. Its initial
        // status read must carry the same facts as that missed transition.
        let mut watcher = Watcher::default();
        let detected = Detected::new(
            "teams",
            "Microsoft Teams",
            "ms-teams.exe".to_owned(),
            CallSignal::Microphone,
        );

        let offered = offer(&mut watcher, &[detected]);
        let outstanding = watcher.outstanding_offer().unwrap();
        assert_eq!(outstanding.provider, offered.provider);
        assert_eq!(outstanding.name, offered.name);
        assert_eq!(outstanding.evidence, offered.evidence);
        assert_eq!(outstanding.signal, offered.signal);
    }

    #[test]
    fn dismiss_clears_the_outstanding_offer() {
        let mut watcher = Watcher::default();
        let _ = offer(&mut watcher, &[detected("zoom", true)]);

        watcher.dismiss();
        assert!(watcher.outstanding_offer().is_none());
    }

    #[test]
    fn recording_started_clears_the_outstanding_offer() {
        let mut watcher = Watcher::default();
        let _ = offer(&mut watcher, &[detected("zoom", true)]);

        watcher.recording_started();
        assert!(watcher.outstanding_offer().is_none());
    }

    #[test]
    fn forget_clears_only_the_matching_outstanding_offer() {
        let mut watcher = Watcher::default();
        let _ = offer(&mut watcher, &[detected("zoom", true)]);

        watcher.forget("teams");
        assert_eq!(
            watcher
                .outstanding_offer()
                .map(|offer| offer.provider.as_str()),
            Some("zoom")
        );
        watcher.forget("zoom");
        assert!(watcher.outstanding_offer().is_none());
    }

    #[test]
    fn an_ended_call_withdraws_and_clears_the_outstanding_offer() {
        let mut watcher = Watcher::default();
        let _ = offer(&mut watcher, &[detected("zoom", true)]);

        assert!(matches!(
            watcher.consider(&[], true),
            Transition::Withdraw(provider) if provider == "zoom"
        ));
        assert!(watcher.outstanding_offer().is_none());
    }

    #[test]
    fn a_meeting_in_progress_is_offered_once_and_not_again() {
        // The property that decides whether this feature is tolerable. Polling is
        // every five seconds; a second offer would be twelve windows a minute.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        let offer = offer(&mut watcher, &running);
        assert_eq!(offer.provider, "zoom");
        assert_eq!(offer.evidence, "zoom.exe");

        for _ in 0..10 {
            assert!(is_none(&watcher.consider(&running, true)), "offered twice");
        }
    }

    #[test]
    fn a_client_merely_being_open_is_not_offered() {
        // Measured, not preferred: `meeting_detect_probe` found ms-teams.exe
        // running with no call in progress, because new Teams runs the call
        // inside its own process. Offering on "the client is open" would fire on
        // every launch for anyone whose Teams starts with Windows.
        let mut watcher = Watcher::default();
        assert!(is_none(
            &watcher.consider(&[detected("teams", false)], true)
        ));
        assert!(!watcher.has_offered("teams"));
    }

    #[test]
    fn the_next_meeting_is_offered_after_the_application_quits() {
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        let _ = offer(&mut watcher, &running);
        // Zoom quits.
        assert!(matches!(
            watcher.consider(&[], true),
            Transition::Withdraw(provider) if provider == "zoom"
        ));
        assert!(!watcher.has_offered("zoom"), "the suppression was released");
        // And a new call is offered.
        let _ = offer(&mut watcher, &running);
    }

    #[test]
    fn an_ended_call_with_its_client_still_open_withdraws_and_rearms() {
        // The exact Teams sequence behind KNEG-134: ms-teams.exe remains in the
        // process table, while microphone use is the call boundary.
        let mut watcher = Watcher::default();
        let calling = vec![detected("teams", true)];
        let open = vec![detected("teams", false)];

        let _ = offer(&mut watcher, &calling);
        assert!(matches!(
            watcher.consider(&open, true),
            Transition::Withdraw(provider) if provider == "teams"
        ));
        assert_eq!(watcher.outstanding(), None);
        assert!(!watcher.has_offered("teams"));

        let second = offer(&mut watcher, &calling);
        assert_eq!(second.provider, "teams");
    }

    #[test]
    fn dismiss_suppresses_only_the_current_call() {
        let mut watcher = Watcher::default();
        let calling = vec![detected("teams", true)];
        let open = vec![detected("teams", false)];

        let _ = offer(&mut watcher, &calling);
        watcher.dismiss();
        assert_eq!(watcher.outstanding(), None);
        assert!(is_none(&watcher.consider(&calling, true)));

        // The idle signal releases the suppression even though there was no
        // visible offer left to withdraw.
        assert!(is_none(&watcher.consider(&open, true)));
        let _ = offer(&mut watcher, &calling);
    }

    #[test]
    fn recording_one_call_does_not_suppress_the_next() {
        // `forget` is what the end of a meeting calls. Without it, a second call
        // in the same Zoom session would never be offered, because Zoom never
        // quit in between.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        let _ = offer(&mut watcher, &running);
        watcher.recording_started();
        watcher.forget("zoom");
        let _ = offer(&mut watcher, &running);
    }

    #[test]
    fn another_meeting_cannot_overwrite_an_outstanding_offer() {
        // One notification window cannot honestly hold two outstanding offers.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true), detected("webex", true)];

        let first = offer(&mut watcher, &running);
        assert_eq!(first.provider, "zoom");
        assert!(is_none(&watcher.consider(&running, true)));
        assert_eq!(watcher.outstanding(), Some("zoom"));

        // Dismissing Zoom frees the window but keeps that call suppressed. The
        // next poll may then offer Webex without re-offering Zoom.
        watcher.dismiss();
        let second = offer(&mut watcher, &running);
        assert_eq!(second.provider, "webex");
        assert!(is_none(&watcher.consider(&running, true)));
    }

    #[test]
    fn another_provider_ending_cannot_withdraw_the_visible_offer() {
        let mut watcher = Watcher::default();
        let both = vec![detected("zoom", true), detected("webex", true)];
        let zoom_only = vec![detected("zoom", true), detected("webex", false)];

        let first = offer(&mut watcher, &both);
        assert_eq!(first.provider, "zoom");
        assert!(is_none(&watcher.consider(&zoom_only, true)));
        assert_eq!(watcher.outstanding(), Some("zoom"));
    }

    #[test]
    fn calls_seen_while_prompting_is_off_stay_suppressed_until_they_end() {
        let mut watcher = Watcher::default();
        let calling = vec![detected("teams", true)];
        let open = vec![detected("teams", false)];

        assert!(is_none(&watcher.consider(&calling, false)));
        assert!(watcher.has_offered("teams"));
        // Turning prompts on during the same call must not produce a late offer.
        assert!(is_none(&watcher.consider(&calling, true)));

        assert!(is_none(&watcher.consider(&open, true)));
        let _ = offer(&mut watcher, &calling);
    }

    #[test]
    fn disabling_prompts_with_an_offer_visible_withdraws_it() {
        let mut watcher = Watcher::default();
        let calling = vec![detected("zoom", true)];

        let _ = offer(&mut watcher, &calling);
        assert!(matches!(
            watcher.consider(&calling, false),
            Transition::Withdraw(provider) if provider == "zoom"
        ));
        assert_eq!(watcher.outstanding(), None);
        // The same call remains suppressed when prompts are enabled again.
        assert!(is_none(&watcher.consider(&calling, true)));
    }

    #[test]
    fn a_call_starting_inside_an_already_open_client_is_offered() {
        // The sequence a real Zoom meeting produces: the client is open first and
        // must not be offered, then the call starts and must be.
        let mut watcher = Watcher::default();
        assert!(is_none(&watcher.consider(&[detected("zoom", false)], true)));
        let _ = offer(&mut watcher, &[detected("zoom", true)]);
    }
}
