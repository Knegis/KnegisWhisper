/**
 * Settings > Meetings.
 *
 * Three things this screen refuses to do.
 *
 * It does not decide which providers can be used — that comes from Rust, along
 * with the reason each one cannot. A screen that worked it out for itself would
 * be a second source of truth about which clients exist, which is exactly the
 * defect that had all six providers reporting as permitted with no client behind
 * any of them.
 *
 * It does not say "streaming" about the local provider. It is repeated short
 * passes over a local model, about three seconds behind the room, and the name it
 * is given here is the name Rust gives it.
 *
 * It does not report a meeting as recording both sources when it is recording
 * one. `Sources` comes back from `meeting_start` saying what actually opened, and
 * that is what is shown.
 */

import { useCallback, useEffect, useState } from "react";

import { Row } from "../components/Row";
import {
  cancelMeeting,
  getMeetingDetectLists,
  getMeetingKeyterms,
  getMeetingLanguage,
  getMeetingProviders,
  getMeetingStatus,
  reviewMeetingConfig,
  resetMeetingNotificationPlacement,
  setMeetingDetectLists,
  setMeetingLanguage,
  setMeetingPrompt,
  getDiarizationStatus,
  installDiarizationModel,
  setDiarizationEnabled,
  onMeetingOutcome,
  onMeetingState,
  onMeetingTranscript,
  startMeeting,
  stopMeeting,
  type ConfigReview,
  type MeetingOutcome,
  type MeetingState,
  type Sources,
  type TranscriptEvent,
} from "../lib/ipc";
import {
  availabilityLabel,
  captureExclusionLabel,
  diarizationBlocker,
  diarizationRoleDetail,
  meetingIsLive,
  meetingStateLabel,
  detectedLabel,
  keytermLabel,
  keytermReviewLabel,
  languageRequiredWarning,
  meetingOutcomeLabel,
  sourcesLabel,
  microphoneDetail,
  microphoneValue,
  systemAudioValue,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

/**
 * One entry per line, trimmed, with blank lines dropped.
 *
 * Shared by the keyterms field and by both detection lists, because all three
 * are the same thing on screen and are normalised the same way in Rust: keyterms
 * by `normalize_keyterms`, the lists by `kw_platform::meeting_detect::detect`,
 * which trims every entry and skips the empty ones. One parser rather than
 * three is what keeps a stray blank line from meaning something different in
 * one of them.
 */
function onePerLine(text: string): string[] {
  return text
    .split("\n")
    .map((k) => k.trim())
    .filter((k) => k.length > 0);
}

/** The newest partial per source, which is what a partial means. */
type Live = Partial<Record<TranscriptEvent["source"], string>>;

export function SettingsMeetings({ onGoToVocabulary }: { onGoToVocabulary: () => void }) {
  const load = useCallback(async () => {
    const [providers, status, language, keyterms, detect, diarization] =
      await Promise.all([
        getMeetingProviders(),
        getMeetingStatus(),
        getMeetingLanguage(),
        getMeetingKeyterms(),
        getMeetingDetectLists(),
        getDiarizationStatus(),
      ]);
    return { providers, status, language, keyterms, detect, diarization };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);

  const [state, setState] = useState<MeetingState | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [wantSystem, setWantSystem] = useState(true);
  const [sources, setSources] = useState<Sources | null>(null);
  const [live, setLive] = useState<Live>({});
  const [outcome, setOutcome] = useState<MeetingOutcome | null>(null);
  // `null` means "not yet touched" -- the loaded, saved value is shown until
  // the user types something, the same idea `chosen` already uses for the
  // provider select.
  const [languageDraft, setLanguageDraft] = useState<string | null>(null);
  const [allowDraft, setAllowDraft] = useState<string | null>(null);
  const [blockDraft, setBlockDraft] = useState<string | null>(null);
  const [review, setReview] = useState<ConfigReview | null>(null);
  /** Which diarization model is downloading, or null. */
  const [installing, setInstalling] = useState<string | null>(null);
  const [resettingNotification, setResettingNotification] = useState(false);
  const [notificationResetError, setNotificationResetError] = useState<string | null>(null);

  const resetNotification = async () => {
    setResettingNotification(true);
    setNotificationResetError(null);
    try {
      await resetMeetingNotificationPlacement();
    } catch (error) {
      setNotificationResetError(String(error));
    } finally {
      setResettingNotification(false);
    }
  };

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void onMeetingState((next) => {
      setState(next);
      if (next === "idle") setSources(null);
      // The state event carries the state and nothing else, and the sentence
      // explaining it lives on `MeetingStatus`. Reloading is how it gets here
      // -- the same thing the outcome listener below does, for the same reason.
      // Both failure states, because both put a sentence on screen and a state
      // that arrived without one would render the caveat empty.
      if (next === "recording-without-transcript" || next === "stalled") {
        void reload();
      }
    }).then((fn) => unlisteners.push(fn));
    // Replaced rather than appended: every event is a partial, and the windows
    // that produce them overlap by two seconds, so appending would repeat words.
    void onMeetingTranscript((event) => {
      setLive((prev) => ({ ...prev, [event.source]: event.text }));
    }).then((fn) => unlisteners.push(fn));
    void onMeetingOutcome((result) => {
      setOutcome(result);
      setLive({});
      void reload();
    }).then((fn) => unlisteners.push(fn));
    return () => unlisteners.forEach((fn) => fn());
  }, [reload]);

  // Computed defensively before `data` is known to have loaded, and reused
  // below once it has, so this effect can sit above the loading guard rather
  // than after it -- a hook cannot be called only on some renders.
  const usable = (data?.providers ?? []).filter(
    (p) => p.availability.state === "available",
  );
  const selected = chosen ?? usable[0]?.id ?? null;
  const effectiveLanguage = languageDraft ?? data?.language ?? "";
  const effectiveKeytermsText = (data?.keyterms ?? []).join("\n");

  useEffect(() => {
    if (!selected) {
      setReview(null);
      return;
    }
    let cancelled = false;
    void reviewMeetingConfig(
      selected,
      effectiveLanguage,
      onePerLine(effectiveKeytermsText),
    )
      .then((result) => {
        if (!cancelled) setReview(result);
      })
      .catch(() => {
        if (!cancelled) setReview(null);
      });
    return () => {
      cancelled = true;
    };
  }, [selected, effectiveLanguage, effectiveKeytermsText]);

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { providers, status, detect, diarization } = data;
  const effectiveAllowText = allowDraft ?? detect.allow.join("\n");
  const effectiveBlockText = blockDraft ?? detect.block.join("\n");
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
  const selectedProvider = providers.find((p) => p.id === selected) ?? null;
  const keytermReview =
    review && selectedProvider
      ? keytermReviewLabel(review, selectedProvider.keyterms)
      : null;
  const languageWarning = review ? languageRequiredWarning(review) : null;

  // Persist exactly the values the review above just described, so a saved
  // setting can never lag behind what the screen showed. Awaited before
  // `startMeeting`, and not left to an on-blur save that a click straight
  // from the keyterms field might outrun — `MeetingRuntime::start` reads
  // these from the database, and a preview describing a configuration it
  // will not use is the defect this task exists to close.
  const persistLanguageAndKeyterms = async () => {
    await setMeetingLanguage(effectiveLanguage.trim());
  };

  // Both lists go in one write because they are one setting: `meetings.detect`
  // holds the pair, so saving either alone would have to re-send the other.
  //
  // Reloaded afterwards, but the list of running applications above will not
  // change for up to five seconds: `status.detected` is whatever the detection
  // timer last computed, not a fresh walk, and the timer is what reads the
  // lists back. Saying so here rather than reaching for a fresh poll — a walk
  // costs a pass over every process on the machine, and the screen catching up
  // one tick later is not worth spending that on every keystroke-and-blur.
  const persistDetectLists = async () => {
    await setMeetingDetectLists({
      allow: onePerLine(effectiveAllowText),
      block: onePerLine(effectiveBlockText),
    });
    await reload();
  };

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.meetings">
        <h2>Meetings</h2>

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
        <Row
          label="System audio"
          value={systemAudioValue(status.systemAudio)}
          detail={
            status.systemAudio.state === "available"
              ? `${status.systemAudio.mechanism} — ${status.systemAudio.device}`
              : status.systemAudio.reason
          }
        />
        {/*
          * Reported next to system audio because it is the same kind of fact
          * and the same kind of trap: where this is anything but "Watching",
          * every "not in a call" further down is an absence of evidence rather
          * than evidence of absence, and the user is the only one who can tell
          * that from the screen.
          */}
        <Row
          label="Microphone activity"
          value={microphoneValue(status.microphone)}
          detail={microphoneDetail(status.microphone)}
        />

        {usable.length === 0 ? (
          <p className="caveat">
            No meeting provider can be used yet. The local one needs a model
            server running — start one under Settings &gt; Transcription &gt;
            Local.
          </p>
        ) : null}

        <div className="row">
          <span className="row-label">Provider</span>
          <span className="row-value controls">
            <select
              value={selected ?? ""}
              disabled={recording}
              onChange={(e) => setChosen(e.target.value)}
            >
              {usable.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            <label>
              <input
                type="checkbox"
                checked={wantSystem}
                disabled={recording || status.systemAudio.state !== "available"}
                onChange={(e) => setWantSystem(e.target.checked)}
              />
              Record what the other participants say
            </label>
          </span>
        </div>

        {selectedProvider ? (
          <>
            <p className="caveat">
              {selectedProvider.summary}.{" "}
              {selectedProvider.audioLeavesDevice
                ? "Audio is sent to this provider."
                : "Audio stays on this machine."}
            </p>

            <div className="row">
              <span className="row-label">Language</span>
              <span className="row-value controls">
                <input
                  type="text"
                  aria-label="Meeting language"
                  placeholder="leave blank to detect automatically, where the provider supports it"
                  value={effectiveLanguage}
                  disabled={recording}
                  onChange={(e) => setLanguageDraft(e.target.value)}
                  onBlur={() => {
                    void guard(persistLanguageAndKeyterms);
                  }}
                />
              </span>
            </div>
            <div className="row">
              <span className="row-label">Vocabulary</span>
              <span className="row-value controls">
                <button type="button" onClick={onGoToVocabulary}>Manage vocabulary</button>
                <span className="muted">{data?.keyterms.length ?? 0} saved terms · changes apply to the next meeting</span>
              </span>
            </div>
            {/*
             * `validate` computed this, not a re-implementation here — see
             * `kw_meeting_core::validate`. Shown for the fields above as
             * they are edited. Saved when a field is left, and saved again
             * right before "Start recording" runs, so what is displayed can
             * never lag behind what gets persisted.
             */}
            {keytermReview ? (
              <Row
                label="Vocabulary for next meeting"
                value={keytermReview.value}
                detail={keytermReview.detail}
              />
            ) : null}
            {selected === "deepgram-realtime" && (data?.keyterms.length ?? 0) > 0 && <p className="muted">Vocabulary-enabled sessions use Deepgram Nova-3.</p>}
            {languageWarning ? <p className="caveat">{languageWarning}</p> : null}
          </>
        ) : null}

        <div className="row">
          <span className="row-value controls">
            {recording ? (
              <>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      setOutcome(await stopMeeting());
                    });
                  }}
                >
                  Stop and transcribe
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void guard(cancelMeeting);
                  }}
                >
                  Discard
                </button>
              </>
            ) : (
              <button
                type="button"
                disabled={selected === null || current === "finishing"}
                onClick={() => {
                  if (selected === null) return;
                  void guard(async () => {
                    // Saved here rather than trusted to an on-blur that a
                    // click straight out of the keyterms field would outrun:
                    // this is what the review just above was describing, so
                    // it is what `start` must read.
                    await persistLanguageAndKeyterms();
                    setOutcome(null);
                    setSources(await startMeeting(selected, wantSystem));
                  });
                }}
              >
                Start recording
              </button>
            )}
          </span>
        </div>

        {outcome ? <p className="caveat">{meetingOutcomeLabel(outcome)}</p> : null}
      </section>

      <section>
        <h2>Notification window</h2>
        <p className="caveat">Drag the notification window to move it or resize it by an edge. Reset returns it to the bottom-right at its default size.</p>
        <button type="button" disabled={resettingNotification} onClick={() => { void resetNotification(); }}>Reset notification window</button>
        {notificationResetError ? <p className="error" role="alert">{notificationResetError}</p> : null}
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

      <section>
        <h2>Detection</h2>
        <p className="caveat">
          Meeting applications that are running. Nothing is ever recorded without
          you pressing a button — when a meeting is detectably in progress, a
          small window offers to record it.
        </p>
        <div className="row">
          <span className="row-label">Offer to record</span>
          <span className="row-value controls">
            <label>
              <input
                type="checkbox"
                checked={status.promptOnDetect}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void guard(async () => {
                    await setMeetingPrompt(enabled);
                    await reload();
                  });
                }}
              />
              Show a window when a meeting starts
            </label>
          </span>
        </div>
        {status.detected.length === 0 ? (
          <p className="muted">No meeting application is running.</p>
        ) : (
          status.detected.map((d) => (
            <Row
              key={`${d.provider}-${d.evidence}`}
              label={d.name}
              value={d.inCall ? "In a call" : "Open"}
              detail={detectedLabel(d)}
            />
          ))
        )}
        {/*
          * The lists are edited here, under the applications they act on,
          * because the name to type is the one each detection above reports.
          * Until now `meetings.detect` was honoured by the backend and settable
          * only by editing SQLite by hand — the fourth instance of that
          * pattern, and the first found by a gate rather than by a person
          * reading a screen. See KNEG-114.
          */}
        <div className="row">
          <span className="row-label">Also count as a meeting</span>
          <span className="row-value controls">
            <textarea
              aria-label="Extra process names that count as a meeting, one per line"
              placeholder="one program name per line, e.g. bluejeans.exe"
              rows={3}
              value={effectiveAllowText}
              onChange={(e) => setAllowDraft(e.target.value)}
              onBlur={() => {
                void guard(persistDetectLists);
              }}
            />
          </span>
        </div>
        <div className="row">
          <span className="row-label">Never count as a meeting</span>
          <span className="row-value controls">
            <textarea
              aria-label="Program names that never count as a meeting, one per line"
              placeholder="one program name per line"
              rows={3}
              value={effectiveBlockText}
              onChange={(e) => setBlockDraft(e.target.value)}
              onBlur={() => {
                void guard(persistDetectLists);
              }}
            />
          </span>
        </div>
        <p className="caveat">
          Names are matched against the running program's own filename, not its
          window title, and capitalisation does not matter — the name each
          detection above is reported by is the one to copy. A name in the second
          list is never offered even when this application recognises it on its
          own, which is how a false positive is silenced without waiting for a
          release. A program you list yourself is reported as running, never as
          being in a call: only you know whether it is.
        </p>
        {status.notificationExcluded ? (
          <Row
            label="The recording window"
            value={captureExclusionLabel(status.notificationExcluded).text}
            detail={captureExclusionLabel(status.notificationExcluded).detail}
          />
        ) : null}
      </section>

      <section>
        <h2>Speaker labels</h2>
        <p className="caveat">
          A second pass over the finished recording, grouping the voices in it so
          each passage of the saved note says who was speaking. It answers how
          many voices and when, never <em>who</em> — a label is{" "}
          <code>Speaker 1</code>, it means nothing outside this one recording, and
          the same person is <code>Speaker 1</code> in one meeting and{" "}
          <code>Speaker 3</code> in the next.
        </p>
        <p className="caveat warn">
          Measured on this machine: the pass costs about 8% of the meeting’s
          length, so an hour-long recording adds roughly five minutes to Stop and
          transcribe — on top of the transcription pass that already runs there.
          That is why it is off unless you turn it on.
        </p>
        <p className="caveat warn">
          The speaker model shipped here is English. Measured: the same recording
          put through an English and a language-matched model gave three speakers
          and four. Meetings in another language still get labels, and they will
          be worse — nothing in the note will say which.
        </p>
        <Row
          label="Status"
          value={diarization.ready ? "ready" : "not ready"}
          detail={diarizationBlocker(diarization) ?? undefined}
        />
        <div className="row">
          <span className="row-label">Label speakers</span>
          <span className="row-value controls">
            <label>
              <input
                type="checkbox"
                checked={diarization.enabled}
                disabled={!diarization.ready && !diarization.enabled}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void guard(async () => {
                    await setDiarizationEnabled(enabled);
                    await reload();
                  });
                }}
              />
              Add speaker labels to saved meeting notes
            </label>
          </span>
        </div>
        <ul className="list">
          {diarization.models.map((m) => (
            <li key={m.id}>
              <code>{m.id}</code> {m.name}
              {m.sizeMb ? ` · ${m.sizeMb} MB` : ""}
              <em className="row-detail">
                {diarizationRoleDetail(m.role)} ·{" "}
                {m.installed ? "installed" : "not installed"}
              </em>
              {m.installed ? null : (
                <button
                  type="button"
                  disabled={installing !== null}
                  onClick={() => {
                    setInstalling(m.id);
                    void guard(async () => {
                      try {
                        await installDiarizationModel(m.id);
                        await reload();
                      } finally {
                        // Cleared in a `finally` rather than on a completion
                        // event: a refused install emits none, which is the
                        // defect that left the transcription screen saying
                        // "Starting..." for ever (9e1a3f6).
                        setInstalling(null);
                      }
                    });
                  }}
                >
                  {installing === m.id ? "Installing…" : "Install"}
                </button>
              )}
            </li>
          ))}
        </ul>
        <p className="caveat">
          Both are needed and neither works alone. The first one installed brings
          the sherpa runtime with it — the same runtime{" "}
          <code>Settings &gt; Transcription &gt; Local</code> uses, so if you already have a local model
          it is already there.
        </p>
      </section>

      <section>
        <h2>Providers</h2>
        <p className="caveat">
          Every provider this application knows the wire details for. A provider
          with no client is listed rather than hidden, so the reason is visible.
        </p>
        {providers.map((p) => (
          <div key={p.id} className="row">
            <span className="row-label">{p.name}</span>
            <span className="row-value">
              <code>
                {p.audioLeavesDevice
                  ? "audio leaves this machine"
                  : "audio stays here"}
              </code>
              <em className="row-detail">{availabilityLabel(p.availability)}</em>
              <em className="row-detail">{keytermLabel(p.keyterms)}</em>
              {p.sessionLimitSeconds !== null ? (
                <em className="row-detail warn">
                  sessions end after {Math.round(p.sessionLimitSeconds / 60)}{" "}
                  minutes, so a longer meeting has to reconnect
                </em>
              ) : null}
              {/*
                `tokenRotation` records what the *provider* documents, and this
                row used to read "its token has to be refreshed mid-session" --
                which describes an application that mints short-lived tokens.
                This one does not: the header carries the key the user stored,
                so there is nothing to refresh and no refresh is scheduled. The
                capability is kept because it is true of the provider; the
                sentence now says what it means here.
              */}
              {p.tokenRotation ? (
                <em className="row-detail">
                  this provider also issues short-lived tokens that need
                  refreshing; your own key does not, and none is scheduled
                </em>
              ) : null}
            </span>
          </div>
        ))}
      </section>
    </div>
  );
}
