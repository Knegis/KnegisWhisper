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

/// Decides what to offer, and remembers what it has already offered.
///
/// Pure, and separate from the polling for that reason: "does this prompt, and
/// has it prompted already" is the whole of the behaviour worth testing, and none
/// of it needs a process table.
#[derive(Debug, Default)]
pub struct Watcher {
    /// Providers already offered and not yet gone away.
    ///
    /// Keyed on the provider rather than on a timestamp: an offer dismissed
    /// during a two-hour call must stay dismissed for that call, and must come
    /// back for the next one. A time-based suppression would either re-prompt
    /// mid-meeting or swallow the next meeting, depending which way it was tuned.
    offered: std::collections::BTreeSet<String>,
}

impl Watcher {
    /// What to offer for this poll, if anything.
    ///
    /// Returns at most one offer. Two meetings at once is possible and offering
    /// both would stack windows; the first is the one to ask about.
    pub fn consider(&mut self, detected: &[Detected]) -> Option<Offer> {
        // Anything that has stopped running can be offered again next time.
        let live: std::collections::BTreeSet<String> =
            detected.iter().map(|d| d.provider.to_owned()).collect();
        self.offered.retain(|provider| live.contains(provider));

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
        let candidate = detected
            .iter()
            .find(|d| d.in_call() && !self.offered.contains(d.provider))?;

        self.offered.insert(candidate.provider.to_owned());
        Some(Offer::from(candidate))
    }

    /// Forget that a provider was offered, so it can be offered again.
    ///
    /// Used when a meeting *ends*: having recorded one Zoom call should not stop
    /// the next one being offered, even if Zoom never quit in between.
    pub fn forget(&mut self, provider: &str) {
        self.offered.remove(provider);
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
        let offer = watcher.consider(&[mic]).expect("a call is offered");
        assert_eq!(offer.signal, CallSignal::Microphone);
        assert_eq!(offer.evidence, "ms-teams.exe");
    }

    #[test]
    fn a_meeting_in_progress_is_offered_once_and_not_again() {
        // The property that decides whether this feature is tolerable. Polling is
        // every five seconds; a second offer would be twelve windows a minute.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        let offer = watcher.consider(&running).expect("the first poll offers");
        assert_eq!(offer.provider, "zoom");
        assert_eq!(offer.evidence, "zoom.exe");

        for _ in 0..10 {
            assert!(watcher.consider(&running).is_none(), "offered twice");
        }
    }

    #[test]
    fn a_client_merely_being_open_is_not_offered() {
        // Measured, not preferred: `meeting_detect_probe` found ms-teams.exe
        // running with no call in progress, because new Teams runs the call
        // inside its own process. Offering on "the client is open" would fire on
        // every launch for anyone whose Teams starts with Windows.
        let mut watcher = Watcher::default();
        assert!(watcher.consider(&[detected("teams", false)]).is_none());
        assert!(!watcher.has_offered("teams"));
    }

    #[test]
    fn the_next_meeting_is_offered_after_the_application_quits() {
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        assert!(watcher.consider(&running).is_some());
        // Zoom quits.
        assert!(watcher.consider(&[]).is_none());
        assert!(!watcher.has_offered("zoom"), "the suppression was released");
        // And a new call is offered.
        assert!(watcher.consider(&running).is_some());
    }

    #[test]
    fn recording_one_call_does_not_suppress_the_next() {
        // `forget` is what the end of a meeting calls. Without it, a second call
        // in the same Zoom session would never be offered, because Zoom never
        // quit in between.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true)];

        assert!(watcher.consider(&running).is_some());
        watcher.forget("zoom");
        assert!(watcher.consider(&running).is_some());
    }

    #[test]
    fn only_one_offer_is_made_when_two_meetings_are_running() {
        // Two windows stacked on top of each other is worse than one question.
        let mut watcher = Watcher::default();
        let running = vec![detected("zoom", true), detected("webex", true)];

        assert!(watcher.consider(&running).is_some());
        // The second is offered on the next poll, not simultaneously.
        let second = watcher.consider(&running).expect("the other one follows");
        assert_eq!(second.provider, "webex");
        assert!(watcher.consider(&running).is_none());
    }

    #[test]
    fn a_call_starting_inside_an_already_open_client_is_offered() {
        // The sequence a real Zoom meeting produces: the client is open first and
        // must not be offered, then the call starts and must be.
        let mut watcher = Watcher::default();
        assert!(watcher.consider(&[detected("zoom", false)]).is_none());
        assert!(watcher.consider(&[detected("zoom", true)]).is_some());
    }
}
