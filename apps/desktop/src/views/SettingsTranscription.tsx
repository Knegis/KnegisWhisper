import { LocalLanguagePicker } from "../components/LocalLanguagePicker";
/**
 * Settings > Transcription, and Settings > Transcription > Local.
 *
 * Two surfaces on one screen because the choice is one choice: a dictation goes
 * either to a cloud provider or to a model on this machine, never both, and
 * `dictation_route` resolves which. Showing them apart would let a user set the
 * route on one page and see it contradicted on another.
 *
 * Keys are not entered here. That is Settings > Providers, which this screen
 * links to rather than duplicating — the earlier version of this section listed
 * every provider, said whether a key was stored, and offered no way to store
 * one, which is how the parity gate came to need a `reachable` field separate
 * from `built`.
 *
 * # In plain words (KNEG-343)
 *
 * The copy rule the task pages follow, applied here too: failures, where
 * audio goes, and anything to do now stay in view; model ids, credential
 * slots, ports, paths, engine names and the mechanism go under Learn more or a
 * row's Details. Nothing true was dropped, only moved. The engines are "the
 * speech engine" (whisper.cpp's server) and "the Parakeet engine"
 * (sherpa-onnx), named under Details. Provider model ids stay as the options of
 * their menus: they are the names the providers sell them by, and there are no
 * others.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { LocalModelRow } from "../components/LocalModelRow";
import { MicrophoneSettings } from "../components/MicrophoneSettings";
import { Row } from "../components/Row";
import { destinationName, LANGUAGES, languageNote, privacyLines } from "../lib/controlBar";
import { languagesApply, localEngine } from "../lib/dictationRoute";
import {
  getDictationRoute,
  getLocalDictationLanguages,
  getLocalStatus,
  getProviders,
  getSherpaStatus,
  installLocalModel,
  installSherpaModel,
  onInstallProgress,
  getSpeechInstallations,
  onSpeechInstallations,
  type SpeechInstall,
  onLocalDictationLanguages,
  refreshLocalServer,
  setCloudProvider,
  startLocalServer,
  stopLocalServer,
  useLocalModel,
  useSherpaModel,
  useSherpaStreamModel,
  type Dispatch,
  type ProviderSummary,
} from "../lib/ipc";
import {
  freshnessLabel,
  progressLabel,
  refreshOutcomeLabel,
  type Plain,
} from "../lib/labels";
import {
  filterByLanguage,
  localModelRows,
  modelLanguageLabel,
  type LocalModelRow as ModelRow,
} from "../lib/localModels";
import { chooseParakeetRoute, chooseWhisperRoute } from "../lib/routeChoice";
import { useRefresh } from "../lib/useRefresh";

/**
 * Whether dictation could actually use this provider, said plainly.
 *
 * `needs-client` is not a failure of configuration and must not read like one:
 * four of the seven providers use an API shape this build has no client for, and
 * the route refuses rather than sending the audio somewhere else. That refusal
 * is the correct behaviour, so the screen names it rather than hiding it behind
 * a greyed-out control with no explanation. The reason Rust gives -- "an
 * unattested request would present as Tinfoil", "a model id is required" -- is
 * the mechanism, so it goes under Details (KNEG-343).
 */
function dispatchLabel(d: Dispatch): Plain {
  switch (d.state) {
    case "ready":
      return { text: "Can be used", more: null };
    case "needs-client":
      // Not always "unwritten". Tinfoil is refused on purpose, and the reason
      // that arrives says which case it is -- so the line says neither "yet"
      // nor "never", and the reason under Details tells them apart
      // (`commands.rs` pins Tinfoil's as not reading like unfinished work).
      return { text: "Can't be used for dictation", more: d.reason };
    case "unresolvable":
      // Usually a missing setting rather than a missing feature — Corti with no
      // environment or tenant is the case this exists for.
      return { text: "Needs setting up", more: d.reason };
  }
}

/** A freshness line and its builds, the same way for both engines. */
function Freshness({ line, current }: { line: Plain; current: boolean }) {
  return (
    <>
      <p className={current ? "muted" : "muted warn"}>{line.text}</p>
      {line.more ? (
        <LearnMore summary="Which version">
          <p>{line.more}</p>
        </LearnMore>
      ) : null}
    </>
  );
}

