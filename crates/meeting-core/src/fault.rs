//! What went wrong on a provider socket, and whether that ends the session.
//!
//! # Why this is a type rather than four copies of a mutex
//!
//! [`crate::openai_socket`] grew the first version of this on 2026-08-28, after
//! a real meeting: the reader and the writer are two halves of a split socket in
//! different tasks, so a provider-initiated close reached the reader while the
//! writer found out only by failing with tungstenite's own words — `Sending
//! after closing is not allowed` — which describes our call and not the reason.
//! That cell answers the question *"what words should this send failure use?"*
//! and it stays where it is, doing that job.
//!
//! This one answers a different question: **is the session over?** It has to be
//! separate, because a provider that reports an error and keeps the socket open
//! produces a *successful* send, so nothing about the send can be asked. Until
//! 2026-08-31 all four clients answered "no" unconditionally: an
//! `Incoming::Failed` frame reached a `tracing::warn!` and nothing else, so a
//! server that rejected the session-config frame and stayed open let a meeting
//! finish "successfully" holding an empty transcript.
//!
//! # The pure half is the half that gets tested
//!
//! There is no socket harness in this workspace — no mock server, no fake
//! [`crate::MeetingTranscriptionProvider`] — and every test in the four socket
//! modules returns before a socket opens. So the rule, *which faults end a
//! session*, lives in [`ends_the_session`]: pure, total, and therefore checked.
//! The same reason [`crate::openai::SessionClock`] was pulled out of the socket
//! — a rule that only runs against a live call is a rule nobody checks.
//!
//! What that buys is worth stating precisely, because it is an argument and not
//! evidence: no key exists on any machine this project has touched for
//! AssemblyAI, Deepgram or Corti, so none of the three has been put in front of
//! a real provider error. The claim is only that this is the mechanism that
//! would have surfaced the five OpenAI faults of 2026-08-28 in one attempt
//! instead of five.

use std::sync::{Arc, OnceLock};

use crate::{AudioSource, MeetingError};

/// The most of a provider's own bytes ever repeated back into a message.
const PREVIEW_LIMIT: usize = 120;

/// What kind of fault a socket saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// The provider named a fault of its own: an error frame it chose to send,
    /// or a close frame carrying a code and usually a sentence.
    Reported,
    /// The transport failed underneath this client.
    Transport,
    /// A frame arrived that this client could not parse at all.
    ///
    /// Deliberately not the same thing as [`FaultKind::Reported`]. An error
    /// frame means *the provider said something was wrong*; this means *we
    /// could not read what the provider said*. Two different facts, and they
    /// get two different answers — see [`ends_the_session`].
    Unreadable,
}

/// Whether a fault of this kind means the session is over.
///
/// `understood_anything` is whether the socket this fault came from has already
/// produced at least one frame this client could parse.
///
/// # The unreadable case, decided rather than left to fall through
///
/// All four `interpret` functions used to answer *any* frame they could not
/// parse as JSON with `Incoming::Failed`, so an HTML error body and a text
/// keepalive this parser happens not to know arrived as the same thing. Now they
/// answer `Incoming::Unreadable` instead, and this is where the difference is
/// spent: ending a meeting over an unrecognised keepalive would be a new defect,
/// and ignoring an HTML error body would be the old one.
///
/// The rule is therefore about *when* it arrives. An unreadable frame on a
/// socket that has never yet produced a frame this client understood ends the
/// session; one that arrives after it has does not. That targets the observed
/// hazard — garbage where a session-accepted frame belonged — and it cannot kill
/// a stream that has demonstrably been speaking this client's language.
/// "Understood" means *parsed*, so a `Ready`, a transcript, a provider error
/// frame and even an event this client does not act on all count. WebSocket ping
/// and pong never reach an `interpret` call at all.
///
/// One risk worth naming rather than hiding: [`FaultKind::Reported`] ends the
/// session on the **first** error frame, and a provider that emits benign
/// `error` frames routinely would make that too eager. The saved note comes from
/// the whole-file local pass and only falls back to the cloud partials, so
/// ending the cloud session early costs the live transcript and little else —
/// but if it does turn out to matter, the mitigation is one line here: give
/// `Reported` the same "before anything was understood" test `Unreadable` has.
#[must_use]
pub const fn ends_the_session(kind: FaultKind, understood_anything: bool) -> bool {
    match kind {
        // The provider named it, or the wire is gone. Either way there is no
        // transcript coming and the meeting should not report success.
        FaultKind::Reported | FaultKind::Transport => true,
        FaultKind::Unreadable => !understood_anything,
    }
}

/// One fault, with the words to report it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketFault {
    pub kind: FaultKind,
    /// What to tell the person in the room. The provider's own sentence
    /// wherever there is one, prefixed with which capture it came from.
    pub detail: String,
}

