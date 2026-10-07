/**
 * What to send `reasoning_set_cleanup`, worked out from what is stored.
 *
 * Settings > Cleanup & summaries and Dictation's Tidy up chip both write the
 * cleanup setting, and they must send the same thing for the same state, so the
 * computation lives here. Each still calls `setCleanup` itself: the settings
 * audit reads a page's own file for the writer of the keys it owns.
 *
 * Pure -- no React, no Tauri -- so it is tested (`tests/cleanup.test.ts`).
 */

import type { CleanupRoute, CleanupSettings, ReasoningDispatch, ReasoningProviderSummary } from "../generated/ipc-types.js";
import { destinationName } from "./controlBar.js";

/**
 * Values being edited and not yet saved, on Settings > Cleanup & summaries.
 * `null` or absent is "not edited", so the stored value shows through; an
 * empty string is an edit (the box was cleared).
 */
export type CleanupDrafts = {
  provider?: string | null;
  model?: string | null;
  url?: string | null;
};

/**
 * Where a provider or URL is stored, which is what makes Tidy up a switch
 * rather than a setup still to do. A blank URL is none.
 */
export function cleanupConfigured(settings: CleanupSettings): boolean {
  return (
    (settings.provider ?? "").trim() !== "" || (settings.selfHostedUrl ?? "").trim() !== ""
  );
}

/**
 * The fields as the settings page shows them: a draft falls back to what is
 * stored, so the controls show the real configuration rather than an empty box
 * next to a working setting. With no stored model, a provider's first model
 * stands in -- unless a URL is set, whose endpoint chooses its own.
 */
export function cleanupFields(
  settings: CleanupSettings,
  providers: readonly ReasoningProviderSummary[],
  drafts: CleanupDrafts = {},
) {
  const provider = drafts.provider ?? settings.provider ?? "";
  const url = drafts.url ?? settings.selfHostedUrl ?? "";
  const selfHosted = url.trim() !== "";
  const chosen = providers.find((p) => p.id === provider);
  const model =
    drafts.model ?? settings.model ?? (selfHosted ? "" : (chosen?.models[0]?.id ?? ""));
  return { provider, url, selfHosted, chosen, model };
}

/**
 * The whole setting to store, with `enabled` as asked: what the settings page
 * sends on Turn on and Turn off, and what Tidy up's toggle sends from what is
 * already stored. Rust resolves it before storing and refuses one that cannot
 * resolve, so a rejection means nothing was saved.
 */
export function effectiveCleanup(
  settings: CleanupSettings,
  providers: readonly ReasoningProviderSummary[],
  enabled: boolean,
  drafts: CleanupDrafts = {},
): CleanupSettings {
  const { provider, url, chosen, model } = cleanupFields(settings, providers, drafts);
  return {
    enabled,
    provider: provider || null,
    model: model || null,
    selfHostedUrl: url.trim() || null,
    enterprise: chosen?.enterprise ?? false,
  };
}

/* ------------------------------------------------- in plain words (KNEG-343) */

/** The three connectors set up under Company accounts, by route id. */
const COMPANY_ACCOUNTS = new Set(["bedrock", "azure", "vertex"]);

const UNTIL_FIXED = "until then dictations are typed as transcribed";

/**
 * Where cleanup sends a dictation, as Settings > Cleanup & summaries says it.
 *
 * Rust's `describe` (`src-tauri/src/reasoning.rs`) gives a destination phrase
 * -- a provider id, `local model`, or `self-hosted at <url>` -- and, for a
 * route that resolves but cannot run, a caveat naming the credential slot.
 * Those are the mechanism, so they go to `more`; `value` names the place,
 * `detail` says whether the text leaves this computer or, when there is a
 * caveat, what to do. The caveat's fix is read from the kind of route, not
 * parsed out of Rust's sentence: no key for a provider, no model running for
 * the local route, no configuration for a company account. `modelName` names
 * the model as the page's model menu does; without it, the id is shown.
 */
export function cleanupRouteLine(route: CleanupRoute, modelName: (id: string) => string = (id) => id): {
  value: string;
  detail: string;
  problem: boolean;
  more: string | null;
} {
  switch (route.state) {
    case "off":
      return { value: "Off", detail: "Dictations are typed exactly as they were transcribed.", problem: false, more: null };
    case "problem":
      return {
        value: "On, but it can't be used",
        detail: "Dictations are typed as transcribed until this is fixed.",
        problem: true,
        more: route.reason,
      };
    case "ready": {
      const local = route.destination === "local model";
      const selfHosted = route.destination.startsWith("self-hosted");
      const place = local ? "This computer" : selfHosted ? "Your server" : destinationName(route.destination);
      const value = `${place} · ${modelName(route.model)}`;
      const address = selfHosted ? `The server is at ${route.destination.replace(/^self-hosted at\s*/, "")}.` : null;
      if (route.caveat !== null) {
        const fix = local
          ? `No local model is running. Start one under Local text models; ${UNTIL_FIXED}.`
          : selfHosted
            ? `Your server can't be used yet; ${UNTIL_FIXED}.`
            : COMPANY_ACCOUNTS.has(route.destination)
              ? `${place} isn't set up yet. Set it up under Company accounts below; ${UNTIL_FIXED}.`
              : `No key is saved for ${place}. Add one on the Providers page; ${UNTIL_FIXED}.`;
        return { value, detail: fix, problem: true, more: [route.caveat, address].filter(Boolean).join(" ") };
      }
      const where = !route.leavesDevice
        ? "Text stays on this computer."
        : selfHosted
          ? "Text is sent to your server to tidy it up."
          : `Text is sent to ${place} to tidy it up.`;
      return { value, detail: where, problem: false, more: address };
    }
  }
}

/**
 * Whether a provider can be used for cleanup and summaries, in words, with
 * Rust's reason for Learn more (`reasoning.rs`, `dispatch_for`).
 */
export function reasoningDispatchLine(dispatch: ReasoningDispatch): { text: string; more: string | null } {
  switch (dispatch.state) {
    case "ready":
      return { text: "Can be used", more: null };
    case "needs-config":
      return { text: "Set up under Company accounts below", more: dispatch.reason };
    case "needs-sidecar":
      return { text: "Start a local model first", more: dispatch.reason };
    case "unresolvable":
      return { text: "Can't be used", more: dispatch.reason };
  }
}
