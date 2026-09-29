/**
 * Meetings: the recording task.
 *
 * The page leads with whether a meeting can be recorded (`lib/readiness.ts`),
 * the one button that starts or stops it, whether the other participants are
 * captured, and who will transcribe it and whether the audio leaves this
 * computer -- with a way to Settings > Meetings, where the provider, the
 * language, detection and speaker labels are set. Below that, the live
 * transcript while there is one, and any recording an earlier session left
 * behind.
 *
 * Nothing on this page writes a setting. The provider and the language are
 * read here, never chosen: Settings > Meetings saves the provider on change and
 * the language when its field is left or Enter is pressed. Rust reads the
 * language from the database when the meeting starts (`start_live`); the
 * provider is the one this page read back from `meetings.provider` and passes
 * to `meeting_start` itself. So what this page previews is what Start will
 * use, with no save of its own to race.
 *
 * Two things it refuses to do, carried over from the screen it came out of.
 *
 * It does not decide which providers can be used -- that comes from Rust,
 * along with the reason each one cannot, and `selectProvider` only picks among
 * the usable ones.
 *
 * It does not report a meeting as recording both sources when it is recording
 * one. `Sources` comes back from `meeting_start` saying what actually opened,
 * and that is what the Status row shows.
 *
 * Meeting state arrives from Rust by event as well as by read: a meeting can
 * be started from the notification window whether or not this page is open,
 * so it reads the live state when it mounts and follows the events after
 * that. (The meeting hotkey starts nothing yet -- `shortcuts.rs` dispatches
 * it as "Meeting capture is not built yet.")
 */

import { useCallback, useEffect, useState } from "react";

import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { StatusLine } from "../components/StatusLine";
import {
  cancelMeeting,
  flushMeetingPanelNotes,
  getMeetingKeyterms,
  getMeetingLanguage,
  getMeetingProvider,
  getMeetingProviders,
  getMeetingStatus,
  getRecoverableRecordings,
  onMeetingOutcome,
  onMeetingState,
  onMeetingTranscript,
  recoverMeetingRecording,
  startMeeting,
  stopMeeting,
  type MeetingOutcome,
  type MeetingState,
  type Sources,
  type TranscriptEvent,
} from "../lib/ipc";
import {
  availabilityLabel,
  languageRequiredWarning,
  meetingIsLive,
  meetingOutcomeLabel,
  meetingStateLabel,
  sourcesLabel,
  systemAudioValue,
} from "../lib/labels";
import { selectProvider } from "../lib/meetingProvider";
import type { ViewId } from "../lib/navigation";
import { meetingReadiness } from "../lib/readiness";
import { useMeetingReview } from "../lib/useMeetingReview";
import { useRefresh } from "../lib/useRefresh";

/**
 * A label read as a sentence: capitalised, and closed with a full stop when
 * it does not already end in one. `availabilityLabel` writes for a line
 * beside a provider's name ("refused: ..."); the status line's Learn more
 * reads it on its own.
 */
function asSentence(label: string): string {
  const text = label.charAt(0).toUpperCase() + label.slice(1);
  return /[.!?]$/.test(text) ? text : `${text}.`;
}

/** The newest partial per source, which is what a partial means. */
type Live = Partial<Record<TranscriptEvent["source"], string>>;

