import { useSettingsDraft } from "../components/SettingsDrafts";
/**
 * Settings > Meetings: how meetings are transcribed, detected and labelled.
 *
 * The provider, the language and the vocabulary a meeting will use, whether
 * its recording is deleted once its transcript is saved (KNEG-409), the
 * notification window, which applications count as a meeting, speaker labels,
 * and every provider this build describes. The recording itself -- Start,
 * Stop, the live transcript, interrupted recordings -- is on the Meetings page
 * (`views/Meetings.tsx`), which reads what is set here and also sets the
 * provider and the language, from its control bar chips' menus. This screen
 * keeps its own controls for both: it is where a language the menu does not
 * list is typed, and where "Use X" answers a saved provider that cannot be
 * used.
 *
 * That split puts one obligation on this screen: whatever it shows has to be
 * saved before anybody can press Start, because Start is now a page away and
 * reads both back rather than being handed them. Rust reads the language from
 * the database when the meeting starts (`start_live`); the provider is the one
 * the Meetings page reads back from `meetings.provider` and passes to
 * `meeting_start`. So the provider is saved on change -- or, when the saved one
 * cannot be used, by the Use button under the caveat that says so -- and the
 * language when its field is left or Enter is pressed. There is no later
 * moment to save them in -- the save that used to run just before Start went
 * with the button.
 *
 * And one from the other direction: a meeting can start while this screen is
 * open -- from the notification window -- so it follows `onMeetingState` and
 * disables the provider and the language while a meeting is live, rather than
 * showing fields for settings the running meeting has already been started
 * with. (The meeting hotkey starts nothing yet: `shortcuts.rs` dispatches it
 * as "Meeting capture is not built yet.")
 *
 * Two things this screen refuses to do.
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
 * That provider also takes a model, saved on change like the provider and read
 * by Rust at Start (`meetings.localModel`). It is read on its own, as the
 * Meetings page reads the same statuses, so a failure costs that row and not
 * the screen.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import {
  getDiarizationStatus,
  getLocalStatus,
  getMeetingDeleteRecordings,
  getMeetingDetectLists,
  getMeetingKeyterms,
  getMeetingLanguage,
  getMeetingLocalModel,
  getMeetingProvider,
  getMeetingProviders,
  getMeetingStatus,
  getSherpaStatus,
  installDiarizationModel,
  onMeetingState,
  resetMeetingNotificationPlacement,
  setDiarizationEnabled,
  setMeetingDeleteRecordings,
  setMeetingDetectLists,
  setMeetingLanguage,
  setMeetingLocalModel,
  setMeetingPrompt,
  setMeetingProvider,
  type MeetingState,
} from "../lib/ipc";
import {
  availabilityLabel,
  captureExclusionLabel,
  detectedLabel,
  diarizationBlocker,
  diarizationRoleDetail,
  keytermLabel,
  keytermReviewLabel,
  languageRequiredWarning,
  meetingIsLive,
  microphoneDetail,
  microphoneValue,
} from "../lib/labels";
import { AUTOMATIC_MODEL, meetingModelDetail, meetingModelOptions } from "../lib/meetingModel";
import { selectProvider } from "../lib/meetingProvider";
import { onePerLine, useMeetingReview } from "../lib/useMeetingReview";
import { useRefresh } from "../lib/useRefresh";

/** "2 of 4 used": the count in the pixel face, as the numbers on these screens are. */
function pixelCount(value: string) {
  const parts = /^(\d+ of \d+)( .*)$/.exec(value);
  return parts ? <span><span className="vocabulary-count">{parts[1]}</span>{parts[2]}</span> : value;
}

