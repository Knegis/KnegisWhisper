/**
 * Which cloud speech services Settings > Transcription draws a card for, and
 * which it offers to add (KNEG-573).
 *
 * The page drew a card for every provider in the registry: seven, each with a
 * model select and an enabled "Use for dictation", all reading "No key saved"
 * on a profile with no keys. A card is now for a service a person has a key
 * for, or the one dictation is set to. The rest are offered under one
 * disclosure, where a key can be entered in place (KNEG-572).
 *
 * A service whose client is not built (`needs-client`) is in neither list: no
 * key makes it usable for dictation, so offering it a key here would be
 * offering nothing. Its key, if it has another use, is still on Providers.
 *
 * Pure, with no React, for `tests/cloudServices.test.ts`.
 */

import type { ProviderSummary, RouteReport } from "../generated/ipc-types.js";

export function cloudServices<P extends Pick<ProviderSummary, "id" | "configured" | "dispatch">>(
  providers: readonly P[],
  route: Pick<RouteReport, "isLocal" | "destination">,
): { cards: P[]; addable: P[]; inUse: string | null } {
  // `destination` is the resolved provider's id when the route is not local.
  const inUse = route.isLocal ? null : (providers.find((p) => p.id === route.destination)?.id ?? null);
  const usable = providers.filter((p) => p.dispatch.state !== "needs-client");
  return {
    cards: usable.filter((p) => p.configured || p.id === inUse),
    addable: usable.filter((p) => !p.configured && p.id !== inUse),
    inUse,
  };
}