export function Meetings({
  onGoTo,
}: {
  /** Where the status line's fix and the Change button take you. */
  onGoTo: (id: ViewId) => void;
}) {
  const load = useCallback(async () => {
    const [providers, status, provider, recoverable, language, keyterms] =
      await Promise.all([
        getMeetingProviders(),
        getMeetingStatus(),
        getMeetingProvider(),
        getRecoverableRecordings(),
        getMeetingLanguage(),
        getMeetingKeyterms(),
      ]);
    return { providers, status, provider, recoverable, language, keyterms };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  const [state, setState] = useState<MeetingState | null>(null);
  const [wantSystem, setWantSystem] = useState(true);
  const [sources, setSources] = useState<Sources | null>(null);
  const [live, setLive] = useState<Live>({});
  const [outcome, setOutcome] = useState<MeetingOutcome | null>(null);
  const [notesFlushFailed, setNotesFlushFailed] = useState(false);
  const [recoveringKey, setRecoveringKey] = useState<string | null>(null);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const [recoveryOutcome, setRecoveryOutcome] = useState<MeetingOutcome | null>(null);

  // `disposed` because `listen` resolves after the effect returns: a page left
  // before it did would otherwise keep its listeners for the rest of the
  // session -- the same guard Settings > Meetings uses for its one listener.
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (unlisten: () => void) => {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    };
    void onMeetingState((next) => {
      if (disposed) return;
      setState(next);
      if (next === "idle") { setSources(null); setNotesFlushFailed(false); }
      // A new meeting's own outcome has not happened yet, so the last one's
      // "Saved as note ..." must not sit under it. Start clears it too, but a
      // meeting started from the notification window never passes through
      // this page's Start.
      if (next === "recording") setOutcome(null);
      // The state event carries the state and nothing else, and the sentence
      // explaining it lives on `MeetingStatus`. Reloading is how it gets here
      // -- the same thing the outcome listener below does, for the same reason.
      // Both failure states, because both put a sentence on screen and a state
      // that arrived without one would render the caveat empty. And
      // `recording`, because a meeting started from the notification window
      // while this page is open brings its provider and its sources only on
      // `MeetingStatus` too -- without the read, "Transcribed by" would
      // name this page's choice rather than the one actually in use, and that
      // row is where this page says whether the audio leaves this computer.
      if (
        next === "recording" ||
        next === "recording-without-transcript" ||
        next === "stalled"
      ) {
        void reload();
      }
    }).then(keep);
    // Replaced rather than appended: every event is a partial, and the windows
    // that produce them overlap by two seconds, so appending would repeat words.
    void onMeetingTranscript((event) => {
      if (disposed) return;
      setLive((prev) => ({ ...prev, [event.source]: event.text }));
    }).then(keep);
    void onMeetingOutcome((result) => {
      if (disposed) return;
      setOutcome(result);
      setLive({});
      void reload();
    }).then(keep);
    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [reload]);

  // Computed defensively before `data` is known to have loaded, and reused
  // below once it has, so the review hook can sit above the loading guard
  // rather than after it -- a hook cannot be called only on some renders.
  const usable = (data?.providers ?? []).filter(
    (p) => p.availability.state === "available",
  );
  // No `chosen`: nothing is chosen on this page. The saved choice wins over
  // the first usable provider, exactly as on Settings > Meetings, so the two
  // screens name the same provider, and this page passes it to `meeting_start`
  // itself -- Rust does not read `meetings.provider` at start. The
  // notification's Record button reads the same key but not through this
  // fallback: it passes the saved id as it is, or `local` when nothing is
  // saved (`RECORD_WITH`), which is why a meeting it started is named from
  // `status.provider` below. `lib/meetingProvider.ts` says why the fallback
  // never writes (KNEG-317).
  const { selected, savedUnusable } = selectProvider(
    null,
    data?.provider ?? null,
    usable.map((p) => p.id),
  );
  // The saved language, not a draft: this page has no field, and the review
  // has to describe the language `start_live` will read back from the
  // database.
  const review = useMeetingReview(
    selected,
    data?.language ?? "",
    (data?.keyterms ?? []).join("\n"),
  );

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { providers, status, recoverable } = data;
  const current = state ?? status.state;
  // A meeting whose provider died is still recording, so every control that
  // asks "is a session running" must keep saying yes: Stop and Discard have to
  // stay on screen, and Start has to stay off it. This was the whole shape of
  // the bug `stop` had -- a screen with no working button on it.
  const transcriptEnded = current === "recording-without-transcript";
  const stalled = current === "stalled";
  // Asked of `meetingIsLive` rather than compared here. A state added to the
  // union does not break a `===`, so when `stalled` arrived this screen kept
  // compiling and would have offered "Start recording" for a session that was
  // still held with a WAV still on disk.
  const recording = meetingIsLive(current);
  const languageWarning = review ? languageRequiredWarning(review) : null;
  const selectedProvider = providers.find((p) => p.id === selected) ?? null;
  const savedProvider = savedUnusable
    ? (providers.find((p) => p.id === savedUnusable) ?? null)
    : null;
  const readiness = meetingReadiness({
    usable,
    selected,
    // Named and with its reason, so the status line can say which choice
    // can't be used and why, rather than the page quietly using another.
    savedUnusable: savedUnusable
      ? {
          name: savedProvider?.name ?? savedUnusable,
          reason: savedProvider
            ? asSentence(availabilityLabel(savedProvider.availability))
            : "This version of the app doesn't offer it.",
        }
      : null,
    languageWarning,
    systemAudio: status.systemAudio,
  });
  // Where readiness holds Start back, the status line's fix is the one thing
  // to do next and takes the page's single filled button.
  const startBlocked = readiness.state === "needs-setup";
  // While a meeting is running, the provider it is running with -- which the
  // notification's Record button may have picked rather than this page.
  const liveProvider =
    recording && status.provider !== null
      ? (providers.find((p) => p.id === status.provider) ?? null)
      : null;
  const transcriber = liveProvider ?? selectedProvider;
  const systemAudioAvailable = status.systemAudio.state === "available";

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="meetings.record" aria-label="Meeting">
        {/* Only while no meeting is under way. Readiness answers "will
            starting work?", which means nothing once one has started or is
            being transcribed -- "Ready" above "The recording stopped
            unexpectedly" was two answers on one screen. The Status row and
            the caveats below speak for a running meeting instead. */}
        {recording || current === "finishing" ? null : (
          <StatusLine readiness={readiness} onFix={onGoTo} fixIsPrimary={startBlocked} />
        )}
        {stalled ? (
          /*
           * A different sentence from the one below, because a different thing
           * has happened: there the recording continues and the transcript
           * does not, here neither does. The one instruction that matters is
           * the same in both — press Stop — because that is what turns what
           * was captured into a note.
           */
          <p className="caveat warn">
            <strong>The recording stopped unexpectedly.</strong>{" "}
            {status.transcriptFailure ??
              "The recording thread ended without saying why."}{" "}
            Nothing further is being captured. Stop will still transcribe what
            was recorded before it stopped, and Discard will throw it away.
          </p>
        ) : null}
        {transcriptEnded ? (
          /*
           * The provider's own sentence, which is why `MeetingError::
           * ProviderFault` carries which of the two sockets died: "the
           * connection failed" would send someone looking at their network.
           *
           * Marked rather than red-texted, the same split `.caveat.warn` was
           * introduced for: nothing here has failed in a way that needs
           * repairing right now. The recording is running and the note will
           * still be produced -- from the whole-file pass, which reads the
           * complete audio and does not depend on the provider that died.
           */
          <p className="caveat warn">
            <strong>The live transcript stopped.</strong>{" "}
            {status.transcriptFailure ??
              "The transcription task ended without saying why."}{" "}
            Recording continues, and Stop will still transcribe the whole
            recording — the saved note comes from that pass, not from the
            lines below.
          </p>
        ) : null}

        {recording ? (
          <>
            {/* Never held back by readiness: a meeting that is running has to
                be stoppable whatever the settings now say. */}
            <button
              type="button"
              className="primary"
              onClick={() => {
                void guard(async () => {
                  try { await flushMeetingPanelNotes(); }
                  catch (cause) { setNotesFlushFailed(true); throw cause; }
                  setNotesFlushFailed(false);
                  setOutcome(await stopMeeting());
                });
              }}
            >
              Stop and transcribe
            </button>
            <button
              type="button"
              onClick={() => {
                void guard(async () => {
                  try { await flushMeetingPanelNotes(); }
                  catch (cause) { setNotesFlushFailed(true); throw cause; }
                  setNotesFlushFailed(false);
                  await cancelMeeting();
                });
              }}
            >
              Discard
            </button>
            {notesFlushFailed && <button type="button" onClick={() => {
              void guard(async () => { setOutcome(await stopMeeting()); setNotesFlushFailed(false); });
            }}>Stop recording anyway</button>}
          </>
        ) : (
          /* Off whenever the status line says Needs setup, not only when no
             provider is selected: with a language the provider requires left
             blank, `start_live` refuses (`resolve_provider_config`,
             src-tauri/src/meeting.rs), so the button would lead straight
             into an error, and with the saved provider unusable it would
             record with one nobody chose. The status line above says what is
             wrong, and its fix -- not this -- is the filled button then. */
          <button
            type="button"
            className={startBlocked ? undefined : "primary"}
            disabled={startBlocked || selected === null || current === "finishing"}
            onClick={() => {
              if (selected === null) return;
              void guard(async () => {
                setOutcome(null);
                setSources(await startMeeting(selected, wantSystem));
              });
            }}
          >
            Start recording
          </button>
        )}

        {/* Unchecked, not just disabled, where system audio cannot be
            captured: a ticked box reading "Record what the other participants
            say" under a caution saying they cannot be recorded would be two
            answers on one screen. The request itself is kept, so the box comes
            back ticked where the capture does. Why the box is off is said
            beside it, in every state -- the status line's caution is only
            there while the page is Ready, and a switched-off control with no
            reason on screen is the button that silently does nothing. Only
            the mechanism, and the device it would capture, are folded. */}
        <div className="row">
          <span className="row-label">Other participants</span>
          <span className="row-value controls">
            <label>
              <input
                type="checkbox"
                checked={wantSystem && systemAudioAvailable}
                disabled={recording || !systemAudioAvailable}
                onChange={(e) => setWantSystem(e.target.checked)}
              />
              Record what the other participants say
            </label>
            {status.systemAudio.state === "available" ? null : (
              <em className="row-detail">
                {`Can't be recorded on this computer: ${status.systemAudio.reason}`}
              </em>
            )}
            {status.systemAudio.state === "unsupported" ? null : (
              <LearnMore>
                <p>
                  System audio: {systemAudioValue(status.systemAudio)} —{" "}
                  {status.systemAudio.state === "available"
                    ? `${status.systemAudio.mechanism} — ${status.systemAudio.device}`
                    : status.systemAudio.mechanism}
                </p>
              </LearnMore>
            )}
          </span>
        </div>
        <Row
          label="Status"
          value={meetingStateLabel(current)}
          detail={
            recording
              ? sourcesLabel(sources ?? status.sources)
              : current === "finishing"
                ? "this runs over the whole recording and can take a while on a long meeting"
                : undefined
          }
        />
        {/* The privacy fact, so it is on the row rather than behind anything:
            whether this meeting's audio leaves this computer, decided by
            `audioLeavesDevice` -- the field Rust sets for exactly that
            question. The provider is chosen on Settings > Meetings, so the
            row links there rather than offering a select of its own. */}
        <div className="row">
          <span className="row-label">Transcribed by</span>
          <span className="row-value controls">
            <span>{transcriber ? transcriber.name : "Not set up"}</span>
            {transcriber ? (
              <em className="row-detail">
                {transcriber.audioLeavesDevice
                  ? `Audio is sent to ${transcriber.name}`
                  : "Audio stays on this computer"}
              </em>
            ) : null}
            <button type="button" onClick={() => onGoTo("meeting-settings")}>
              Change
            </button>
          </span>
        </div>

        {recording && notesFlushFailed && <p className="caveat">The meeting panel could not confirm its latest note save. Stopping anyway keeps the recording and any notes already received by the app; the most recent edit may be missing.</p>}

        {outcome ? <p className="caveat">{meetingOutcomeLabel(outcome)}</p> : null}
      </section>

      {recording || Object.keys(live).length > 0 ? (
        <section>
          <h2>Live transcript</h2>
          <p className="caveat">
            {transcriptEnded
              ? "These are the last lines the provider sent before the live transcription stopped. Nothing further will appear here, and the transcript that gets saved comes from one pass over the whole recording when the meeting ends."
              : "Each line is the most recent few seconds, replaced as it is revised. The transcript that gets saved comes from one pass over the whole recording when the meeting ends, so it will read better than this."}
          </p>
          <Row label="You" value={live.mic ?? "—"} />
          <Row label="Everyone else" value={live.system ?? "—"} />
        </section>
      ) : null}

      {(recoverable.length > 0 || recoveryError || recoveryOutcome) && (
        <section>
          <h2>Interrupted recordings</h2>
          {/* The one sentence that changes what you expect to get back stays
              visible; what these are and how Recover works is folded. */}
          <p className="caveat">The audio may end abruptly.</p>
          <LearnMore>
            <p>
              These recordings are not attached to a saved note. Recover uses
              the local whole-recording transcriber and saves any notes typed
              during the meeting.
            </p>
          </LearnMore>
          {recoverable.map((candidate) => (
            <div className="row" key={candidate.storageKey}>
              <span className="row-label">{candidate.storageKey}</span>
              <span className="row-value controls">
                {candidate.hasNotes && <span className="muted">Typed notes found</span>}
                <button
                  type="button"
                  disabled={recording || current === "finishing" || recoveringKey !== null}
                  onClick={() => {
                    setRecoveringKey(candidate.storageKey);
                    setRecoveryError(null);
                    setRecoveryOutcome(null);
                    void recoverMeetingRecording(candidate.storageKey)
                      .then(async (result) => {
                        setRecoveryOutcome(result);
                        await reload();
                      })
                      .catch((cause: unknown) => setRecoveryError(String(cause)))
                      .finally(() => setRecoveringKey(null));
                  }}
                >
                  {recoveringKey === candidate.storageKey ? "Recovering…" : "Recover recording"}
                </button>
              </span>
            </div>
          ))}
          {recoveryOutcome && <p className="caveat">{meetingOutcomeLabel(recoveryOutcome)}</p>}
          {recoveryError && <p className="error" role="alert">{recoveryError}</p>}
        </section>
      )}
    </div>
  );
}
