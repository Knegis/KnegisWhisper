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
 * `tools/integration-audit/generate-provider-registry.mjs`.
 *
 * And it covers the third thing, which is *when*. A row used to say "Installed"
 * and mean only that a path existed — so a model corrupted months after a clean
 * install looked exactly like one checked five minutes ago. `ggufIntegrityLabel`
 * now says which of those it is, with a fourth answer for the models installed
 * before any of this existed: nothing is recorded about them, and that is
 * rendered as nothing rather than as either good news or bad. The **Check the
 * bytes** button is the only thing on this screen that actually hashes a file,
 * and it says how long that will take before it starts. KNEG-107.
 */

import { useCallback, useEffect, useState } from "react";

import { Row } from "../components/Row";
import {
  ggufIntegrityLabel,
  recheckOutcomeLabel,
  verificationLabel,
} from "../lib/labels";
import {
  getLocalLlmStatus,
  installLocalLlm,
  onLocalLlmProgress,
  recheckLocalLlmModel,
  startLocalLlm,
  stopLocalLlm,
  type LocalLlmModelStatus,
  type LocalLlmProgress,
} from "../lib/ipc";
import { message, useRefresh } from "../lib/useRefresh";

/** Bytes, at a scale a person reads. */
function size(bytes: number | null): string {
  if (bytes === null) return "size unknown";
  const gb = bytes / 1_000_000_000;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(bytes / 1_000_000).toFixed(0)} MB`;
}

/** Progress as one line, with bytes rather than a spinner. */
function progressLabel(p: LocalLlmProgress): string {
  const of = (written: number, total: number | null) =>
    total === null ? size(written) : `${size(written)} of ${size(total)}`;

  switch (p.stage) {
    case "server":
      return `Downloading the server — ${of(p.written, p.total)}`;
    case "model":
      return `Downloading ${p.id} — ${of(p.written, p.total)}`;
    case "draft":
      // Named apart from the model so a second bar does not read as the first
      // one restarting.
      return `Downloading the drafter for ${p.id} — ${of(p.written, p.total)}`;
    case "extracting":
      return "Unpacking the server…";
    case "verifying":
      // Named because it is not instant: the check reads the whole file back,
      // and the largest model here is 20 GB.
      return `Checking ${p.id} against its checksum…`;
    case "done":
      // `null` means the files were already on disk, so this install checked
      // nothing and must not borrow the previous one's claim.
      return p.verifiedBy === null
        ? `${p.id} was already installed.`
        : `${p.id} is installed — ${verificationLabel(p.verifiedBy)}.`;
    case "stopped-when-idle":
      return "The server stopped itself after being idle. The next request will reload it.";
  }
}

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
  } | null>(null);
  /** Which model is being hashed, so its button can say so and not restart. */
  const [checking, setChecking] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onLocalLlmProgress((p) => {
      setProgress(progressLabel(p));
      if (p.stage === "done" || p.stage === "stopped-when-idle") void reload();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [reload]);

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const running = data.port !== null;

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
      setChecked({ id, ...recheckOutcomeLabel(outcome) });
      // The record beside the file has just been rewritten or removed, so the
      // row's own state is now stale.
      await reload();
    } catch (e) {
      setChecked({
        id,
        text: "The check could not be made",
        detail: message(e),
      });
    } finally {
      setChecking(null);
    }
  };

  const modelRow = (m: LocalLlmModelStatus) => {
    const installed = m.integrity.state !== "not-installed";
    const state = ggufIntegrityLabel(m.integrity);
    const draftState = m.draft ? ggufIntegrityLabel(m.draft.integrity) : null;

    return (
      <li key={m.id}>
        <code>{m.id}</code> {m.name}
        <em className="row-detail">
          {m.provider} · {size(m.sizeBytes)}
          {m.quantization ? ` · ${m.quantization}` : ""}
          {m.contextLength ? ` · ${(m.contextLength / 1024).toFixed(0)}k context` : ""}
          {m.supportsThinking ? " · thinking" : ""}
          {m.recommended ? " · recommended" : ""}
        </em>
        <em className="row-detail">
          {/* Before the button, for the same reason the size is: a guarantee
              nobody was told about is a guarantee nobody can weigh. */}
          {verificationLabel(m.verifiedBy)}
        </em>
        {installed ? (
          <em className="row-detail">
            {state.text} — {state.detail}
          </em>
        ) : null}
        {m.draft ? (
          <em className="row-detail">
            has a speculative-decoding drafter, {size(m.draft.sizeBytes)}
            {m.draft.integrity.state === "not-installed"
              ? " — downloaded with the model"
              : ` (${draftState?.text.toLowerCase() ?? ""})`}
            {m.draft.verifiedBy === m.verifiedBy
              ? ""
              : ` — ${verificationLabel(m.draft.verifiedBy)}`}
          </em>
        ) : null}
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
                : `Check the bytes (reads ${size(m.sizeBytes)})`}
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
            <em className="row-detail">
              {checked.text} — {checked.detail}
            </em>
            {checked.caveat ? <p className="caveat">{checked.caveat}</p> : null}
          </>
        ) : null}
        {installed && state.caveat ? (
          <p className="caveat">{state.caveat}</p>
        ) : null}
      </li>
    );
  };

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.intelligence.local">
        <h2>Local text generation</h2>

        {data.availability.state === "not-published" ? (
          <>
            <Row
              label="Availability"
              value="No server is published for this platform"
            />
            <p className="muted">{data.availability.reason}</p>
            <p className="muted">{data.availability.tracking}</p>
          </>
        ) : (
          <>
            <p className="muted">
              A model that runs on this machine. Nothing you type or dictate
              leaves the device on this route, and no key is needed. The trade is
              size: these are gigabytes, and the largest are slower than a cloud
              provider on a laptop without a GPU.
            </p>

            <Row
              label="Server"
              value={
                running
                  ? `Running on 127.0.0.1:${data.port}`
                  : data.serverInstalled
                    ? "Installed, not running"
                    : "Not installed"
              }
              detail={
                data.runningModel
                  ? `loaded ${data.runningModel}`
                  : running
                    ? undefined
                    : "installing a model installs the server with it"
              }
            />
            <Row label="Models are kept in" value={data.installDir} />
            <Row
              label="Idle shutdown"
              value={`After ${data.idleStopMinutes} minutes`}
              detail="a loaded model holds several gigabytes of memory, so it is released when nothing has used it; the next request reloads it"
            />

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
                Stop the server
              </button>
            ) : null}

            <p className="caveat">
              These are the CPU builds of llama.cpp. A CUDA, Vulkan or ROCm build
              is fifteen to twenty-five times the download, needs a matching
              driver, and fails at launch in a way that reads as a broken install
              rather than a missing runtime — so choosing one is left to whoever
              has the hardware to test it.
            </p>
          </>
        )}
      </section>
    </div>
  );
}
