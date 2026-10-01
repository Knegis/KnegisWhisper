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

import type { CleanupSettings, ReasoningProviderSummary } from "../generated/ipc-types.js";

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
