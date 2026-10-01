/**
 * Whether the summary settings are complete enough to turn summaries on.
 *
 * Mirrors `SummarySettings::validate` (`crates/notes-core/src/summary.rs`),
 * which is the first thing `route()` in `src-tauri/src/summaries.rs` does and
 * what `summary_settings_set` runs when `enabled` is set: a model, a provider
 * or a self-hosted URL, a language Rust knows, and a context budget in range.
 * Rust's `is_none()` on the URL is mirrored as `=== null`, so a blank URL
 * counts as present, as it does there.
 *
 * What it cannot mirror is the rest of `route()`: whether the provider id is in
 * the registry (`resolve_chat`) or the URL parses. Those are refused by Rust
 * when the toggle writes, and the page shows the refusal.
 */

import type { SummarySettings } from "../generated/ipc-types.js";

export function summaryConfigured(settings: SummarySettings): boolean {
  if (settings.model.trim() === "") return false;
  if (settings.provider.trim() === "" && settings.selfHostedUrl === null) return false;
  if (!["match", "sv", "en"].includes(settings.language)) return false;
  return settings.contextBudget >= 4096 && settings.contextBudget <= 262_144;
}
