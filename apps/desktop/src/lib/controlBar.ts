/**
 * What a task page's control bar says: where the audio goes, where the text
 * goes, and the words on its chips.
 *
 * Pure, and with no React: `tests/controlBar.test.ts` imports this directly,
 * and `components/ControlBar.tsx` only draws what it returns.
 *
 * # The privacy line
 *
 * It is never hidden (the redesign's decision 5), so every route has a
 * sentence, including the ones the canvas did not draw:
 *
 *   - **The local model** -- `isLocal` -- stays on this computer.
 *   - **A route that leaves the device** names where it goes.
 *   - **A self-hosted route on a private address** is sent to a server on
 *     your network. Rust sets `leavesDevice` from
 *     `kw_network_policy::is_private_host` for an HTTP route
 *     (`dictation_route`, `src-tauri/src/dictation.rs`), so `false` there
 *     means "on your network", which may be another machine. The old
 *     Dictation page said "stays on this computer" for it, which is only true
 *     when the server happens to be on loopback. It says "sent to" because
 *     the audio leaves this computer, and the sentence is only ever "stays on
 *     this computer" or "sent to" something (ruling R15).
 *   - **A route with a problem** says the audio stays: the backend resolves
 *     the route at Stop and cancels when it cannot (`toggle`,
 *     `src-tauri/src/dictation.rs`), so nothing is sent.
 *
 * Cleanup adds a second sentence only when it is `ready`, leaves the device,
 * and carries no `caveat`. A caveat is Rust's "resolves, but cannot run" --
 * no key stored, a connector not configured -- and its own wording says
 * cleanup "will leave the transcript as it is" until that is fixed
 * (`describe`, `src-tauri/src/reasoning.rs`). That is believed from the
 * sentence, not traced through the cleanup call; the chip says "needs setup"
 * in the same case, so the two cannot disagree.
 */

import type { MenuOption } from "./chipMenu.js";
import type { CleanupRoute, InputDevice, RouteReport } from "./ipc.js";

/**
 * Dictation's languages, by code, in the order the picker lists them: the
 * eight `validated_local_languages` accepts (src-tauri/src/dictation.rs).
 * Moved here from `components/LocalLanguagePicker.tsx`, which imports it back,
 * so the chip, the picker and its note cannot name a language two ways.
 */
export const LANGUAGES = [
  ["sv", "Swedish"],
  ["en", "English"],
  ["da", "Danish"],
  ["no", "Norwegian"],
  ["fi", "Finnish"],
  ["de", "German"],
  ["fr", "French"],
  ["es", "Spanish"],
] as const;

/**
 * Display names for the provider ids a route reports as its `destination`.
 *
 * Copied from `config/provider-registry.json`'s `transcription`, `reasoning`
 * and `enterprise` lists rather than imported, so the bundle does not carry
 * the whole registry for a dozen names; `tests/controlBar.test.ts` reads the
 * registry and fails when the two disagree.
 */
const PROVIDER_NAMES: Readonly<Record<string, string>> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Google Gemini",
  groq: "Groq",
  berget: "Berget AI",
  xai: "xAI",
  mistral: "Mistral",
  corti: "Corti",
  tinfoil: "Tinfoil",
  bedrock: "AWS Bedrock",
  azure: "Azure OpenAI",
  vertex: "GCP Vertex AI",
};

/**
 * Rust's names for the two routes a person points at a URL themselves
 * (`crates/transcription-core/src/route.rs`, `Transport::HttpBatch`). They are
 * not providers, so the registry has no name for them (KNEG-352).
 */
const ENDPOINT_NAMES: Readonly<Record<string, string>> = {
  "self-hosted": "your self-hosted server",
  custom: "your custom endpoint",
};

/**
 * A route's `destination` as a person reads it: the provider's name where the
 * registry has one, the endpoint's where it is one of those two, otherwise the
 * phrase Rust sent unchanged rather than guessed at.
 */
export function destinationName(destination: string): string {
  return PROVIDER_NAMES[destination] ?? ENDPOINT_NAMES[destination] ?? destination;
}

/**
 * The control bar's privacy sentences, audio first. See the module comment.
 *
 * `problem` is optional so a page whose route has no such field (Meetings)
 * can pass `{ isLocal, leavesDevice, destination }` alone; Dictation passes
 * its whole `RouteReport`. It is read rather than `destination === "nowhere"`,
 * because that phrase is Rust's for a person to read, and `RouteReport`'s own
 * comment says rewording it must not change what the screen does.
 */
export function privacyLines(
  route: { isLocal: boolean; leavesDevice: boolean; destination: string; problem?: string | null },
  cleanup: CleanupRoute | null,
): string[] {
  const lines = [
    route.leavesDevice
      ? `Audio is sent to ${destinationName(route.destination)}`
      : route.isLocal || (route.problem ?? null) !== null
        ? "Audio stays on this computer"
        : "Audio is sent to a server on your network",
  ];
  if (cleanup?.state === "ready" && cleanup.leavesDevice && cleanup.caveat === null) {
    lines.push(`Text is sent to ${destinationName(cleanup.destination)} to tidy it up`);
  }
  return lines;
}

/**
 * The language chip: the chosen languages by name, or "Detect language" when
 * none is chosen. A route that is told a language all the same (Corti) says
 * so in the picker's note (`languageNote`). A code with no name here is shown as
 * itself rather than dropped, so the chip never claims fewer languages than
 * are set.
 */