impl SocketFault {
    #[must_use]
    pub fn new(kind: FaultKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// The error a client reports this fault as.
    ///
    /// One mapping shared by four clients, so the four cannot drift into
    /// describing the same wire event three different ways.
    #[must_use]
    pub fn into_error(self, provider: &str) -> MeetingError {
        match self.kind {
            // Neither `Rejected` nor `Disconnected` is true of these: the
            // connection *was* accepted, and it may well still be open. That
            // gap is exactly why `ProviderFault` exists.
            FaultKind::Reported | FaultKind::Unreadable => MeetingError::ProviderFault {
                provider: provider.to_owned(),
                detail: self.detail,
            },
            // This one really is a connection that was lost, which is the
            // sentence `Disconnected` already renders.
            FaultKind::Transport => MeetingError::Disconnected {
                provider: provider.to_owned(),
                detail: self.detail,
            },
        }
    }
}

/// The session's one fault cell, shared by both readers and consulted by both
/// writers.
///
/// **One per session, not one per socket.** Every client here refuses to open
/// only one socket — "a session with only the microphone open would transcribe
/// half the meeting and report success" — so a dead system-audio socket has to
/// end the session even while the microphone socket is healthy. The two almost
/// always die of the same cause anyway, and the recorded sentence names which
/// capture it came from, so one true sentence beats the same sentence twice.
#[derive(Debug, Clone, Default)]
pub struct SessionFault {
    cell: Arc<OnceLock<SocketFault>>,
}

impl SessionFault {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `fault`, and say whether the session is over because of it.
    ///
    /// **First fault wins**, by construction rather than by convention:
    /// `OnceLock::set` on an already-set cell does nothing, so there is no
    /// comparison a later edit could invert, and no poisoned-mutex branch to
    /// write around this workspace's `unwrap_used = "deny"`.
    ///
    /// First rather than last, because of the one real observation there is. On
    /// 2026-08-28 an OpenAI Realtime socket produced, in this order:
    ///
    /// ```text
    /// WARN the provider reported an error       The Realtime Beta API is no longer supported.
    /// WARN the transcription socket was closed  said=... (code 4000)
    /// ```
    ///
    /// The first line is the sentence that explains the meeting. The second is a
    /// number. Last-wins would keep the number.
    ///
    /// A fault that does not end the session is not recorded at all, so "this
    /// cell is set" and "the session is over" are one statement rather than two,
    /// and there is no third state for a reader to have to reason about.
    pub fn record(&self, fault: SocketFault, understood_anything: bool) -> bool {
        if !ends_the_session(fault.kind, understood_anything) {
            return false;
        }
        let _ = self.cell.set(fault);
        true
    }

    /// What a provider's error frame said. Always ends the session.
    pub fn reported(&self, source: AudioSource, message: &str) {
        self.record(
            SocketFault::new(
                FaultKind::Reported,
                format!(
                    "on the {} stream the provider reported: {message}",
                    source.label()
                ),
            ),
            true,
        );
    }

    /// Why the provider closed the socket. Always ends the session.
    pub fn closed(&self, source: AudioSource, said: &str) {
        self.record(
            SocketFault::new(
                FaultKind::Reported,
                format!("on the {} stream, {said}", source.label()),
            ),
            true,
        );
    }

    /// The transport failed. Always ends the session.
    pub fn transport(&self, source: AudioSource, error: &str) {
        self.record(
            SocketFault::new(
                FaultKind::Transport,
                format!("on the {} stream: {error}", source.label()),
            ),
            true,
        );
    }

    /// A frame this client could not read. Returns whether the session is over
    /// because of it — see [`ends_the_session`] for why that depends on when it
    /// arrived.
    pub fn unreadable(
        &self,
        source: AudioSource,
        preview: &str,
        understood_anything: bool,
    ) -> bool {
        self.record(
            SocketFault::new(
                FaultKind::Unreadable,
                format!(
                    "on the {} stream, the first thing the provider sent could not be read: \
                     {preview}",
                    source.label()
                ),
            ),
            understood_anything,
        )
    }

