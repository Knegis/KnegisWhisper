import { SummarySettingsPanel } from "../components/SummarySettingsPanel";
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
 *
 * # In plain words (KNEG-343)
 *
 * Where text goes, what to do, and that no live call has been made to a
 * company account stay in view; credential slots, field ids, Rust's reasons,
 * the plaintext-HTTP rule and the documentation path go under Learn more or a
 * row's Details (`cleanupRouteLine`, `reasoningDispatchLine` in
 * `lib/cleanup.ts`). The enterprise connectors are "Company accounts", as on
 * Settings › Providers.
 */

import { useCallback, useState } from "react";

import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { cleanupFields, cleanupRouteLine, effectiveCleanup, reasoningDispatchLine, savedLine } from "../lib/cleanup";
import {
  clearEnterpriseConfig,
  getReasoningStatus,
  setCleanup,
  setEnterpriseConfig,
  type EnterpriseConfig,
  type EnterpriseStatus,
  type ReasoningProviderSummary,
} from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

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
  // A saved field by the label its box has, not the key Rust stores it under.
  const label = (name: string) => fields.find((f) => f.key === name)?.label ?? name;

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
        {status.configured ? "Set up" : "Not set up"}
        {status.problem ? (
          <FailureLine
            text="The saved settings can't be used. Enter them again."
            detail={status.problem}
          />
        ) : status.configured ? (
          <em className="row-detail">
            Saved:{" "}
            {status.fields
              .map((f) => `${label(f.name)}${f.filled ? "" : " (empty)"}`)
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
  onGoToLocalModels,
  summaryFocus,
}: {
  onGoToProviders: () => void;
  onGoToLocalModels: () => void;
  summaryFocus?: number | undefined;
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
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { settings, route, providers, enterprise } = data;

  // A draft field falls back to what is stored, so the controls always show the
  // real configuration rather than an empty box next to a working setting.
  const drafts = { provider: draftProvider, model: draftModel, url: draftUrl };
  const { provider, url, selfHosted, chosen, model } = cleanupFields(settings, providers, drafts);

  const usable = providers.filter((p) => p.dispatch.state === "ready");
  // The model as the model menu names it, not its id.
  const modelName = (id: string) =>
    providers.flatMap((p) => p.models).find((m) => m.id === id)?.name ?? id;
  const current = cleanupRouteLine(route, modelName);

  const apply = (enabled: boolean) => {
    setSaid(null);
    void guard(async () => {
      // The same computation Dictation's Tidy up toggle sends (`lib/cleanup.ts`);
      // the write stays here, where the settings audit looks for it.
      const next = await setCleanup(effectiveCleanup(settings, providers, enabled, drafts));
      setSaid(savedLine(next, modelName));
      // Drafts drop so the controls go back to mirroring what is stored.
      setDraftProvider(null);
      setDraftModel(null);
      setDraftUrl(null);
    });
  };

  const providerRow = (p: ReasoningProviderSummary) => {
    const dispatch = reasoningDispatchLine(p.dispatch);
    const local = p.id === "local";
    return (
      <div key={p.id} className="row">
        <span className="row-label">{p.name}</span>
        <span className="row-value">
          {/* The local route has no key: `configured` is whether a model is
              running. It read "Key saved" / "No key saved" (branch review). */}
          {local
            ? p.configured
              ? "A model is running"
              : "No model is running"
            : p.enterprise
              ? "Company account"
              : p.configured
                ? "Key saved"
                : "No key saved"}
          <em className="row-detail">
            {dispatch.text} · {p.models.length} model
            {p.models.length === 1 ? "" : "s"}
          </em>
          <LearnMore summary="Details">
            {dispatch.more ? <p>{dispatch.more}</p> : null}
            <p>
              {local ? (
                "It runs on this computer and needs no key. Start a model under Local text models."
              ) : p.credentialSlot ? (
                <>
                  Its key is in the credential slot <code>{p.credentialSlot}</code>.
                </>
              ) : (
                "It needs several settings rather than one key, so it is set up under Company accounts."
              )}
            </p>
          </LearnMore>
        </span>
      </div>
    );
  };

  return (
    <div className="view">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      <SummarySettingsPanel focusRequest={summaryFocus} onGoToProviders={onGoToProviders} onGoToLocalModels={onGoToLocalModels} />
      <section data-surface="settings.intelligence">
        <h2>Dictation cleanup</h2>
        <p className="muted">
          An AI model can tidy a dictation — punctuation, filler words, names —
          before it is typed. It is off by default: with a cloud provider, what
          you said leaves this computer.
        </p>
        <LearnMore>
          <p>
            A raw transcript is already a correct answer, which is the other
            reason cleanup is off until you turn it on. Choose a model as well as
            a provider: the app refuses a setting with no model rather than
            guessing one, because providers differ in what their models accept.
          </p>
        </LearnMore>

        <Row label="Currently" value={current.value} detail={current.detail} />
        {current.more ? (
          <LearnMore summary="Details">
            <p>{current.more}</p>
          </LearnMore>
        ) : null}

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
                placeholder="the model name your server uses"
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
                      {m.supportsThinking ? " (reasons before answering)" : ""}
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
            ? "This address is in use, so the provider above is ignored."
            : "An address here is used instead of the provider above."}
        </p>
        <LearnMore summary="About your own server">
          <p>
            Plain <code>http://</code> is only allowed to this computer or your
            own network, so a key is never sent over the internet unencrypted.
            Requests to your server go without a key unless you save one under
            Providers › Your own server. Summaries use the same key.
          </p>
        </LearnMore>

        <span className="controls">
          {/* Never disabled: a control that silently does nothing is the defect. */}
          <button type="button" onClick={() => apply(true)}>
            Turn cleanup on
          </button>
          <button type="button" onClick={() => apply(false)}>
            Turn it off
          </button>
        </span>
        {said ? (
          <p className="muted" role="status">
            {said}
          </p>
        ) : null}
        {usable.length === 0 ? (
          <p className="caveat">
            No provider can be used yet. Add a key on the Providers page first.
          </p>
        ) : null}
        <button type="button" onClick={onGoToProviders}>
          Open Providers
        </button>
      </section>

      <section data-surface="settings.intelligence.enterprise">
        <h2>Company accounts</h2>
        <p className="muted">
          AWS Bedrock, Azure OpenAI and Vertex AI use your organisation's
          settings rather than one key, so they are set up here. They are saved
          on this computer, in its secure credential storage.
        </p>
        {/*
          About this project's testing, not your use: once one is set up and
          chosen, transcripts are sent to it. The first wording read as a claim
          about the app ("no request has been sent") -- caught in review.
        */}
        <p className="caveat">
          <strong>Not yet tried against a real account.</strong> The requests
          are built and tested, but the people who make this app have not been
          able to run one against any of the three.
        </p>
        <LearnMore>
          <p>
            Each configuration is one entry in this computer's credential store;
            nothing of it is written to the app's database. The request signing
            and body shapes are written and tested against published examples and
            recorded responses. A live call needs AWS, Azure or GCP organisation
            credentials this project does not have. See{" "}
            <code>docs/PLATFORM_SUPPORT.md</code>.
          </p>
        </LearnMore>
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
          Every provider for cleanup and summaries, including the ones this
          version can't use yet, with the reason.
        </p>
        {providers.map(providerRow)}
      </section>
    </div>
  );
}
