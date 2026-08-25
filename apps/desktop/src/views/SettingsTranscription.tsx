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
 */

import { useCallback, useEffect, useState } from "react";

import { Row } from "../components/Row";
import {
  getDictationRoute,
  getLocalStatus,
  getProviders,
  installLocalModel,
  onInstallProgress,
  setCloudProvider,
  startLocalServer,
  stopLocalServer,
  useLocalModel,
  type Dispatch,
  type ProviderSummary,
} from "../lib/ipc";
import { progressLabel, verificationLabel } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

/**
 * Whether dictation could actually use this provider, said plainly.
 *
 * `needs-client` is not a failure of configuration and must not read like one:
 * four of the seven providers use an API shape this build has no client for, and
 * the route refuses rather than sending the audio somewhere else. That refusal
 * is the correct behaviour, so the screen names it rather than hiding it behind
 * a greyed-out control with no explanation.
 */
function dispatchLabel(d: Dispatch): string {
  switch (d.state) {
    case "ready":
      return "can be used";
    case "needs-client":
      // Not always "unwritten". Tinfoil is refused on purpose, and the reason
      // that arrives says which case it is.
      return d.reason;
    case "unresolvable":
      // Usually a missing setting rather than a missing feature — Corti with no
      // environment or tenant is the case this exists for.
      return d.reason;
  }
}

export function SettingsTranscription({
  onGoToProviders,
}: {
  onGoToProviders: () => void;
}) {
  const load = useCallback(async () => {
    const [providers, local, route] = await Promise.all([
      getProviders(),
      getLocalStatus(),
      getDictationRoute(),
    ]);
    return { providers, local, route };
  }, []);

  const { data, error, reload, guard } = useRefresh(load);
  const [installing, setInstalling] = useState<string | null>(null);
  /** Per-provider model choice, before it is committed. */
  const [selected, setSelected] = useState<Record<string, string>>({});
  /** Per-provider outcome of the last press, so the control is never silent. */
  const [chosen, setChosen] = useState<Record<string, string>>({});
  /**
   * Corti's two scoping values.
   *
   * Only one provider needs them, so they are two fields rather than a generic
   * per-provider settings map — a map would be a shape inviting a second
   * provider's options into it, and no other provider has any.
   */
  const [cortiEnvironment, setCortiEnvironment] = useState<string | null>(null);
  const [cortiTenant, setCortiTenant] = useState<string | null>(null);

  const choose = (p: ProviderSummary) => {
    const model = selected[p.id] ?? p.models[0];
    if (!model) {
      setChosen((prev) => ({
        ...prev,
        [p.id]: "The registry lists no model for this provider.",
      }));
      return;
    }

    // Corti needs two more values, and Rust refuses the route without them.
    // Checked here so the message names the empty box rather than arriving as a
    // rejected command.
    if (p.id === "corti") {
      const environment = (cortiEnvironment ?? "").trim();
      const tenant = (cortiTenant ?? "").trim();
      if (!environment || !tenant) {
        setChosen((prev) => ({
          ...prev,
          [p.id]:
            "Corti needs both an environment and a tenant. Neither has a safe default.",
        }));
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
      setChosen((prev) => ({
        ...prev,
        [p.id]: p.configured
          ? `Dictation now goes to ${p.name} / ${model}.`
          : `Set to ${p.name} / ${model}. It will not work until a key is stored on the Providers screen.`,
      }));
    });
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onInstallProgress((progress) => {
      setInstalling(progressLabel(progress));
      if (progress.stage === "done") void reload();
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

  const { providers, local, route } = data;
  const withKeys = providers.filter((p) => p.configured);

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.transcription">
        <h2>Cloud providers</h2>
        <p className="muted">
          A stored key is not a working provider. Store keys and run a real
          connection test on the Providers screen.
        </p>
        <Row
          label="Dictation goes to"
          value={route.problem ? "Nowhere usable" : route.destination}
          detail={
            route.problem
              ? route.problem
              : route.leavesDevice
                ? "audio leaves this machine"
                : "audio stays on this machine"
          }
        />
        <Row
          label="Keys stored"
          value={
            withKeys.length === 0
              ? "None"
              : withKeys.map((p) => p.name).join(", ")
          }
          detail={
            withKeys.length === 0
              ? "no cloud provider can be used until one is stored"
              : undefined
          }
        />
        {providers.map((p) => (
          <div key={p.id} className="row">
            <span className="row-label">{p.name}</span>
            <span className="row-value">
              {p.configured ? "Key stored" : "No key stored"}
              <em className="row-detail">
                {dispatchLabel(p.dispatch)} · {p.modelCount} model
                {p.modelCount === 1 ? "" : "s"} · slot <code>{p.credentialSlot}</code>
              </em>
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
              {chosen[p.id] ? <em className="row-detail">{chosen[p.id]}</em> : null}
            </span>
          </div>
        ))}
        <button type="button" onClick={onGoToProviders}>
          Open Providers
        </button>
      </section>

      <section data-surface="settings.transcription.local">
        <h2>Local transcription</h2>
        {local.availability.state === "not-published" ? (
          <>
            <Row
              label="Availability"
              value="No server is published for this platform"
            />
            <p className="muted">{local.availability.reason}</p>
            <p className="muted">{local.availability.tracking}</p>
          </>
        ) : (
          <>
            <Row
              label="Server"
              value={local.serverInstalled ? "Installed" : "Not installed"}
              detail={
                local.port ? `running on 127.0.0.1:${local.port}` : undefined
              }
            />
            <Row
              label="Installed to"
              value={local.installDir}
              detail={
                local.installDirFellBack
                  ? "the per-user location contained a space or a non-ASCII character, which the native binaries cannot load from, so a machine-wide location is used instead"
                  : undefined
              }
            />
            {installing ? <Row label="Progress" value={installing} /> : null}
            <ul className="purposes">
              {local.models.map((m) => (
                <li key={m.id}>
                  <code>{m.id}</code> {m.name}
                  {m.sizeMb ? ` · ${m.sizeMb} MB` : ""}
                  {m.recommended ? " · recommended" : ""}
                  <em className="row-detail">
                    {m.installed
                      ? `installed, ${verificationLabel(m.verifiedBy)}`
                      : `not installed — will be ${verificationLabel(m.verifiedBy)}`}
                  </em>
                  {m.installed ? (
                    <button
                      type="button"
                      onClick={() => {
                        void guard(async () => {
                          await startLocalServer(m.id);
                          // Two steps under one label, and the label says both:
                          // a server that is up is not the same as a route
                          // pointing at it.
                          await useLocalModel(true);
                        });
                      }}
                    >
                      Use for dictation
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => {
                        setInstalling("Starting…");
                        void guard(() => installLocalModel(m.id));
                      }}
                    >
                      Install
                    </button>
                  )}
                </li>
              ))}
            </ul>
            <p className="muted">
              Only whisper models are offered here. Parakeet and Nemotron need
              the sherpa-onnx sidecar, which is not built yet.
            </p>
            {local.port ? (
              <button
                type="button"
                onClick={() => {
                  void guard(async () => {
                    await useLocalModel(false);
                    await stopLocalServer();
                  });
                }}
              >
                Stop and use a provider instead
              </button>
            ) : null}
          </>
        )}
      </section>
    </div>
  );
}