    /// The fault the session ended with, if it ended with one.
    ///
    /// Named for the call site rather than for the mechanism: it does **not**
    /// clear the cell, and cannot — an `OnceLock` behind an `Arc` has no way to
    /// be unset. Nothing needs it to. A session that faulted stays faulted, so
    /// `send_audio` asking on every chunk and `finish` asking afterwards all get
    /// the same answer, which is the answer all of them want.
    #[must_use]
    pub fn take(&self) -> Option<SocketFault> {
        self.cell.get().cloned()
    }
}

/// What a WebSocket close frame said, as a sentence.
///
/// `code` is `None` when the provider closed with no frame at all.
///
/// Shared because all four clients need it and only one had it. `openai_socket`
/// gained this wording on 2026-08-28 and it is what made five separate GA-API
/// faults diagnosable — before it, every one surfaced identically as *"WebSocket
/// protocol error: Sending after closing is not allowed"*. The other three
/// clients logged `"the provider closed the socket"` at `debug` and discarded
/// the code and the reason entirely, which is the position OpenAI's client was
/// in for the five attempts nobody could tell apart.
#[must_use]
pub fn close_reason(code: Option<u16>, reason: &str) -> String {
    let reason = reason.trim();
    match (code, reason.is_empty()) {
        (None, _) => "the provider closed the connection without saying why".to_owned(),
        (Some(code), true) => format!("the provider closed the connection (code {code})"),
        (Some(code), false) => {
            format!("the provider closed the connection: {reason} (code {code})")
        }
    }
}

/// A provider's own bytes, made safe to put in a log field or a sentence on
/// screen.
///
/// Provider responses are outside this application's trust boundary — the same
/// reasoning as `deepgram_socket`'s `MAX_MESSAGE_BYTES` — and this text reaches
/// both a `tracing` field and [`MeetingError::ProviderFault`]'s message. So:
/// control characters and newlines become single spaces, runs of them collapse,
/// and the whole thing is capped.
///
/// It keeps the frame rather than discarding it because *"the server sent
/// something that is not JSON"* is worth very little next to `<html>502 Bad
/// Gateway</html>`, and the wire most likely to produce one of these is Corti's,
/// which was invented with no vendor documentation to check it against.
#[must_use]
pub fn preview(frame: &str) -> String {
    let mut out = String::new();
    let mut kept = 0_usize;
    let mut truncated = false;

    for ch in frame.chars() {
        let ch = if ch.is_control() || ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        if ch == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        if kept == PREVIEW_LIMIT {
            truncated = true;
            break;
        }
        out.push(ch);
        kept += 1;
    }

    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return "a frame with nothing readable in it".to_owned();
    }
    if truncated {
        format!("{trimmed}…")
    } else {
        trimmed.to_owned()
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
    use super::*;

    #[test]
    fn a_reported_fault_and_a_dead_transport_both_end_the_session() {
        // The whole point of the change: a provider that says something is
        // wrong must not leave a meeting looking successful.
        assert!(ends_the_session(FaultKind::Reported, false));
        assert!(ends_the_session(FaultKind::Reported, true));
        assert!(ends_the_session(FaultKind::Transport, false));
        assert!(ends_the_session(FaultKind::Transport, true));
    }

    #[test]
    fn an_unreadable_frame_ends_the_session_only_before_anything_was_understood() {
        // Garbage where a session-accepted frame belonged is a dead session.
        assert!(ends_the_session(FaultKind::Unreadable, false));
        // A text keepalive this parser does not know, on a socket that has been
        // speaking this client's language, is not. Killing a working meeting
        // over one would be a new defect of this change's own making.
        assert!(!ends_the_session(FaultKind::Unreadable, true));
    }

    #[test]
    fn the_first_fault_wins_and_a_cascade_after_it_does_not_bury_it() {
        // The recorded order from the real 2026-08-28 meeting: the sentence
        // first, then the close code. The sentence is the one worth keeping.
        let fault = SessionFault::new();
        fault.reported(
            AudioSource::System,
            "The Realtime Beta API is no longer supported.",
        );
        fault.closed(
            AudioSource::System,
            "the provider closed the connection (code 4000)",
        );
        fault.transport(AudioSource::System, "Sending after closing is not allowed");

        let held = fault.take().unwrap();
        assert_eq!(held.kind, FaultKind::Reported);
        assert!(held.detail.contains("Realtime Beta API"), "{held:?}");
        assert!(
            !held.detail.contains("4000"),
            "the close code buried the sentence: {held:?}"
        );
    }

    #[test]
    fn a_fault_that_does_not_end_the_session_is_not_recorded_at_all() {
        // Otherwise a keepalive recorded first would block the real error the
        // provider sends later -- first-wins turned against itself.
        let fault = SessionFault::new();
        assert!(
            !fault.unreadable(AudioSource::Mic, "PONG", true),
            "an unreadable frame after a readable one does not end the session"
        );
        assert!(fault.take().is_none(), "and so is not recorded");

        fault.reported(AudioSource::Mic, "invalid sample rate");
        assert!(fault.take().unwrap().detail.contains("invalid sample rate"));
    }

    #[test]
    fn an_unreadable_first_frame_is_recorded_and_ends_the_session() {
        let fault = SessionFault::new();
        assert!(fault.unreadable(AudioSource::Mic, "<html>502 Bad Gateway</html>", false));
        let held = fault.take().unwrap();
        assert_eq!(held.kind, FaultKind::Unreadable);
        assert!(held.detail.contains("502 Bad Gateway"), "{held:?}");
    }

    #[test]
    fn the_cell_is_one_cell_however_many_clones_of_it_a_session_holds() {
        // Both readers get a clone and both writers consult one; if these were
        // separate cells a dead system-audio socket would be invisible to the
        // microphone's next chunk.
        let session = SessionFault::new();
        let reader = session.clone();
        reader.reported(AudioSource::System, "session config rejected");
        assert!(
            session.take().is_some(),
            "the writer sees the reader's fault"
        );
    }

    #[test]
    fn reading_the_cell_twice_gives_the_same_answer() {
        // `take` does not clear, deliberately: `send_audio` asks on every chunk
        // and `finish` asks afterwards, and a session that faulted stays
        // faulted.
        let fault = SessionFault::new();
        fault.reported(AudioSource::Mic, "boom");
        assert_eq!(fault.take(), fault.take());
        assert!(fault.take().is_some());
    }

    #[test]
    fn a_fresh_cell_holds_nothing_so_a_healthy_session_is_never_failed() {
        assert!(SessionFault::new().take().is_none());
    }

    #[test]
    fn the_sentence_says_which_capture_it_came_from() {
        // Both sockets share one cell and the first fault wins, so the sentence
        // that survives has to say where it came from.
        let mic = SessionFault::new();
        mic.reported(AudioSource::Mic, "x");
        assert!(mic.take().unwrap().detail.contains("microphone"));

        let system = SessionFault::new();
        system.reported(AudioSource::System, "x");
        assert!(system.take().unwrap().detail.contains("system audio"));
    }

    #[test]
    fn a_reported_fault_is_neither_rejected_nor_disconnected() {
        // `Rejected` says the connection was refused and `Disconnected` says it
        // was lost. Here it was accepted and may still be open, which is the
        // gap that let this failure look like success.
        let err = SocketFault::new(FaultKind::Reported, "it said no").into_error("corti-realtime");
        match err {
            MeetingError::ProviderFault { provider, detail } => {
                assert_eq!(provider, "corti-realtime");
                assert_eq!(detail, "it said no");
            }
            other => panic!("{other:?}"),
        }
        let rendered = SocketFault::new(FaultKind::Reported, "it said no")
            .into_error("corti-realtime")
            .to_string();
        assert!(rendered.contains("corti-realtime"), "{rendered}");
        assert!(rendered.contains("it said no"), "{rendered}");
    }

    #[test]
    fn an_unreadable_fault_reports_as_the_same_variant_and_a_transport_one_does_not() {
        // Unreadable and Reported are both "accepted, then stopped": the socket
        // is open and the provider is the one behaving oddly. A transport
        // failure really is a connection that was lost.
        assert!(matches!(
            SocketFault::new(FaultKind::Unreadable, "x").into_error("p"),
            MeetingError::ProviderFault { .. }
        ));
        assert!(matches!(
            SocketFault::new(FaultKind::Transport, "x").into_error("p"),
            MeetingError::Disconnected { .. }
        ));
    }

    #[test]
    fn a_close_frame_keeps_its_code_and_its_reason() {
        assert_eq!(
            close_reason(Some(4000), "The Realtime Beta API is no longer supported."),
            "the provider closed the connection: The Realtime Beta API is no longer supported. \
             (code 4000)"
        );
        assert_eq!(
            close_reason(Some(1000), "   "),
            "the provider closed the connection (code 1000)"
        );
        assert_eq!(
            close_reason(None, ""),
            "the provider closed the connection without saying why"
        );
    }

    #[test]
    fn a_preview_keeps_the_useful_part_of_a_frame_and_flattens_the_rest() {
        assert_eq!(
            preview("<html>\n  <body>502 Bad Gateway</body>\r\n</html>"),
            "<html> <body>502 Bad Gateway</body> </html>"
        );
        assert_eq!(preview("  PONG  "), "PONG");
    }

    #[test]
    fn a_preview_is_capped_and_stays_on_a_character_boundary() {
        // Provider bytes are untrusted; a megabyte of them must not reach a log
        // line, and slicing a multi-byte character in half must not panic.
        let long = "å".repeat(1_000);
        let previewed = preview(&long);
        assert_eq!(previewed.chars().count(), PREVIEW_LIMIT + 1, "{previewed}");
        assert!(previewed.ends_with('…'));

        assert_eq!(
            preview("\u{0}\u{1}\u{2}"),
            "a frame with nothing readable in it"
        );
        assert_eq!(preview(""), "a frame with nothing readable in it");
    }
}
