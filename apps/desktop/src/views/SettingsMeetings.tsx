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
  getMeetingKeyterms,
  getMeetingLanguage,
  getMeetingProviders,
  getMeetingStatus,
  reviewMeetingConfig,
  setMeetingKeyterms,
  setMeetingLanguage,
  setMeetingPrompt,
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
  detectedLabel,
  keytermLabel,
  keytermReviewLabel,
  languageRequiredWarning,
  meetingOutcomeLabel,
  sourcesLabel,
  systemAudioValue,
} from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

/** One keyterm per line, trimmed, with blank lines dropped. */
function parseKeyterms(text: string): string[] {
  return text
    .split("\n")
    .map((k) => k.trim())
    .filter((k) => k.length > 0);
}

/** The newest partial per source, which is what a partial means. */
type Live = Partial<Record<TranscriptEvent["source"], string>>;

export function SettingsMeetings() {
  const load = useCallback(async () => {
    const [providers, status, language, keyterms] = await Promise.all([
      getMeetingProviders(),
      getMeetingStatus(),
      getMeetingLanguage(),
      getMeetingKeyterms(),
    ]);
    return { providers, status, language, keyterms };
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
  const [keytermsDraft, setKeytermsDraft] = useState<string | null>(null);
  const [review, setReview] = useState<ConfigReview | null>(null);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void onMeetingState((next) => {
      setState(next);
      if (next === "idle") setSources(null);
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
  const effectiveKeytermsText = keytermsDraft ?? (data?.keyterms ?? []).join("\n");

  useEffect(() => {
    if (!selected) {
      setReview(null);
      return;
    }
    let cancelled = false;
    void reviewMeetingConfig(
      selected,
      effectiveLanguage,
      parseKeyterms(effectiveKeytermsText),
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

  const { providers, status } = data;
  const current = state ?? status.state;
  const recording = current === "recording";
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
    await setMeetingKeyterms(parseKeyterms(effectiveKeytermsText));
  };

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.meetings">
        <h2>Meetings</h2>

        <Row
          label="Status"
          value={
            current === "recording"
              ? "Recording"
              : current === "finishing"
                ? "Transcribing the recording…"
                : "Not recording"
          }
          detail={
            recording
              ? sourcesLabel(sources ?? status.sources)
              : current === "finishing"
                ? "this runs over the whole recording and can take a while on a long meeting"
                : undefined
          }
        />
        <Row
          label="System audio"
          value={systemAudioValue(status.systemAudio)}
          detail={
            status.systemAudio.state === "available"
              ? `${status.systemAudio.mechanism} — ${status.systemAudio.device}`
              : status.systemAudio.reason
          }
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
              <span className="row-label">Keyterms</span>
              <span className="row-value controls">
                <textarea
                  aria-label="Meeting keyterms, one per line"
                  placeholder="one term per line"
                  rows={3}
                  value={effectiveKeytermsText}
                  disabled={recording}
                  onChange={(e) => setKeytermsDraft(e.target.value)}
                  onBlur={() => {
                    void guard(persistLanguageAndKeyterms);
                  }}
                />
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
                label="Keyterms in use"
                value={keytermReview.value}
                detail={keytermReview.detail}
              />
            ) : null}
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

      {recording || Object.keys(live).length > 0 ? (
        <section>
          <h2>Live transcript</h2>
          <p className="caveat">
            Each line is the most recent few seconds, replaced as it is revised.
            The transcript that gets saved comes from one pass over the whole
            recording when the meeting ends, so it will read better than this.
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
        {status.notificationExcluded ? (
          <Row
            label="The recording window"
            value={captureExclusionLabel(status.notificationExcluded).text}
            detail={captureExclusionLabel(status.notificationExcluded).detail}
          />
        ) : null}
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
              {p.tokenRotation ? (
                <em className="row-detail">
                  its token has to be refreshed mid-session
                </em>
              ) : null}
            </span>
          </div>
        ))}
      </section>
    </div>
  );
}