export function languageLabel(codes: readonly string[]): string {
  if (codes.length === 0) return "Detect language";
  return codes
    .map((code) => LANGUAGES.find(([known]) => known === code)?.[1] ?? code)
    .join(", ");
}

/** How every language note begins; what follows depends on the route. */
export const LANGUAGE_NOTE_LEAD = "Choose up to two.";

/** A language by name, or by its code where this build has no name for it, as `languageLabel` does. */
function languageName(code: string): string {
  return LANGUAGES.find(([known]) => known === code)?.[1] ?? code;
}

/**
 * A destination's name to begin a sentence with. The two endpoint phrases
 * ("your self-hosted server") take a capital; a provider's own name never
 * does, since "xAI" is spelt that way.
 */
function sentenceStart(destination: string): string {
  const endpoint = ENDPOINT_NAMES[destination];
  return endpoint === undefined ? destinationName(destination) : endpoint.charAt(0).toUpperCase() + endpoint.slice(1);
}

/**
 * The line under Dictation's language picker: what the route in use does
 * with the languages chosen (KNEG-370). Always "Choose up to two.", then:
 *
 *   - **local Whisper** checks each dictation for which of two was spoken;
 *   - **a cloud route** says what it is sent, from `route.cloudLanguage`,
 *     which Rust settles (`route::resolve`) -- so xAI dropping a language it
 *     cannot take, and Corti being told English when not exactly one is
 *     chosen, are read here rather than repeated. With nothing sent, it
 *     says the provider detects the language, and why when one was chosen;
 *   - **a route with a problem**, which sends nothing, and a **Parakeet**
 *     model, which uses its own languages, add nothing.
 */
export function languageNote(
  engine: "whisper" | "parakeet" | null,
  route: RouteReport,
  chosen: readonly string[],
): string {
  const more = languageNoteMore(engine, route, chosen);
  return more === null ? LANGUAGE_NOTE_LEAD : `${LANGUAGE_NOTE_LEAD} ${more}`;
}

function languageNoteMore(
  engine: "whisper" | "parakeet" | null,
  route: RouteReport,
  chosen: readonly string[],
): string | null {
  if (route.problem !== null) return null;
  if (engine === "whisper") return "With two, each dictation is checked for which one you spoke.";
  if (route.isLocal) return null;
  const provider = sentenceStart(route.destination);
  if (route.cloudLanguage !== null) {
    const language = languageName(route.cloudLanguage);
    return chosen.length === 1
      ? `${provider} is told to expect ${language}.`
      : `${provider} needs one language, so it is told to expect ${language}.`;
  }
  if (chosen.length === 2) return `With two, ${destinationName(route.destination)} detects the language itself.`;
  const [only] = chosen;
  return only === undefined
    ? `${provider} detects the language itself.`
    : `${provider} can't be told ${languageName(only)}, so it detects the language itself.`;
}

/**
 * Dictation's microphone chip and its menu, from what the backend reports:
 * the devices, and the one chosen (`null` for the system default).
 *
 * `missing` is a microphone chosen earlier and unplugged since. It is listed
 * first, as itself and current, rather than letting the menu fall back to
 * "System default", which would read as a choice nobody made. The recording
 * itself does fall back to the default -- `Capture::start` in
 * crates/audio-core/src/capture.rs asks for the device by name and takes the
 * default when it is gone -- so the page says that beside it.
 *
 * The chip's label is the current row's own words, so the full name is the
 * chip's `title` once the chip has cut it short. "System default" is id `""`,
 * which the page turns back into `null` when it is chosen.
 *
 * `group` puts every row under one heading -- the Meetings page's "Used by
 * meetings and dictation", which says beside the rows that the choice is
 * shared (KNEG-364). Dictation's menu has none.
 */
export function microphoneMenu(
  devices: readonly InputDevice[],
  device: string | null,
  group: string | null = null,
): { label: string; missing: string | null; options: MenuOption[] } {
  const missing = device !== null && !devices.some((d) => d.name === device) ? device : null;
  const row = (id: string, label: string, current: boolean): MenuOption => ({
    id,
    label,
    group,
    current,
    disabled: false,
    reason: null,
  });
  const options = [
    ...(missing === null ? [] : [row(missing, `${missing} (not connected)`, true)]),
    row("", "System default", device === null),
    ...devices.map((d) => row(d.name, d.isDefault ? `${d.name} (default)` : d.name, d.name === device)),
  ];
  return { label: options.find((o) => o.current)?.label ?? "System default", missing, options };
}

/**
 * The Tidy up chip. `null` is a page with no cleanup step to speak of, which
 * reads as off. A `ready` route with a caveat cannot run yet, so it is "needs
 * setup" alongside a `problem`, not "on".
 */
export function cleanupLabel(
  cleanup: CleanupRoute | null,
): "Tidy up: off" | "Tidy up: on" | "Tidy up: needs setup" {
  if (cleanup === null) return "Tidy up: off";
  switch (cleanup.state) {
    case "off":
      return "Tidy up: off";
    case "problem":
      return "Tidy up: needs setup";
    case "ready":
      return cleanup.caveat === null ? "Tidy up: on" : "Tidy up: needs setup";
  }
}
