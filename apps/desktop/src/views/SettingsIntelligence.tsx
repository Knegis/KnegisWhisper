/**
 * Settings > Intelligence — the reasoning model that tidies a dictation.
 *
 * `crates/reasoning-core` has routed, shaped and sent chat requests since phase
 * 3, and none of it was configurable: `DictationRuntime::build` was handed
 * `None` for cleanup with a comment saying a half-configured cleanup is worse
 * than none. This screen is what makes it a `Some`.
 *
 * What it refuses to do:
 *
 * - **Guess a model.** Seven providers disagree about `max_tokens` versus
 *   `max_completion_tokens`, and several reasoning models reject `temperature`
 *   rather than ignoring it. Rust refuses a configuration with no model instead
 *   of picking one.
 * - **Hide the ten providers that cannot be used.** Three enterprise connectors
 *   need a request shape nobody has written and the local route needs a sidecar
 *   that is not in this build. They are listed, with the reason, rather than
 *   quietly missing.
 * - **Say cleanup is on when it will do nothing.** A provider with no stored key
 *   resolves fine and then leaves the transcript alone, so the caveat says so.
 */

import { useCallback, useState } from "react";

import { Row } from "../components/Row";
import {
  clearEnterpriseConfig,
  getReasoningStatus,
  setCleanup,
  setEnterpriseConfig,
  type CleanupRoute,
  type EnterpriseConfig,
  type EnterpriseStatus,
  type ReasoningDispatch,
  type ReasoningProviderSummary,
} from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

/** Why a provider can or cannot be used, said plainly. */
function dispatchLabel(d: ReasoningDispatch): string {
  switch (d.state) {
    case "ready":
      return "can be used";
    case "needs-config":
      return d.reason;
    case "needs-sidecar":
      return d.reason;
    case "unresolvable":
      return `cannot be routed: ${d.reason}`;
  }
}

/** Where cleanup currently sends a transcript. */
function routeLabel(r: CleanupRoute): { value: string; detail: string } {
  switch (r.state) {
    case "off":
      return {
        value: "Off",
        detail: "dictations are inserted exactly as they were transcribed",
      };
    case "problem":
      return { value: "On, and not usable", detail: r.reason };
    case "ready":
      return {
        value: `${r.destination} · ${r.model}`,
        detail:
          r.caveat ??
          (r.leavesDevice
            ? "the transcript leaves this machine for cleanup"
            : "the transcript stays on this machine"),
      };
  }
}


/**
 * The fields each connector needs, declared once.
 *
 * `secret` decides the input type. Getting it wrong is not cosmetic: a secret
 * access key in a plain text field is a secret on screen in a screen share, and
 * this application ships an overlay whose whole purpose is not being in one.
 */
const ENTERPRISE_FIELDS: Record<
  string,
  { key: string; label: string; secret: boolean; hint?: string }[]
> = {
  bedrock: [
    { key: "region", label: "Region", secret: false, hint: "e.g. eu-west-1" },
    { key: "accessKeyId", label: "Access key ID", secret: false },
    { key: "secretAccessKey", label: "Secret access key", secret: true },
    {
      key: "sessionToken",
      label: "Session token",
      secret: true,
      hint: "only for temporary credentials",
    },
  ],
  azure: [
    {
      key: "endpoint",
      label: "Endpoint",
      secret: false,
      hint: "https://your-org.openai.azure.com",
    },
    {
      key: "deployment",
      label: "Deployment",
      secret: false,
      hint: "the deployment name selects the model",
    },
    { key: "apiVersion", label: "API version", secret: false, hint: "e.g. 2024-10-21" },
    { key: "apiKey", label: "API key", secret: true },
  ],
  vertex: [
    { key: "projectId", label: "Project ID", secret: false },
    { key: "region", label: "Region", secret: false, hint: "e.g. europe-west4, or global" },
    {
      key: "serviceAccountJson",
      label: "Service account JSON",
      secret: true,
      hint: "the key file, pasted whole",
    },
  ],
};

/**
 * One connector's form.
 *
 * Fields are never prefilled from what is stored — nothing can read a
 * configuration back — so a saved connector shows which fields it has and an
 * empty form that replaces the lot. Partial edits are not offered because a
 * partial write would leave a configuration half from one set of credentials
 * and half from another.
 */