export function SettingsMeetings({ onGoToVocabulary }: { onGoToVocabulary: () => void }) {
  const load = useCallback(async () => {
    const [providers, status, language, keyterms, detect, diarization, provider, deleteRecordings] =
      await Promise.all([
        getMeetingProviders(),
        getMeetingStatus(),
        getMeetingLanguage(),
        getMeetingKeyterms(),
        getMeetingDetectLists(),
        getDiarizationStatus(),
        getMeetingProvider(),
        getMeetingDeleteRecordings(),
      ]);
    return { providers, status, language, keyterms, detect, diarization, provider, deleteRecordings };
  }, []);

  const { data, error, reload, guard, fail } = useRefresh(load);
  const loadModels = useCallback(async () => {
    const [local, sherpa, chosen] = await Promise.all([getLocalStatus(), getSherpaStatus(), getMeetingLocalModel()]);
    return { local, sherpa, chosen };
  }, []);
  const models = useRefresh(loadModels);
  /** The model being saved, shown until the read after it; `null` otherwise. */
  const [modelChoice, setModelChoice] = useState<string | null>(null);

  /** Live meeting state, which arrives by event between reads. */
  const [state, setState] = useState<MeetingState | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  // `null` means "not yet touched" -- the loaded, saved value is shown until
  // the user types something, the same idea `chosen` already uses for the
  // provider select.
  const [languageDraft, setLanguageDraft] = useState<string | null>(null);
  const [allowDraft, setAllowDraft] = useState<string | null>(null);
  const [blockDraft, setBlockDraft] = useState<string | null>(null);
  /** Which diarization model is downloading, or null. */
  const [installing, setInstalling] = useState<string | null>(null);
  const [resettingNotification, setResettingNotification] = useState(false);
  const [notificationResetError, setNotificationResetError] = useState<string | null>(null);

  const resetNotification = async () => {
    setResettingNotification(true);
    setNotificationResetError(null);
    try {
      await preferencesGuard.run(resetMeetingNotificationPlacement, "notification-placement");
    } catch (error) {
      setNotificationResetError(String(error));
    } finally {
      setResettingNotification(false);
    }
  };

  // Only the state, and only so the provider and the language can be
  // disabled while a meeting is live. The meeting's own events -- the
  // transcript, the outcome -- are the Meetings page's; nothing on this screen
  // changes when a meeting ends. `disposed` because `listen` resolves after
  // the effect returns, and a screen left before it did would otherwise keep
  // its listener for the rest of the session.
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void onMeetingState((next) => {
      if (!disposed) setState(next);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);

  // Computed defensively before `data` is known to have loaded, and reused
  // below once it has, so the review hook can sit above the loading guard
  // rather than after it -- a hook cannot be called only on some renders.
  const usable = (data?.providers ?? []).filter(
    (p) => p.availability.state === "available",
  );
  // The saved choice wins over the first usable provider, and a choice made
  // on this visit wins over the saved one. `lib/meetingProvider.ts` says why
  // the fallback never writes (KNEG-317).
  const { selected, savedUnusable } = selectProvider(
    chosen,
    data?.provider ?? null,
    usable.map((p) => p.id),
  );
  const effectiveLanguage = languageDraft ?? data?.language ?? "";
  const effectiveKeytermsText = (data?.keyterms ?? []).join("\n");
  const review = useMeetingReview(selected, effectiveLanguage, effectiveKeytermsText);
  // Asked of `meetingIsLive`, the same question the Meetings page asks before
  // it offers Start, so the two screens cannot disagree about whether a
  // meeting is running. Above the loading guard for the effect below.
  const recording = meetingIsLive(state ?? data?.status.state ?? "idle");
  // A meeting started elsewhere freezes configuration, but keeps the draft for after it ends.
  const languageSaved = useRef<string | null>(null);
  const providerSaved = useRef<{ value: string | null } | null>(null);
  const listsSaved = useRef<{ allow: string[]; block: string[] } | null>(null);
  const languageNow = useRef(languageDraft); languageNow.current = languageDraft;
  const providerNow = useRef(chosen); providerNow.current = chosen;
  const listsNow = useRef({ allow: allowDraft, block: blockDraft }); listsNow.current = { allow: allowDraft, block: blockDraft };
  const persistLanguage = async () => {
    const submitted = languageNow.current;
    if (submitted === null) return;
    if (recording) throw new Error("Wait until the meeting ends to save its language.");
    await setMeetingLanguage(submitted.trim()); languageSaved.current = submitted.trim();
    // Keep the acknowledged value visible even if the following read fails.
    await reload();
  };
  const persistProvider = async () => {
    const submitted = providerNow.current;
    if (submitted === null) return;
    if (recording) throw new Error("Wait until the meeting ends to change its provider.");
    await setMeetingProvider(submitted); providerSaved.current = { value: submitted };
    await reload();
  };
  const persistLists = async () => {
    if (!data) throw new Error("Settings haven't loaded yet.");
    const submitted = listsNow.current;
    const lists = { allow: onePerLine(submitted.allow ?? data.detect.allow.join("\n")), block: onePerLine(submitted.block ?? data.detect.block.join("\n")) };
    await setMeetingDetectLists(lists); listsSaved.current = lists;
    await reload();
  };
  const preferencesGuard = useSettingsDraft({ label: "Meeting preferences", order: 3, dirty: false, save: async () => {}, discard: () => {} });
  const modelGuard = useSettingsDraft({ label: "Meeting model", order: 4, dirty: false, save: async () => {}, discard: () => {} });
  const languageGuard = useSettingsDraft({ label: "Meeting language", order: 1, dirty: languageDraft !== null && languageDraft.trim() !== (languageSaved.current ?? data?.language ?? ""), save: persistLanguage, discard: () => setLanguageDraft(languageSaved.current ?? data?.language ?? null) });
  const providerGuard = useSettingsDraft({ label: "Meeting provider", dirty: chosen !== null && chosen !== (providerSaved.current?.value ?? data?.provider ?? null), save: persistProvider, discard: () => setChosen(providerSaved.current?.value ?? data?.provider ?? null) });
  const listGuard = useSettingsDraft({ label: "Meeting detection lists", order: 2, dirty: (allowDraft !== null || blockDraft !== null) && JSON.stringify({ allow: onePerLine(allowDraft ?? data?.detect.allow.join("\n") ?? ""), block: onePerLine(blockDraft ?? data?.detect.block.join("\n") ?? "") }) !== JSON.stringify(listsSaved.current ?? data?.detect), save: persistLists, discard: () => { setAllowDraft(listsSaved.current?.allow.join("\n") ?? null); setBlockDraft(listsSaved.current?.block.join("\n") ?? null); } });
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
  const selectedProvider = providers.find((p) => p.id === selected) ?? null;
  const savedProvider = savedUnusable
    ? (providers.find((p) => p.id === savedUnusable) ?? null)
    : null;
  const keytermReview =
    review && selectedProvider
      ? keytermReviewLabel(review, selectedProvider.keyterms)
      : null;
  const languageWarning = review ? languageRequiredWarning(review) : null;

  // Persist exactly the value the review above just described, so a saved
  // setting can never lag behind what the screen showed. On blur and on
  // Enter, because Start is on the Meetings page now and there is no save
  // right before it any more: `MeetingRuntime::start` reads the language from
  // the database, and a preview describing a configuration it will not use is
  // the defect this used to be awaited before `startMeeting` to close.
  // Nothing typed is nothing to save -- the field is showing the saved value.
  const saveLanguage = () => { void languageGuard.run(persistLanguage).catch(fail); };

  // Both lists go in one write because they are one setting: `meetings.detect`
  // holds the pair, so saving either alone would have to re-send the other.
  //
  // Reloaded afterwards, but the list of running applications above will not
  // change for up to five seconds: `status.detected` is whatever the detection
  // timer last computed, not a fresh walk, and the timer is what reads the
  // lists back. Saying so here rather than reaching for a fresh poll — a walk
  // costs a pass over every process on the machine, and the screen catching up
  // one tick later is not worth spending that on every keystroke-and-blur.
  const persistDetectLists = () => listGuard.run(persistLists);

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}
      {preferencesGuard.feedback}

      <section data-surface="settings.meetings">
        {usable.length === 0 ? (
          <p className="caveat">
            No meeting provider can be used yet. The local one needs a model
            server running — start one under Settings &gt; Transcription &gt;
            Local.
          </p>
        ) : null}

        {/* Said, not left to a greyed-out field: a control that stops
            responding with no reason on screen is the button that silently
            does nothing. */}
        {recording ? (
          <p className="caveat">
            A meeting is being recorded, so the provider, its model and the
            language can't be changed until it ends.
            {languageDraft !== null && languageDraft.trim() !== (languageSaved.current ?? data.language) && " Your unsaved language is kept for after the meeting; it does not change the meeting being recorded."}
          </p>
        ) : null}

        <div className="row">
          <span className="row-label">Provider</span>
          <div className="row-value">
            <select
              aria-label="Meeting provider"
              value={selected ?? ""}
              disabled={recording}
              onChange={(e) => {
                const id = e.target.value;
                setChosen(id);
                // Saved on change rather than at Start, so a choice survives
                // a restart even when no meeting was recorded after making it
                // -- and so the Meetings page, which reads it back, names it.
                void providerGuard.run(async () => { await setMeetingProvider(id); providerSaved.current = { value: id }; await reload(); }).catch(fail);
              }}
            >
              {usable.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
              {providerGuard.feedback}
              {selectedProvider ? (
                <p className="caveat">
                  {/* Some summaries end in a full stop already: Deepgram's read
                      "no session limit.." */}
                  {/[.!?]$/.test(selectedProvider.summary) ? selectedProvider.summary : `${selectedProvider.summary}.`}{" "}
                  {selectedProvider.audioLeavesDevice
                    ? "Audio is sent to this provider."
                    : "Audio stays on this computer."}
                </p>
              ) : null}
          </div>
        </div>

        {/* Only for the provider it means anything to. The streaming provider's
            model is chosen under Settings > Transcription > Local. Meetings'
            transcription chip writes the same setting. */}
        {selected === "local" && models.error ? <p className="error">{models.error}</p> : null}
        {selected === "local" && models.data ? (
          <div className="row">
            <span className="row-label">Model</span>
            <div className="row-value">
              <select
                aria-label="Meeting model"
                value={modelChoice ?? models.data.chosen ?? AUTOMATIC_MODEL}
                disabled={recording}
                onChange={(e) => {
                  const id = e.target.value;
                  setModelChoice(id);
                  void modelGuard
                    .run(async () => {
                      await setMeetingLocalModel(id === AUTOMATIC_MODEL ? null : id);
                      await models.reload();
                    })
                    .catch(fail)
                    .finally(() => setModelChoice(null));
                }}
              >
                {meetingModelOptions(models.data.local.models, models.data.chosen).map((option) => (
                  <option key={option.id} value={option.id} disabled={option.disabled}>
                    {option.label}
                  </option>
                ))}
              </select>
              {modelGuard.feedback}
              <em className="row-detail">
                {meetingModelDetail({
                  models: models.data.local.models,
                  chosen: models.data.chosen,
                  runningModel: models.data.local.runningModel,
                  parakeet:
                    models.data.sherpa.selected === null
                      ? null
                      : (models.data.sherpa.models.find((m) => m.id === models.data?.sherpa.selected)?.name ?? models.data.sherpa.selected),
                })}
              </em>
            </div>
          </div>
        ) : null}

        {/* The Meetings page holds Start back while this is showing, and its
            "Choose a provider" comes here. The select above cannot answer it
            on its own: it already shows the fallback, and choosing the option
            a select already shows fires no change, so with one usable
            provider there would be nothing to press. The button is that
            choice, made explicitly -- the fallback is never saved by itself,
            because it can be the opposite privacy choice from the saved one. */}
        {savedUnusable ? (
          <>
            {/* Until KNEG-354 this said the fallback "is selected instead.
                That choice is kept until you pick another." -- above a
                button that has to be pressed before Meetings will record.
                A provider this version does not offer is not named by its
                raw id; the id is folded. */}
            <p className="caveat">
              {savedProvider
                ? `${savedProvider.name} was chosen last time but can't be used right now (${availabilityLabel(savedProvider.availability)}).`
                : "The provider chosen last time isn't offered by this version of the app."}{" "}
              {`Meetings won't record until you choose a provider${
                selectedProvider && !recording
                  ? `: use ${selectedProvider.name} below, or pick another above.`
                  : " above."
              }`}
            </p>
            {savedProvider ? null : (
              <LearnMore>
                <p>{`It was saved as “${savedUnusable}”.`}</p>
              </LearnMore>
            )}
            {selectedProvider && !recording ? (
              <button
                type="button"
                onClick={() => {
                  const id = selectedProvider.id;
                  setChosen(id);
                  void providerGuard.run(async () => { await setMeetingProvider(id); providerSaved.current = { value: id }; await reload(); }).catch(fail);
                }}
              >
                {`Use ${selectedProvider.name}`}
              </button>
            ) : null}
          </>
        ) : null}

        {selectedProvider ? (
          <>
            <div className="row">
              <span className="row-label">Language</span>
              <div className="row-value">
                <input
                  type="text"
                  aria-label="Meeting language"
                  placeholder="leave blank to detect automatically, where the provider supports it"
                  value={effectiveLanguage}
                  disabled={recording}
                  onChange={(e) => setLanguageDraft(e.target.value)}
                  onBlur={(e) => { if (!e.currentTarget.disabled) saveLanguage(); }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.nativeEvent.isComposing) saveLanguage();
                  }}
                />
                {languageGuard.feedback}
              </div>
            </div>
            <div className="row">
              <span className="row-label">Vocabulary</span>
              <div className="row-value">
                <button type="button" onClick={onGoToVocabulary}>Manage vocabulary</button>
                <span className="row-detail">{data?.keyterms.length ?? 0} saved terms · changes apply to the next meeting</span>
              </div>
            </div>
            {/*
             * `validate` computed this, not a re-implementation here — see
             * `kw_meeting_core::validate`. Shown for the fields above as
             * they are edited, and saved as each is left: the provider on
             * change, the language on blur or Enter. Start is on the
             * Meetings page, which reads the saved values back, so what is
             * displayed here is what gets persisted before it can be used.
             */}
            {keytermReview ? (
              <Row
                label="Vocabulary for next meeting"
                value={pixelCount(keytermReview.value)}
                detail={keytermReview.detail}
              />
            ) : null}
            {selected === "deepgram-realtime" && (data?.keyterms.length ?? 0) > 0 && <p className="muted">Vocabulary-enabled sessions use Deepgram Nova-3.</p>}
            {languageWarning ? <p className="caveat">{languageWarning}</p> : null}
          </>
        ) : null}
      </section>

      {/* KNEG-409. Not held while a meeting records: the meeting took its
          choice at Start, which the Learn more says, and the next one takes
          what is ticked here. What it costs -- no playback -- is in view. */}
      <section>
        <h2>recordings</h2>
        <div className="row">
          <span className="row-label">Delete recordings</span>
          <div className="row-value">
            <label className="switch-row">
              <span>Delete each recording once its transcript is saved</span>
              <input
                type="checkbox"
                role="switch"
                checked={data.deleteRecordings}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void preferencesGuard.run(async () => {
                    await setMeetingDeleteRecordings(enabled);
                    await reload();
                  }, "retention").catch(fail);
                }}
              />
            </label>
            <em className="row-detail">
              The note keeps the transcript and your typed notes, but can't be
              played back. If the transcript may be incomplete, the recording is
              kept.
            </em>
          </div>
        </div>
        <LearnMore>
          <p>
            Changing this doesn't affect a meeting that's already recording.
            Discard always deletes the recording.
          </p>
        </LearnMore>
      </section>

      <section>
        <h2>notification window</h2>
        <button type="button" disabled={resettingNotification} onClick={() => { void resetNotification(); }}>Reset notification window</button>
        <LearnMore>
          <p>Drag the notification window to move it or resize it by an edge. Reset returns it to the bottom-right at its default size.</p>
        </LearnMore>
        {notificationResetError ? <p className="error" role="alert">{notificationResetError}</p> : null}
      </section>

      <section>
        <h2>detection</h2>
        {/* Visible, not folded: "nothing is ever recorded without you pressing
            a button" is the promise this whole section is built around. The
            matching rules further down are the explanation, and those are. */}
        <p className="muted">
          Meeting applications that are running. Nothing is ever recorded without
          you pressing a button — when a meeting is detectably in progress, a
          small window offers to record it.
        </p>
        <div className="row">
          <span className="row-label">Offer to record</span>
          <div className="row-value">
            <label className="switch-row">
              <span>Show a window when a meeting starts</span>
              <input
                type="checkbox"
                role="switch"
                checked={status.promptOnDetect}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void preferencesGuard.run(async () => {
                    await setMeetingPrompt(enabled);
                    await reload();
                  }, "notification").catch(fail);
                }}
              />
            </label>
          </div>
        </div>
        {/*
          * Reported here, above the applications it is evidence about: where
          * this is anything but "Watching", every "not in a call" further down
          * is an absence of evidence rather than evidence of absence, and the
          * user is the only one who can tell that from the screen. It used to
          * sit beside the system-audio row, as the same kind of fact; that row
          * went to the Meetings page with the checkbox it decides.
          */}
        <Row
          label="Microphone activity"
          value={
            <span className="state-tag" data-tone={status.microphone?.state === "available" && status.microphone.endpoints > 0 ? "on" : "open"}>
              {status.microphone?.state === "available" && status.microphone.endpoints > 0 ? <span className="state-tag-dot" aria-hidden="true" /> : null}
              {microphoneValue(status.microphone)}
            </span>
          }
          detail={microphoneDetail(status.microphone)}
        />
        {status.detected.length === 0 ? (
          <p className="muted">No meeting application is running.</p>
        ) : (
          status.detected.map((d) => (
            <Row
              key={`${d.provider}-${d.evidence}`}
              label={d.name}
              value={
                <span className="state-tag" data-tone={d.inCall ? "call" : "open"}>
                  {d.inCall ? <span className="state-tag-dot" aria-hidden="true" /> : null}
                  {d.inCall ? "In a call" : "Open"}
                </span>
              }
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
          <div className="row-value">
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
          </div>
        </div>
        <div className="row">
          <span className="row-label">Never count as a meeting</span>
          <div className="row-value">
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
                {listGuard.feedback}
          </div>
        </div>
        <LearnMore>
          <p>
            Names are matched against the running program's own filename, not its
            window title, and capitalisation does not matter — the name each
            detection above is reported by is the one to copy. A name in the second
            list is never offered even when this application recognises it on its
            own, which is how a false positive is silenced without waiting for a
            release. A program you list yourself is reported as running, never as
            being in a call: only you know whether it is.
          </p>
        </LearnMore>
        {status.notificationExcluded ? (
          <Row
            label="The recording window"
            value={captureExclusionLabel(status.notificationExcluded).text}
            detail={captureExclusionLabel(status.notificationExcluded).detail}
          />
        ) : null}
      </section>

      <section>
        <h2>speaker labels</h2>
        {/* What it does stays visible; what a label does and does not mean is
            folded. The two measured caveats are not: they are what switching
            this on costs, and what it gets wrong. */}
        <p className="muted">
          A second pass over the finished recording, grouping the voices in it so
          each passage of the saved note says who was speaking.
        </p>
        <LearnMore>
          <p>
            It answers how many voices and when, never <em>who</em> — a label is{" "}
            <code>Speaker 1</code>, it means nothing outside this one recording, and
            the same person is <code>Speaker 1</code> in one meeting and{" "}
            <code>Speaker 3</code> in the next.
          </p>
        </LearnMore>
        <p className="caveat warn">
          Measured on this machine: the pass costs about 8% of the meeting’s
          length, so an hour-long recording adds roughly five minutes when you
          stop and save, even with a streaming model.
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
          value={<span className="state-pixel" data-ready={diarization.ready}>{diarization.ready ? "ready" : "not ready"}</span>}
          detail={diarizationBlocker(diarization) ?? undefined}
        />
        {/* Meetings' Speaker labels chip writes the same setting. */}
        <div className="row">
          <span className="row-label">Label speakers</span>
          <div className="row-value">
            <label className="switch-row">
              <span>Add speaker labels to saved meeting notes</span>
              <input
                type="checkbox"
                role="switch"
                checked={diarization.enabled}
                disabled={!diarization.ready && !diarization.enabled}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void preferencesGuard.run(async () => {
                    await setDiarizationEnabled(enabled);
                    await reload();
                  }, "speaker-labels").catch(fail);
                }}
              />
            </label>
          </div>
        </div>
        <ul className="model-cards">
          {diarization.models.map((m) => (
            <li key={m.id} className="model-card">
              <span className="model-card-id">{m.id.toUpperCase()}</span>
              <span className="model-card-name">{m.name}{m.sizeMb ? ` · ${m.sizeMb} MB` : ""}</span>
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
        {/* Folded: which half is missing is already on the Status row above,
            in `diarizationBlocker`'s words. This is why, and where the runtime
            comes from. */}
        <LearnMore summary="About the two models">
          <p>
            Both are needed and neither works alone. The first one installed brings
            the sherpa runtime with it — the same runtime{" "}
            <code>Settings &gt; Transcription &gt; Local</code> uses, so if you already have a local model
            it is already there.
          </p>
        </LearnMore>
      </section>

      <section>
        <h2>all meeting providers</h2>
        <LearnMore>
          <p>
            Every provider this application knows the wire details for. A provider
            with no client is listed rather than hidden, so the reason is visible.
          </p>
        </LearnMore>
        {providers.map((p) => (
          <div key={p.id} className="row">
            <span className="row-label">{p.name}</span>
            <span className="row-value">
              {p.audioLeavesDevice
                ? "audio leaves this computer"
                : "audio stays on this computer"}
              {/*
                `tokenRotation` records what the *provider* documents, and this
                row used to read "its token has to be refreshed mid-session" --
                which describes an application that mints short-lived tokens.
                This one does not: the header carries the key the user stored,
                so there is nothing to refresh and no refresh is scheduled. The
                capability is kept because it is true of the provider; the
                sentence now says what it means here.
              */}
              <em className="row-detail">
                {[
                  availabilityLabel(p.availability),
                  keytermLabel(p.keyterms),
                  p.sessionLimitSeconds !== null
                    ? `sessions end after ${Math.round(p.sessionLimitSeconds / 60)} minutes, so a longer meeting has to reconnect`
                    : null,
                  p.tokenRotation
                    ? "this provider also issues short-lived tokens that need refreshing; your own key does not, and none is scheduled"
                    : null,
                ]
                  .filter((part) => part !== null)
                  .join(" · ")}
              </em>
            </span>
          </div>
        ))}
      </section>
    </div>
  );
}
