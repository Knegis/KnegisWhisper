/**
 * The steps that switch where dictation goes, shared by the buttons on
 * Settings > Transcription and the Dictation page's route menu, so the two
 * switch the same way.
 *
 * Imports the IPC, so no test loads it; what to switch to is decided in
 * `lib/dictationRoute.ts`, which is tested.
 *
 * A local model is two steps under one choice, and both are said: a server
 * that is up is not the same as a route pointing at it, so the engine is
 * readied first and the route pointed at it second. A step that fails stops
 * there, so the route is never pointed at an engine that did not come up --
 * though a failed start may already have stopped the server it replaced.
 *
 * The engine's own step is a parameter that defaults to its wrapper.
 * Settings > Transcription passes it by name, because the settings gate's
 * rule F looks for the wrapper that writes `transcription.localModel` and
 * `transcription.sherpaModel` in that page's own file and nowhere else
 * (`tools/settings-audit/scan.mjs`, `usesWrapper`).
 */

import type { RouteChoice } from "./dictationRoute.js";
import { setCloudProvider, startLocalServer, useLocalModel, useSherpaModel } from "./ipc.js";

/**
 * Dictation to a cloud provider and model. Not Corti: Rust refuses Corti
 * without an environment and a tenant, which only Settings takes, and
 * Settings writes that one itself.
 */
export async function chooseCloudRoute(id: string, model: string): Promise<void> {
  await setCloudProvider(id, model);
}

/**
 * Dictation to a whisper model: start the server with it, then point the
 * route at it. `startLocalServer` stops a server running another model and
 * waits for the new one, which takes seconds, and releases a Parakeet
 * selection (`local_server_start`).
 */
export async function chooseWhisperRoute(
  id: string,
  start: (id: string) => Promise<unknown> = startLocalServer,
): Promise<void> {
  await start(id);
  await useLocalModel(true);
}

/** Dictation to a Parakeet model: select it, then point the route at the local engine. */
export async function chooseParakeetRoute(
  id: string,
  select: (id: string) => Promise<unknown> = useSherpaModel,
): Promise<void> {
  await select(id);
  await useLocalModel(true);
}

/** Apply one of the route menu's choices. */
export function chooseRoute(choice: RouteChoice): Promise<void> {
  switch (choice.kind) {
    case "cloud":
      return chooseCloudRoute(choice.provider, choice.model);
    case "whisper":
      return chooseWhisperRoute(choice.model);
    case "parakeet":
      return chooseParakeetRoute(choice.model);
  }
}