function EnterpriseForm({
  status,
  onSave,
  onClear,
}: {
  status: EnterpriseStatus;
  onSave: (config: EnterpriseConfig) => void;
  onClear: () => void;
}) {
  const [values, setValues] = useState<Record<string, string>>({});
  const fields = ENTERPRISE_FIELDS[status.provider] ?? [];

  const save = () => {
    // The tag Rust discriminates on. Built here rather than inferred, so an
    // unknown provider is a missing key rather than a mis-tagged document.
    onSave({ provider: status.provider, ...values } as unknown as EnterpriseConfig);
    setValues({});
  };

  return (
    <div className="row">
      <span className="row-label">{status.name}</span>
      <span className="row-value">
        {status.configured ? "Configured" : "Not configured"}
        {status.problem ? (
          <em className="row-detail warn">{status.problem}</em>
        ) : status.configured ? (
          <em className="row-detail">
            {status.fields
              .map((f) => `${f.name}${f.filled ? "" : " (empty)"}`)
              .join(", ")}
          </em>
        ) : null}

        {fields.map((f) => (
          <span key={f.key} className="controls">
            <input
              type={f.secret ? "password" : "text"}
              autoComplete="off"
              aria-label={`${status.name} ${f.label}`}
              placeholder={f.hint ? `${f.label} — ${f.hint}` : f.label}
              value={values[f.key] ?? ""}
              onChange={(e) =>
                setValues((prev) => ({ ...prev, [f.key]: e.target.value }))
              }
            />
          </span>
        ))}

        <span className="controls">
          {/* Never disabled: a control that silently does nothing is the defect. */}
          <button type="button" onClick={save}>
            Save
          </button>
          {status.configured ? (
            <button type="button" onClick={onClear}>
              Clear
            </button>
          ) : null}
        </span>
      </span>
    </div>
  );
}

