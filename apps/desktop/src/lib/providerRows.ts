/**
 * What Settings > Providers lists, and under which heading.
 *
 * Pure, and with no React: `tests/providerRows.test.ts` imports this directly,
 * and `views/SettingsProviders.tsx` only draws what it returns.
 *
 * Every row is a credential slot from `getCredentialSlots` -- the registry's
 * `credentials` array, through Rust -- so nothing here names a provider. What
 * a row says about itself comes from these fields and nowhere else:
 *
 *   - **`purposes`** from the slot's `usedFor`, in its order: `transcription`
 *     is "Speech to text" and `reasoning` is "Cleanup and summaries". A use
 *     the page has no words for is left out rather than shown as its id. Then
 *     "Live meetings", when a meeting provider's client reads the slot
 *     (`getMeetingProviders().credentialSlot`, from `credential_slot` in
 *     `crates/meeting-core/src/capabilities.rs`). That is how Deepgram's and
 *     AssemblyAI's rows say what their keys are for: their `usedFor` is empty
 *     on purpose (`crates/provider-registry/src/lib.rs`,
 *     `assemblyai_and_deepgram_are_provider_slots_upstream_never_needed`),
 *     and before KNEG-373 they had no description and no filter found them.
 *     Custom endpoint and enterprise slots still have no purposes.
 *   - **`letter`**, the first character of `name`, for the tile.
 *   - **`unsupported`** from `getProviders().dispatch`, matched on
 *     `credentialSlot`: the slot's transcription provider is `needs-client`
 *     and speech is the only thing the slot is used for -- no other use in
 *     `usedFor`, and no meeting client reading it. A slot with another use
 *     keeps its +, because a key for it still does something -- Tinfoil's
 *     transcription is refused and its reasoning is not (`clients::shape` and
 *     `resolve_chat`, read, not run) -- and a row that hid the form would stop
 *     this page storing a key the registry declares.
 *
 * "Speech to text" also finds the "Live meetings" keys: live meeting
 * transcription is speech to text, and a fourth tab would crowd a narrow
 * window for a distinction the row's own description already makes.
 *
 * The four lists: custom endpoints and enterprise connectors by `kind`, stored
 * or not, because what they need is different from a key on its own; the
 * rest are in use when a key is stored and available when not. Each keeps the
 * order the slots arrived in, which is the registry's.
 */

import type { CredentialSlotStatus, MeetingProviderSummary, ProviderSummary } from "../generated/ipc-types.js";

export type ProviderRow = { slot: string; name: string; letter: string; purposes: string[]; stored: boolean; unsupported: boolean };

export type ProviderFilter = "all" | "speech" | "text";

export const SPEECH = "Speech to text";
export const TEXT = "Cleanup and summaries";
export const MEETINGS = "Live meetings";

const PURPOSE: Readonly<Record<string, string>> = { transcription: SPEECH, reasoning: TEXT };

export function providerRows(
  slots: readonly CredentialSlotStatus[],
  transcription: readonly ProviderSummary[],
  meetings: readonly Pick<MeetingProviderSummary, "credentialSlot">[] = [],
): { inUse: ProviderRow[]; available: ProviderRow[]; custom: ProviderRow[]; enterprise: ProviderRow[] } {
  const lists = { inUse: [] as ProviderRow[], available: [] as ProviderRow[], custom: [] as ProviderRow[], enterprise: [] as ProviderRow[] };
  const meetingSlots = new Set(meetings.flatMap((m) => m.credentialSlot ?? []));
  for (const slot of slots) {
    const forMeetings = meetingSlots.has(slot.slot);
    const purposes = [...slot.usedFor.flatMap((use) => PURPOSE[use] ?? []), ...(forMeetings ? [MEETINGS] : [])];
    const noClient = transcription.some((p) => p.credentialSlot === slot.slot && p.dispatch.state === "needs-client");
    const row: ProviderRow = {
      slot: slot.slot,
      name: slot.name,
      letter: [...slot.name.trim()][0]?.toLocaleUpperCase() ?? "",
      purposes,
      stored: slot.stored,
      unsupported: noClient && !forMeetings && slot.usedFor.every((use) => use === "transcription"),
    };
    const list = slot.kind === "custom-endpoint" ? lists.custom
      : slot.kind === "enterprise" ? lists.enterprise
      : slot.stored ? lists.inUse
      : lists.available;
    list.push(row);
  }
  return lists;
}

/**
 * Whether `slot`'s key row is on screen under `filter` and `query`: in one of
 * the three lists that have a key form, and kept by them.
 *
 * The page closes a row's form, dropping what was typed, when this turns
 * false. A typed key stayed in the form of a row a filter or a search had
 * hidden, and came back with it (KNEG-377).
 */
export function keyRowShown(
  lists: ReturnType<typeof providerRows>,
  slot: string,
  filter: ProviderFilter,
  query: string,
): boolean {
  return filterRows([...lists.inUse, ...lists.available, ...lists.custom], filter, query).some((row) => row.slot === slot);
}

/**
 * The rows a filter tab and the search field leave: by purpose, then by name,
 * ignoring case. "Speech to text" keeps the "Live meetings" keys too.
 */
export function filterRows(rows: readonly ProviderRow[], filter: ProviderFilter, query: string): ProviderRow[] {
  const wanted = filter === "speech" ? [SPEECH, MEETINGS] : filter === "text" ? [TEXT] : null;
  const needle = query.trim().toLocaleLowerCase();
  return rows.filter((row) =>
    (wanted === null || row.purposes.some((purpose) => wanted.includes(purpose))) &&
    (needle === "" || row.name.toLocaleLowerCase().includes(needle)));
}