/**
 * The row id the server-binary progress line belongs to.
 *
 * `runInstall` scopes its progress line to whichever row started the run, so
 * the server binary needs an id of its own. Not a model id, and it cannot
 * collide with one: model ids come from the registry and none of them is a
 * dotted path.
 */
const SERVER_ROW = "sidecar.whisper";

export function SettingsTranscription({
  onGoToProviders,
  onGoToVocabulary,
}: {
  onGoToProviders: () => void;
  onGoToVocabulary: () => void;
}) {
  const load = useCallback(async () => {
    const [providers, local, sherpa, route, languages] = await Promise.all([
      getProviders(),
      getLocalStatus(),
      getSherpaStatus(),
      getDictationRoute(),
      // For the note under the language picker, which says what the route
      // does with them (KNEG-370).
      getLocalDictationLanguages(),
    ]);
    return { providers, local, sherpa, route, languages };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);
  const [jobs, setJobs] = useState<SpeechInstall[]>([]);
  useEffect(() => {
    let disposed = false;
    let revision = -1;
    let stop: (() => void) | undefined;
    const accept = (next: Awaited<ReturnType<typeof getSpeechInstallations>>) => {
      if (!disposed && next.revision >= revision) { revision = next.revision; setJobs(next.jobs); }
    };
    void onSpeechInstallations(accept).then(async unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      accept(await getSpeechInstallations());
    }).catch(() => {});
    return () => { disposed = true; stop?.(); };
  }, []);
  const [installing, setInstalling] = useState<string | null>(null);
  /**
   * Which model's Install button started the run in progress.
   *
   * The progress line used to render once, near the top of the whisper block.
   * Pressing Install on a Parakeet model further down the page therefore
   * produced no visible change at all -- a bench check on 2026-08-28 pressed it
   * and could not tell whether a 631 MB download had begun. A control that
   * appears to do nothing is the defect this file keeps correcting; the fix is
   * to put the line on the row whose button was pressed.
   */
  const [installingId, setInstallingId] = useState<string | null>(null);
  /** What the last press of the speech-engine button did. */
  const [refreshed, setRefreshed] = useState<(Plain & { problem: boolean }) | null>(null);
  /** Per-provider model choice, before it is committed. */
  const [selected, setSelected] = useState<Record<string, string>>({});
  /**
   * The outcome of the last press, so the control is never silent.
   *
   * One message, not one per provider. The record this replaced had two
   * problems and the second is the worse of them. It grew the row it belonged
   * to, so pressing a button pushed every control below it downward at the
   * moment the pointer was over them. And it never cleared: pressing "use for
   * dictation" on two providers left two sentences on screen, both describing
   * where dictation goes, only one of them still true, and nothing saying
   * which. Dictation resolves to exactly one route, so its feedback is one
   * line.
   */
  const [chosen, setChosen] = useState<string | null>(null);
  /**
   * Corti's two scoping values.
   *
   * Only one provider needs them, so they are two fields rather than a generic
   * per-provider settings map — a map would be a shape inviting a second
   * provider's options into it, and no other provider has any.
   */
  const [cortiEnvironment, setCortiEnvironment] = useState<string | null>(null);
  const [cortiTenant, setCortiTenant] = useState<string | null>(null);
  /** The local list's language filter: a dictation language's code, or `null` for any (KNEG-330). */
  const [language, setLanguage] = useState<string | null>(null);

  const choose = (p: ProviderSummary) => {
    const model = selected[p.id] ?? p.models[0];
    if (!model) {
      setChosen(`${p.name} has no model to choose.`);
      return;
    }

    // Corti needs two more values, and Rust refuses the route without them.
    // Checked here so the message names the empty box rather than arriving as a
    // rejected command.
    if (p.id === "corti") {
      const environment = (cortiEnvironment ?? "").trim();
      const tenant = (cortiTenant ?? "").trim();
      if (!environment || !tenant) {
        setChosen(
          "Corti needs both an environment and a tenant. Neither has a safe default.",
        );
        return;
      }
    }

    void guard(async () => {
      await setCloudProvider(
        p.id,
        model,
        p.id === "corti"
          ? {
              environment: (cortiEnvironment ?? "").trim(),
              tenant: (cortiTenant ?? "").trim(),
            }
          : undefined,
      );
      // Where the audio goes is said with the choice, not left to the row
      // above to be noticed (KNEG-343).
      setChosen(
        p.configured
          ? `Dictation now goes to ${p.name} (${model}). Audio is sent to ${p.name}.`
          : `Set to ${p.name} (${model}). It won't work until you add a key on the Providers page.`,
      );
    });
  };

  /**
   * Run an install and always stop saying it is running.
   *
   * `installing` used to be cleared only by the `done` progress event, so an
   * install that was **refused** left "Starting…" under the row for ever — the
   * refusal went to the error banner and the row went on claiming work was
   * happening. Found on the unpinned Nemotron model, whose install is refused
   * outright (`MissingPin`), but it applied to every failure: a dead network, a
   * digest mismatch, a full disk.
   *
   * `guard` swallows the error after showing it, so this cannot tell success
   * from failure and does not need to: either way the install is over by the
   * time it resolves.
   */
  const runInstall = (id: string, work: () => Promise<unknown>) => {
    setInstalling("Starting…");
    setInstallingId(id);
    void guard(work).then(() => {
      setInstalling(null);
      setInstallingId(null);
    });
  };

  /**
   * A local model row's button, once the model is installed.
   *
   * The wrappers are named here rather than in `LocalModelRow`: the settings
   * gate's rule F looks for the writers of `transcription.localModel`,
   * `transcription.sherpaModel` and `meetings.streamModel` in this file and
   * nowhere else (`tools/settings-audit/scan.mjs`, `usesWrapper`).
   */
  const chooseModel = (row: ModelRow) => {
    void guard(async () => {
      if (row.engine === "whisper") {
        // Two steps under one label, and the label says both: a server that
        // is up is not the same as a route pointing at it. The steps are the
        // Dictation route menu's too (`lib/routeChoice.ts`).
        await chooseWhisperRoute(row.id, startLocalServer);
      } else if (row.usedFor === "meetings") {
        // No `useLocalModel` beside it: choosing a meeting model says nothing
        // about where dictation goes, and flipping the dictation route here
        // would be the silent cross-setting this split exists to prevent.
        await useSherpaStreamModel(row.id);
      } else {
        await chooseParakeetRoute(row.id, useSherpaModel);
      }
    });
  };

  const installModel = (row: ModelRow) => {
    runInstall(row.id, () =>
      row.engine === "whisper" ? installLocalModel(row.id) : installSherpaModel(row.id),
    );
  };

  // The progress event carries a model's id; the line names it, from the model
  // lists as they were last read. A ref, so a reload does not resubscribe the
  // listener and drop an event in between.
  const latest = useRef(data);
  latest.current = data;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const nameOf = (id: string) =>
      latest.current?.local.models.find((m) => m.id === id)?.name ??
      latest.current?.sherpa.models.find((m) => m.id === id)?.name ??
      id;
    void onInstallProgress((progress) => {
      if (disposed) return;
      setInstalling(progressLabel(progress, nameOf));
      if (progress.stage === "done") {
        setInstallingId(null);
        void reload();
      }
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [reload]);

  // The languages are chosen on Dictation and from the floating microphone
  // too. On their announcement the page re-reads, the route with them, so the
  // note under the picker says what is sent now (KNEG-370).
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onLocalDictationLanguages(() => {
      if (!disposed) void reload();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [reload]);

  if (!data) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { providers, local, sherpa, route, languages } = data;
  const engine = localEngine(route, sherpa);
  const withKeys = providers.filter((p) => p.configured);
  const serverFreshness = freshnessLabel(local.serverFreshness);
  const sherpaFreshness = freshnessLabel(sherpa.cliFreshness);
  const shown = filterByLanguage(localModelRows(local, sherpa, route), language);

  return (
    <div className="view">
      {jobs.length > 0 && <section aria-label="Speech model installation progress" role="status">{jobs.map(job => <p key={job.id}>{data.local.models.find(model => model.id === job.id)?.name ?? data.sherpa.models.find(model => model.id === job.id)?.name ?? (job.id === "speech-server" ? "Speech processing tools" : job.id)}: {job.progress ? progressLabel(job.progress, id => data.local.models.find(model => model.id === id)?.name ?? data.sherpa.models.find(model => model.id === id)?.name ?? id) : "Starting…"}</p>)}</section>}
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      <MicrophoneSettings />
      <section data-surface="settings.transcription">
        <button type="button" onClick={onGoToVocabulary}>Manage vocabulary</button>
        <h2>Cloud providers</h2>
        <p className="muted">
          Add keys and test them on the Providers page.
        </p>
        <Row
          label="Dictation goes to"
          value={
            route.problem
              ? "Nowhere yet"
              : route.isLocal
                ? "This computer"
                : destinationName(route.destination)
          }
          detail={route.problem ? undefined : privacyLines(route, null)[0]}
        />
        {route.problem ? (
          <FailureLine
            text="Dictation can't be used until this is set up. Nothing is sent anywhere meanwhile."
            detail={route.problem}
          />
        ) : null}
        {/* Under where dictation goes, since the languages apply to that
            route, local Whisper or cloud (KNEG-370). Off under a Parakeet
            model, which uses its own; the last sentence says so either way. */}
        <LocalLanguagePicker
          disabled={!languagesApply(engine)}
          note={`${languageNote(engine, route, languages)} Parakeet models use the languages they were made for.`}
        />
        <Row
          label="Keys saved"
          value={
            withKeys.length === 0
              ? "None"
              : withKeys.map((p) => p.name).join(", ")
          }
          detail={
            withKeys.length === 0
              ? "add one on the Providers page to use a cloud provider"
              : undefined
          }
        />
        {providers.map((p) => {
          const dispatch = dispatchLabel(p.dispatch);
          return (
          <div key={p.id} className="row">
            <span className="row-label">{p.name}</span>
            <span className="row-value">
              {p.configured ? "Key saved" : "No key saved"}
              <em className="row-detail">
                {dispatch.text} · {p.modelCount} model
                {p.modelCount === 1 ? "" : "s"}
              </em>
              <LearnMore summary="Details">
                {dispatch.more ? <p>{dispatch.more}</p> : null}
                <p>
                  Its key is in the credential slot <code>{p.credentialSlot}</code>.
                </p>
              </LearnMore>
              {p.dispatch.state !== "needs-client" ? (
                <span className="controls">
                  <select
                    aria-label={`model for ${p.name}`}
                    value={selected[p.id] ?? p.models[0] ?? ""}
                    onChange={(e) =>
                      setSelected((prev) => ({ ...prev, [p.id]: e.target.value }))
                    }
                  >
                    {/* Straight from the registry. No model id is written here. */}
                    {p.models.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    onClick={() => choose(p)}
                    disabled={p.models.length === 0}
                  >
                    Use for dictation
                  </button>
                </span>
              ) : null}
              {p.id === "corti" ? (
                <span className="controls">
                  <input
                    aria-label="Corti environment"
                    placeholder="environment, e.g. eu"
                    value={cortiEnvironment ?? ""}
                    onChange={(e) => setCortiEnvironment(e.target.value)}
                  />
                  <input
                    aria-label="Corti tenant"
                    placeholder="tenant name"
                    value={cortiTenant ?? ""}
                    onChange={(e) => setCortiTenant(e.target.value)}
                  />
                </span>
              ) : null}
            </span>
          </div>
          );
        })}
        {/*
          Always rendered, never conditional, and below every control it reports
          on. The slot holds its height empty, so a press fills it instead of
          growing the section under the pointer. `role="status"` because the
          message now sits further from the button that caused it than it did
          when it was inline, and a screen reader should still hear it.
        */}
        <p className="status-slot row-detail" role="status">
          {chosen}
        </p>
        <button type="button" onClick={onGoToProviders}>
          Open Providers
        </button>
      </section>

      <section data-surface="settings.transcription.local">
        <h2>Local transcription</h2>
        <p className="muted">
          Speech models that run on this computer. Nothing downloads until you
          press Install.
        </p>
        <div className="row">
          <span className="row-label">Show models for</span>
          <span className="row-value controls">
            <select
              aria-label="Show models for"
              value={language ?? ""}
              onChange={(e) => setLanguage(e.target.value === "" ? null : e.target.value)}
            >
              <option value="">Any language</option>
              {LANGUAGES.map(([code, name]) => (
                <option key={code} value={code}>
                  {name}
                </option>
              ))}
            </select>
          </span>
        </div>
        {/*
          One list, Whisper and Parakeet together (KNEG-330): dictation models
          first, then the meetings models, each smallest first. Which engine a
          model runs on is under its Details; the in-use rules are
          `lib/localModels.ts`'s. `list`, not `purposes`: these rows carry a
          button each, and `purposes` is a wrapping row of pills that cannot
          hold one.
        */}
        {shown.length > 0 ? (
          <ul className="list">
            {shown.map((row) => (
              <LocalModelRow
                key={row.id}
                row={row}
                progress={installingId === row.id ? installing : null}
                onUse={() => chooseModel(row)}
                onInstall={() => installModel(row)}
              />
            ))}
          </ul>
        ) : language !== null ? (
          <p className="muted">{localModelRows(local, sherpa, route).length === 0 ? "No local speech models are available on this platform." : `No available model here is listed for ${modelLanguageLabel([language])}.`}</p>
        ) : null}
        <p className="muted">
          Dictation uses one model at a time, so choosing one for dictation
          stops it using the one before. Choosing a model for meetings doesn't
          change dictation.
        </p>
        {/* The catch in a big model, said once (KNEG-407): one Whisper
            model at a time serves dictation as well as meetings and
            imports, so choosing a slow one for long recordings slows
            dictation too. "Usually": believed from how Whisper sizes
            behave, not measured here. */}
        {local.availability.state !== "not-published" ? (
          <p className="muted">
            Only one Whisper model runs at a time. Meetings and imports
            transcribed on this computer use it, and so does dictation when
            it uses Whisper, so a bigger one is usually more accurate and
            slower for all of them.
          </p>
        ) : null}

        <h3>Engines</h3>
        {local.availability.state === "not-published" ? (
          <>
            <Row label="Speech engine" value="Not available on this computer yet" />
            <LearnMore summary="Why">
              <p>{local.availability.reason}</p>
              <p>{local.availability.tracking}</p>
            </LearnMore>
          </>
        ) : (
          <>
            <Row
              label="Speech engine"
              value={local.serverInstalled ? "Installed" : "Not installed"}
              detail={local.port ? "running" : undefined}
            />
            {/*
              Which build is installed, said out loud. "Installed" was the only
              thing this screen ever claimed about the server binary, and it
              stayed true while the binary fell two releases behind the manifest
              the application ships -- the state this machine was in when
              KNEG-113 was written. The three answers are kept apart: `unknown`
              is not a polite `stale`, and the button below is offered for both
              because pressing it is how either one is settled.
            */}
            {serverFreshness ? (
              <Freshness line={serverFreshness} current={local.serverFreshness.state === "current"} />
            ) : null}
            {local.serverFreshness.state !== "not-installed" ? (
              <button
                type="button"
                onClick={() => {
                  runInstall(SERVER_ROW, async () => {
                    const outcome = await refreshLocalServer();
                    setRefreshed(refreshOutcomeLabel(outcome));
                  });
                }}
              >
                {local.serverFreshness.state === "current"
                  ? "Reinstall the speech engine"
                  : "Update the speech engine"}
              </button>
            ) : null}
            {installingId === SERVER_ROW && installing ? <Row label="Progress" value={installing} /> : null}
            {/*
              Always rendered, so a press fills a slot rather than growing the
              section under the pointer -- the same reason the providers block
              above holds its status line open.
            */}
            <p
              className={refreshed?.problem ? "status-slot row-detail warn" : "status-slot row-detail"}
              role="status"
            >
              {refreshed?.text}
            </p>
            {refreshed?.more ? (
              <LearnMore summary="What was replaced">
                <p>{refreshed.more}</p>
              </LearnMore>
            ) : null}
            <LearnMore summary="Technical details">
              <p>
                The speech engine is whisper.cpp's server
                {local.port ? `, running on 127.0.0.1:${local.port}` : ""}. It
                is installed in <code>{local.installDir}</code>.
              </p>
              {local.installDirFellBack ? (
                <p>
                  The per-user location contained a space or a non-ASCII
                  character, which the native binaries cannot load from, so a
                  machine-wide location is used instead.
                </p>
              ) : null}
            </LearnMore>
          </>
        )}
        {sherpa.availability.state === "not-published" ? (
          <>
            <Row label="Parakeet engine" value="Not available on this computer yet" />
            <LearnMore summary="Why">
              <p>{sherpa.availability.reason}</p>
              <p>{sherpa.availability.tracking}</p>
            </LearnMore>
          </>
        ) : (
          <>
            <Row
              label="Parakeet engine"
              value={sherpa.cliInstalled ? "Installed" : "Not installed"}
              detail={
                sherpa.cliInstalled
                  ? undefined
                  : "installed with the first model"
              }
            />
            {/*
              Which build is installed, said out loud — and, unlike the
              speech engine above, with no button beside it. That is a
              deliberate limit and the sentence says so rather than leaving
              a reader hunting for a control that is not there.

              The reason is this runtime's lifecycle, not the archive.
              `sherpa_stream` starts its meeting server out of this same
              directory and nothing ever stops it, so a running server means
              "a meeting happened this session", not "a meeting is happening
              now". A refresh replaces the directory, which on Windows fails
              against a running .exe — and an unconditional stop would end
              the live transcript of a meeting in progress, from a dictation
              screen that never mentions meetings. Reporting the staleness
              honestly beats a button that is wrong at the worst moment.
            */}
            {sherpaFreshness ? (
              <Freshness line={sherpaFreshness} current={sherpa.cliFreshness.state === "current"} />
            ) : null}
            {sherpa.cliFreshness.state !== "not-installed" &&
            sherpa.cliFreshness.state !== "current" ? (
              <>
                <p className="caveat">
                  This engine can't be updated from here yet. To reinstall
                  it, close the app and delete its folder by hand.
                </p>
                {/*
                  The engine is in the sidecar folder, `local.installDir`
                  (`local_asr.rs`, `cli_root`), not with the models. The
                  branch review caught the fold naming the models folder,
                  where deleting removes the models and keeps the engine.
                */}
                <LearnMore summary="Why, and which folder">
                  <p>
                    One archive serves three things — dictation with a
                    Parakeet model, the live transcript in meetings, and
                    speaker separation — so replacing it affects all three,
                    and the meeting server runs out of its folder for as long
                    as the app is open.
                  </p>
                  <p>
                    The engine is the <code>sherpa</code> folder inside{" "}
                    <code>{local.installDir}</code>. The models are kept
                    separately, in <code>{sherpa.modelsDir}</code>, and
                    don't need deleting.
                  </p>
                </LearnMore>
              </>
            ) : (
              <LearnMore summary="Technical details">
                <p>
                  Models are kept in <code>{sherpa.modelsDir}</code>.
                </p>
              </LearnMore>
            )}
          </>
        )}
        <LearnMore summary="About the two engines">
          <p>
            Whisper models run on whisper.cpp; Parakeet and Nemotron models run
            on sherpa-onnx. Choosing one for dictation releases the other.
          </p>
        </LearnMore>
        {local.port ? (
          <button
            type="button"
            onClick={() => {
              void guard(async () => {
                // The stop first: it is refused while something is
                // using the server (KNEG-414), and a refusal must mean
                // nothing changed. In the other order the route had
                // already moved to a provider beside a "Not now".
                await stopLocalServer();
                await useLocalModel(false);
              });
            }}
          >
            Stop and use a provider instead
          </button>
        ) : null}
      </section>
    </div>
  );
}
