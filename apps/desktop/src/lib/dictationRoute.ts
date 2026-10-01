/**
 * Where dictation goes, as the Dictation page's route menu offers it: the
 * rows, which one is in effect, and what choosing each one writes.
 *
 * Pure, with no React: `tests/dictationRoute.test.ts` imports it directly.
 * The writes themselves are `lib/routeChoice.ts`, which imports the IPC and
 * so is never loaded by a test.
 *
 * # The rows
 *
 * Grouped by where the audio goes, the question the choice answers, as the
 * Meetings provider menu is (`providerMenu`):
 *
 *   - **On this computer**: the installed whisper models, then the installed
 *     Parakeet models for dictation. A model that is not installed has
 *     nothing to switch to, and installing is Settings > Transcription's job.
 *     A Parakeet model for meetings (`usedFor: "meetings"`) is not a
 *     dictation route at all, so it is not listed.
 *   - **Cloud**: the providers, in the registry's order, except those this
 *     build has no client for (`needs-client`), which no choice could make
 *     work. A provider with no key stored is greyed "needs a key": the key is
 *     `configured`, which `providers` (commands.rs) sets from the credential
 *     store, the same field Settings > Transcription reads for "will not work
 *     until a key is stored". Corti is greyed "set up in Settings" unless it
 *     is already in use, because it needs an environment and a tenant that
 *     only Settings takes; its `dispatch` is always `unresolvable`, since
 *     `dispatch_for` asks the router with neither set.
 *
 * # What is current
 *
 * The route Rust resolved, not a stored setting, read the way Settings >
 * Transcription marks its "In use" buttons. A whisper model is current when
 * the running server loaded it, the route is local, and no Parakeet model is
 * selected -- a selected one outranks the whisper server (`local_asr`'s
 * module doc). A Parakeet model is current when it is selected and the route
 * is local. A provider is current when the route is not local, has no
 * problem, and its `destination` is the provider's id. A local route with
 * neither (no server running) has no current row.
 *
 * # What choosing writes
 *
 * `choices` holds a `RouteChoice` for every row that can be chosen and is not
 * already in effect. The current row has none: choosing it writes nothing. For
 * a provider that is also how the route keeps its own model -- `RouteReport`
 * names no model and no read reports the one stored, so writing the
 * provider again would have to guess one. Any other provider is written with
 * its first model, as Settings does when its model picker is untouched.
 */

import type { MenuOption } from "./chipMenu.js";
import type { LocalStatus, ProviderSummary, RouteReport, SessionState, SherpaStatus } from "./ipc.js";

/** One switch of the route, and the words the chip says while it is applied. */
export type RouteChoice =
  | { kind: "whisper"; model: string; label: string }
  | { kind: "parakeet"; model: string; label: string }
  | { kind: "cloud"; provider: string; model: string; label: string };

export type RouteMenu = {
  options: MenuOption[];
  /** The row the route resolves to, or `null` when none does. */
  current: MenuOption | null;
  /** What choosing a row writes, by option id. See the module comment. */
  choices: ReadonlyMap<string, RouteChoice>;
};

const LOCAL = "On this computer";
const CLOUD = "Cloud";

/**
 * Which engine a local route runs on, or `null` for a route that is not
 * local: Parakeet while a model is selected, since it outranks a running
 * whisper server, and otherwise whisper.
 */
export function localEngine(route: RouteReport, sherpa: SherpaStatus): "whisper" | "parakeet" | null {
  if (!route.isLocal) return null;
  return sherpa.selected !== null ? "parakeet" : "whisper";
}

export function routeMenu(
  providers: readonly ProviderSummary[],
  local: LocalStatus,
  sherpa: SherpaStatus,
  route: RouteReport,
): RouteMenu {
  const engine = localEngine(route, sherpa);
  const choices = new Map<string, RouteChoice>();
  const row = (
    id: string,
    label: string,
    group: string,
    current: boolean,
    reason: string | null,
    choice: RouteChoice | null,
  ): MenuOption => {
    if (!current && reason === null && choice !== null) choices.set(id, choice);
    return { id, label, group, current, disabled: reason !== null, reason };
  };

  const whisper = local.models
    .filter((m) => m.installed)
    .map((m) =>
      row(`whisper:${m.id}`, m.name, LOCAL, engine === "whisper" && local.runningModel === m.id, null, {
        kind: "whisper",
        model: m.id,
        label: m.name,
      }),
    );
  const parakeet = sherpa.models
    .filter((m) => m.installed && m.usedFor === "dictation")
    .map((m) =>
      row(`parakeet:${m.id}`, m.name, LOCAL, engine === "parakeet" && sherpa.selected === m.id, null, {
        kind: "parakeet",
        model: m.id,
        label: m.name,
      }),
    );
  const cloud = providers
    .filter((p) => p.dispatch.state !== "needs-client")
    .map((p) => {
      const current = !route.isLocal && route.problem === null && route.destination === p.id;
      const model = p.models[0];
      const reason = !p.configured
        ? "needs a key"
        : model === undefined
          ? "no model listed"
          : p.id === "corti" && !current
            ? "set up in Settings"
            : null;
      return row(
        `cloud:${p.id}`,
        p.name,
        CLOUD,
        current,
        reason,
        model === undefined ? null : { kind: "cloud", provider: p.id, model, label: p.name },
      );
    });

  const options = [...whisper, ...parakeet, ...cloud];
  return { options, current: options.find((o) => o.current) ?? null, choices };
}

/**
 * What the route chip says while a choice is applied: "Starting <model>…" for
 * a local model, whose server can take seconds to come up, and nothing for a
 * provider, which is one quick write.
 */
export function routeBusyLabel(choice: RouteChoice | null): string | null {
  return choice === null || choice.kind === "cloud" ? null : `Starting ${choice.label}…`;
}

/**
 * Why the route and the language cannot be changed right now, or `null` when
 * they can: only between dictations, as the live meeting view holds its chips
 * until the meeting ends.
 *
 * While recording, because Rust resolves both at Stop (`toggle`,
 * src-tauri/src/dictation.rs): a change would apply to audio already captured
 * under the privacy line the page showed when it started. While the words are
 * transcribed, because choosing another whisper model stops the server
 * running the one in use (`start_model`, src-tauri/src/local_model.rs) --
 * believed from that code to fail the transcription in flight, not tried.
 * And while they are typed, so the chips are held for one span, from Start
 * until the dictation is over, rather than flickering back for a moment.
 */
export function routeHeld(state: SessionState): string | null {
  return state === "idle" ? null : "Can't change during a dictation";
}
