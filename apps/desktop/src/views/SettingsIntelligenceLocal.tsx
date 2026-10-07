/**
 * Settings > Intelligence > Local — the llama.cpp server.
 *
 * The screen has one job the cloud screens do not: **make the cost visible
 * before anyone commits to it.** The recommended model is 5.9 GB. A row that
 * says only "Install" is a row that starts a twenty-minute download from a
 * single click, and the current whisper install path — 148 MB, one button, no
 * size — got away with that only because of the scale.
 *
 * So every model shows its size before the button, progress shows bytes while
 * it runs, and the server's five-minute idle stop is stated rather than being a
 * surprise when a chat is slow after lunch.
 *
 * The same rule now covers the other thing that is easy to leave implied: **how
 * the bytes will be checked.** Every row says so before the button, and *checksum
 * verified* and *checksum verified against the publisher's own* are different
 * sentences because they are different guarantees. See `LOCAL_LLM_PINS` in
 * `tools/integration-audit/generate-provider-registry.mjs`. Since KNEG-343 a
 * row's Details says which, and its line says "verified" for both.
 *
 * And it covers the third thing, which is *when*. A row used to say "Installed"
 * and mean only that a path existed — so a model corrupted months after a clean
 * install looked exactly like one checked five minutes ago. `ggufIntegrityLabel`
 * now says which of those it is, with a fourth answer for the models installed
 * before any of this existed: nothing is recorded about them, and that is
 * rendered as nothing rather than as either good news or bad. The **Check the
 * file** button is the only thing on this screen that actually hashes a file,
 * and it says how long that will take before it starts. KNEG-107.
 *
 * # In plain words (KNEG-343)
 *
 * What a person needs is in view: the size, whether it is installed, a finding
 * that asks them to do something, and that nothing leaves this computer. Model
 * ids, the quantisation, the context length, checksums, the port and the
 * folder are under each row's Details or the page's Technical details. The
 * server is "the text engine".
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import {
  freshnessLabel,
  ggufIntegrityLabel,
  llmProgressLabel,
  recheckOutcomeLabel,
  refreshOutcomeLabel,
  size,
  verificationLabel,
  verifiedPlain,
  type Plain,
} from "../lib/labels";
import {
  getLocalLlmStatus,
  installLocalLlm,
  onLocalLlmProgress,
  recheckLocalLlmModel,
  refreshLocalLlmServer,
  startLocalLlm,
  stopLocalLlm,
  type LocalLlmModelStatus,
} from "../lib/ipc";
import { message, useRefresh } from "../lib/useRefresh";

export function SettingsIntelligenceLocal() {
  const load = useCallback(() => getLocalLlmStatus(), []);
  const { data, error, reload, guard } = useRefresh(load);
  const [progress, setProgress] = useState<string | null>(null);
  /**
   * What the last re-hash found, and the caveat that came with it.
   *
   * Kept beside the row rather than folded into `progress`, because a mismatch
   * is a finding somebody has to read and decide about — putting it in the line
   * that install progress overwrites would let the most important sentence on
   * this screen be replaced by "Downloading…".
   */
  const [checked, setChecked] = useState<{
    id: string;
    text: string;
    detail: string;
    caveat?: string | undefined;
    failed?: boolean;
  } | null>(null);
  /** Which model is being hashed, so its button can say so and not restart. */
  const [checking, setChecking] = useState<string | null>(null);
  /** What the last press of the text-engine button did. */
  const [refreshed, setRefreshed] = useState<(Plain & { problem: boolean }) | null>(null);

  // The progress event carries a model's id; the line names it, from the list
  // as it was last read. A ref, so a reload does not resubscribe the listener.
  const latest = useRef(data);
  latest.current = data;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const nameOf = (id: string) => latest.current?.models.find((m) => m.id === id)?.name ?? id;
    void onLocalLlmProgress((p) => {
      setProgress(llmProgressLabel(p, nameOf));
      if (p.stage === "done" || p.stage === "stopped-when-idle") void reload();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [reload]);

  if (!data) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const running = data.port !== null;
  const runningName = data.models.find((m) => m.id === data.runningModel)?.name ?? data.runningModel;
  const freshness = freshnessLabel(data.serverFreshness);

  /**
   * Hash one model's files, on request.
   *
   * Not routed through `guard`, which is for mutations whose only interesting
   * outcome is "it worked or it threw". A re-check has a third outcome that is
   * neither — the file does not match — and that one has to reach the screen as
   * a finding rather than as a red error line.
   */
  const recheck = async (id: string) => {
    setChecking(id);
    setChecked(null);
    try {
      const outcome = await recheckLocalLlmModel(id);
      setChecked({ id, ...recheckOutcomeLabel(outcome), failed: outcome.outcome === "mismatched" });
      // The record beside the file has just been rewritten or removed, so the
      // row's own state is now stale.
      await reload();
    } catch (e) {
      setChecked({
        id,
        text: "Couldn't check the file.",
        detail: message(e),
        failed: true,
      });
    } finally {
      setChecking(null);
    }
  };

  const modelRow = (m: LocalLlmModelStatus) => {
    const installed = m.integrity.state !== "not-installed";
    const state = ggufIntegrityLabel(m.integrity);
    const draftState = m.draft ? ggufIntegrityLabel(m.draft.integrity) : null;
    // The two states that ask the reader to do something are warnings.
    const asks = m.integrity.state === "changed" || m.integrity.state === "failed-recheck";

    return (
      <li key={m.id}>
        {m.name}
        <em className="row-detail">
          {size(m.sizeBytes)}
          {m.recommended ? " · recommended" : ""}
          {/* Before the button, for the same reason the size is: a guarantee
              nobody was told about is a guarantee nobody can weigh. */}
          {installed ? "" : ` · will be ${verifiedPlain(m.verifiedBy)}`}
        </em>
        {installed ? (
          <em className={asks ? "row-detail warn" : "row-detail"}>{state.text}</em>
        ) : null}
        <LearnMore summary="Details">
          <p>
            Model id <code>{m.id}</code>, from the {m.provider} family
            {m.quantization ? `, quantised as ${m.quantization}` : ""}
            {m.contextLength ? `, with a ${(m.contextLength / 1024).toFixed(0)}k context` : ""}
            {m.supportsThinking ? ", and it can reason step by step before it answers" : ""}.
          </p>
          <p>How the download is checked: {verificationLabel(m.verifiedBy)}.</p>
          {installed ? <p>{state.detail}.</p> : null}
          {installed && state.caveat ? <p>{state.caveat}</p> : null}
          {m.draft ? (
            <p>
              It comes with a smaller helper model that makes replies faster,{" "}
              {size(m.draft.sizeBytes)}
              {m.draft.integrity.state === "not-installed"
                ? ", downloaded with the model"
                : ` (${draftState?.text.toLowerCase() ?? ""})`}
              {m.draft.verifiedBy === m.verifiedBy
                ? "."
                : `; ${verificationLabel(m.draft.verifiedBy)}.`}
            </p>
          ) : null}
        </LearnMore>
        {installed ? (
          <>
            <button
              type="button"
              onClick={() => {
                void guard(() => startLocalLlm(m.id));
              }}
            >
              {data.runningModel === m.id ? "Restart" : "Use this model"}
            </button>
            <button
              type="button"
              disabled={checking !== null}
              onClick={() => {
                void recheck(m.id);
              }}
            >
              {/* The cost is in the label, exactly as the download size is: this
                  reads the whole file back, and a button that looks free but
                  takes a minute on a 20 GB model reads as a hang. */}
              {checking === m.id
                ? "Reading the whole file…"
                : `Check the file (reads ${size(m.sizeBytes)})`}
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => {
              // Said before it starts, because the number is the decision.
              setProgress(`Starting a ${size(m.sizeBytes)} download…`);
              void guard(() => installLocalLlm(m.id));
            }}
          >
            Install ({size(m.sizeBytes)})
          </button>
        )}
        {checked?.id === m.id ? (
          <>
            <em className={checked.failed ? "row-detail warn" : "row-detail"} role="status">
              {checked.text}
            </em>
            <LearnMore summary="What the check found">
              <p>{checked.detail}</p>
              {checked.caveat ? <p>{checked.caveat}</p> : null}
            </LearnMore>
          </>
        ) : null}
      </li>
    );
  };

  return (
    <div className="view">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      <section data-surface="settings.intelligence.local">
        <h2>On this computer</h2>

        {data.availability.state === "not-published" ? (
          <>
            <Row
              label="Availability"
              value="Not available on this computer yet"
            />
            <LearnMore summary="Why">
              <p>{data.availability.reason}</p>
              <p>{data.availability.tracking}</p>
            </LearnMore>
          </>
        ) : (
          <>
            <p className="muted">
              A model that runs on this computer. Nothing you type or dictate
              leaves this computer, and no key is needed. These models are large,
              several gigabytes each, and on a computer without a graphics card
              the largest are slower than a cloud service.
            </p>

            <Row
              label="Text engine"
              value={
                running
                  ? "Running"
                  : data.serverInstalled
                    ? "Installed, not running"
                    : "Not installed"
              }
              detail={
                runningName
                  ? `using ${runningName}`
                  : running
                    ? undefined
                    : "installed with the first model"
              }
            />
            {/*
              Which build is installed, said out loud. "Installed" was the only
              thing this screen ever claimed about the server binary, and it
              stays true while the binary falls releases behind the manifest
              this application ships. `install` has written the record since
              KNEG-113 and nothing here read it. The three answers are kept
              apart — `unknown` is not a polite `stale` — and the button is
              offered for both, because pressing it is how either is settled.
            */}
            {freshness ? (
              <>
                <p className={data.serverFreshness.state === "current" ? "muted" : "muted warn"}>
                  {freshness.text}
                </p>
                {freshness.more ? (
                  <LearnMore summary="Which version">
                    <p>{freshness.more}</p>
                  </LearnMore>
                ) : null}
              </>
            ) : null}
            {data.serverFreshness.state !== "not-installed" ? (
              <>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      // Said before it starts: this stops the server, and a
                      // loaded model is several gigabytes to reload.
                      setProgress("Replacing the text engine…");
                      const outcome = await refreshLocalLlmServer();
                      setRefreshed(refreshOutcomeLabel(outcome));
                      setProgress(null);
                    });
                  }}
                >
                  {data.serverFreshness.state === "current"
                    ? "Reinstall the text engine"
                    : "Update the text engine"}
                </button>
                {running ? (
                  <p className="caveat">
                    The model in use stops while the engine is replaced, then
                    loads again, which takes a while.
                  </p>
                ) : null}
              </>
            ) : null}
            {/*
              Always rendered, so a press fills a slot rather than growing the
              section under the pointer.
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
            <Row
              label="When unused"
              value={`Unloaded after ${data.idleStopMinutes} minutes`}
              detail="to free the memory a model holds; the next request loads it again"
            />
            <LearnMore summary="Technical details">
              <p>
                The text engine is llama.cpp's server
                {running ? `, running on 127.0.0.1:${data.port}` : ""}. Models are
                kept in <code>{data.installDir}</code>.
              </p>
              <p>
                These are the builds that run on the processor. A build for a
                graphics card (CUDA, Vulkan or ROCm) is fifteen to twenty-five
                times the download, needs a matching driver, and fails at launch
                in a way that reads as a broken install rather than a missing
                runtime, so choosing one is left to whoever has the hardware to
                test it.
              </p>
            </LearnMore>

            {progress ? <Row label="Progress" value={progress} /> : null}

            {/* `list`, not `purposes`: a model row carries its own button. */}
            <ul className="list">{data.models.map(modelRow)}</ul>

            {running ? (
              <button
                type="button"
                onClick={() => {
                  void guard(stopLocalLlm);
                }}
              >
                Stop the model
              </button>
            ) : null}
          </>
        )}
      </section>
    </div>
  );
}
