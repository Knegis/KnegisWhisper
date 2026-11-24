/**
 * The first-run setup guide on Dictation, as the things it says (KNEG-329).
 *
 * Three steps: where speech becomes text, the microphone, and a first try.
 * Each is done on evidence and on nothing else. The first is done when the
 * route would work, by the same reading `dictationReadiness` uses, so the
 * guide cannot call something ready that the page then says is not. The
 * second is done when a check heard sound. The third is done when a dictation
 * came back with text.
 *
 * The guide downloads nothing and sends nothing. It can offer Install; it
 * never presses it.
 *
 * Pure, with no React, for `tests/setupGuide.test.ts`.
 */

import type { ProviderSummary, RouteReport } from "../generated/ipc-types.js";

export type RouteStep = { done: boolean; line: string };

/** Step one: where dictation goes now, and whether that would work. */
export function routeStep(
  route: Pick<RouteReport, "isLocal" | "destination" | "problem">,
  providers: readonly Pick<ProviderSummary, "id" | "name" | "configured">[],
): RouteStep {
  if (route.problem) return { done: false, line: "Nothing is chosen yet." };
  if (route.isLocal) return { done: true, line: "On this computer. Your audio stays here." };
  const service = providers.find((p) => p.id === route.destination);
  // A destination that is no listed service is a server of the person's own.
  if (!service) return { done: true, line: "At a server you set up." };
  return service.configured
    ? { done: true, line: `At ${service.name}. Your audio is sent to ${service.name}.` }
    : { done: false, line: `${service.name} is chosen, and it has no key yet.` };
}

/** How many of the three steps are done, for the guide's own count and its button. */
export function stepsDone(route: RouteStep, heard: boolean, tried: boolean): number {
  return [route.done, heard, tried].filter(Boolean).length;
}

/** The button that closes the guide: it is finished, or it is being skipped. */
export function closeLabel(done: number): string {
  return done === 3 ? "Done" : "Skip setup";
}