export function SettingsIntelligence({
  onGoToProviders,
}: {
  onGoToProviders: () => void;
}) {
  const load = useCallback(() => getReasoningStatus(), []);
  const { data, error, guard } = useRefresh(load);

  /** Draft edits, so nothing is saved until Apply. */
  const [draftProvider, setDraftProvider] = useState<string | null>(null);
  const [draftModel, setDraftModel] = useState<string | null>(null);
  const [draftUrl, setDraftUrl] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { settings, route, providers, enterprise } = data;

  // A draft field falls back to what is stored, so the controls always show the
  // real configuration rather than an empty box next to a working setting.
  const provider = draftProvider ?? settings.provider ?? "";
  const url = draftUrl ?? settings.selfHostedUrl ?? "";
  const selfHosted = url.trim() !== "";
  const chosen = providers.find((p) => p.id === provider);
  const model =
    draftModel ?? settings.model ?? (selfHosted ? "" : (chosen?.models[0]?.id ?? ""));

  const usable = providers.filter((p) => p.dispatch.state === "ready");

  const apply = (enabled: boolean) => {
    setSaid(null);
    void guard(async () => {
      const next = await setCleanup({
        enabled,
        provider: provider || null,
        model: model || null,
        selfHostedUrl: url.trim() || null,
        enterprise: chosen?.enterprise ?? false,
      });
      const label = routeLabel(next);
      setSaid(`${label.value} — ${label.detail}`);
      // Drafts drop so the controls go back to mirroring what is stored.
      setDraftProvider(null);
      setDraftModel(null);
      setDraftUrl(null);
    });
  };

  const providerRow = (p: ReasoningProviderSummary) => (
    <div key={p.id} className="row">
      <span className="row-label">{p.name}</span>
      <span className="row-value">
        {p.enterprise
          ? "Enterprise connector"
          : p.configured
            ? "Key stored"
            : "No key stored"}
        <em className="row-detail">
          {dispatchLabel(p.dispatch)} · {p.models.length} model
          {p.models.length === 1 ? "" : "s"}
          {p.credentialSlot ? (
            <>
              {" "}
              · slot <code>{p.credentialSlot}</code>
            </>
          ) : (
            " · multi-field credentials, so there is no single slot"
          )}
        </em>
      </span>
    </div>
  );

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="settings.intelligence">
        <h2>Dictation cleanup</h2>
        <p className="muted">
          A reasoning model can tidy a transcript — punctuation, filler words,
          proper nouns — before it is typed at the cursor. Off by default,
          because it sends what you said to whichever provider you choose, and
          because a raw transcript is already a correct answer.
        </p>

        <Row
          label="Currently"
          value={routeLabel(route).value}
          detail={routeLabel(route).detail}
        />

        <div className="row">
          <span className="row-label">Provider</span>
          <span className="row-value controls">
            <select
              aria-label="cleanup provider"
              value={provider}
              onChange={(e) => {
                setDraftProvider(e.target.value);
                // The previous provider's model does not exist on the new one.
                // Rust would refuse the pair; clearing it here means the user
                // sees the new provider's list rather than an error.
                setDraftModel(null);
              }}
            >
              <option value="">Choose a provider</option>
              {providers.map((p) => (
                <option
                  key={p.id}
                  value={p.id}
                  disabled={p.dispatch.state !== "ready"}
                >
                  {p.name}
                  {p.dispatch.state === "ready" ? "" : " — not usable yet"}
                </option>
              ))}
            </select>
          </span>
        </div>

        <div className="row">
          <span className="row-label">Model</span>
          <span className="row-value controls">
            {selfHosted ? (
              // A server the user runs has no model list to offer. Typed, and
              // required all the same: `params::shape_for` needs the real id.
              <input
                aria-label="cleanup model"
                placeholder="the model id your server serves"
                value={model}
                onChange={(e) => setDraftModel(e.target.value)}
              />
            ) : (
              <select
                aria-label="cleanup model"
                value={model}
                disabled={!chosen || chosen.models.length === 0}
                onChange={(e) => setDraftModel(e.target.value)}
              >
                {chosen?.models.length ? (
                  chosen.models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                      {m.supportsThinking ? " · thinking" : ""}
                    </option>
                  ))
                ) : (
                  <option value="">Choose a provider first</option>
                )}
              </select>
            )}
          </span>
        </div>

        <div className="row">
          <span className="row-label">Or a server you run</span>
          <span className="row-value controls">
            <input
              type="url"
              aria-label="self-hosted endpoint"
              placeholder="http://127.0.0.1:11434/v1"
              value={url}
              onChange={(e) => setDraftUrl(e.target.value)}
            />
          </span>
        </div>
        <p className="caveat">
          {selfHosted
            ? "This URL is in use, so the provider above is ignored — "
            : "A URL here wins over the provider above — "}
          it is a route you declared.
          Plaintext HTTP is refused to anything but a loopback or private
          address, so a key cannot be sent over the open internet in the clear.
        </p>

        <span className="controls">
          {/* Never disabled: a control that silently does nothing is the defect. */}
          <button type="button" onClick={() => apply(true)}>
            Turn cleanup on
          </button>
          <button type="button" onClick={() => apply(false)}>
            Turn it off
          </button>
        </span>
        {said ? <p className="muted">{said}</p> : null}
        {usable.length === 0 ? (
          <p className="caveat">
            No provider can be used yet. Store a key on the Providers screen
            first.
          </p>
        ) : null}
        <button type="button" onClick={onGoToProviders}>
          Open Providers
        </button>
      </section>

      <section data-surface="settings.intelligence.enterprise">
        <h2>Enterprise connectors</h2>
        <p className="muted">
          Bedrock, Azure OpenAI and Vertex AI use organisation credentials rather
          than a single API key, so they are configured here rather than on the
          Providers screen. The whole configuration goes into one entry in this
          machine's credential store — nothing enterprise-shaped is written to
          the database.
        </p>
        <p className="caveat">
          The request signing and body shapes are written and tested against
          published vectors and recorded fixtures. <strong>No live call has been
          made to any of the three</strong>, because that needs AWS, Azure or GCP
          organisation credentials this project does not have. See
          <code>docs/PLATFORM_SUPPORT.md</code>.
        </p>
        {enterprise.map((e) => (
          <EnterpriseForm
            key={e.provider}
            status={e}
            onSave={(config) => {
              void guard(() => setEnterpriseConfig(config));
            }}
            onClear={() => {
              void guard(() => clearEnterpriseConfig(e.provider));
            }}
          />
        ))}
      </section>

      <section>
        <h2>All providers</h2>
        <p className="muted">
          Every reasoning provider the registry declares, including the ones this
          build cannot send to. Listing them with the reason beats leaving them
          out and saying nothing.
        </p>
        {providers.map(providerRow)}
      </section>
    </div>
  );
}
