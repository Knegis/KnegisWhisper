/**
 * The local speech models as one list (KNEG-330).
 *
 * Settings > Transcription listed the Whisper models and the Parakeet models
 * apart, each with its own in-use rule, so choosing between a Whisper and a
 * Parakeet model for dictation meant reading two lists and the sentence
 * between them. They are one choice for dictation -- one local model at a
 * time -- so they are one list here: dictation models first, then the
 * meetings models, each smallest first. The engine each runs on goes under a
 * row's Details.
 *
 * Pure, so the rules are tested (`tests/localModels.test.ts`); the screen
 * renders the rows and owns the buttons' IPC.
 */

import { destinationName, LANGUAGES } from "./controlBar.js";
import type {
  LocalStatus,
  RouteReport,
  SherpaModelStatus,
  SherpaStatus,
  UsedFor,
  VerifiedBy,
} from "./ipc.js";
import { useForDetail, useForLabel, whisperModelHint } from "./labels.js";

/** Which engine a model runs on: whisper.cpp, or sherpa-onnx (Parakeet and Nemotron). */
export type ModelEngine = "whisper" | "parakeet";

/** The language codes a model is listed for, or `"many"` where no list is known. */
export type ModelLanguages = readonly string[] | "many";

export interface LocalModelRow {
  id: string;
  name: string;
  engine: ModelEngine;
  usedFor: UsedFor;
  languages: ModelLanguages;
  sizeMb: number | null;
  installed: boolean;
  recommended: boolean;
  inUse: boolean;
  /**
   * Where dictation goes instead, for the Whisper model the running server
   * has loaded while dictation goes elsewhere; `null` otherwise. Loaded is not
   * in use, and the row says so rather than looking like it is.
   */
  loadedButGoesTo: string | null;
  /** The KNEG-407 "slower" hint, for a big Whisper model. */
  hint: string | null;
  /** How a Whisper model's download is checked; `null` for a Parakeet model. */
  verifiedBy: VerifiedBy | null;
}

/**
 * A Whisper model's languages, from its id: the registry has no field for
 * them. KB Whisper is KBLab's Swedish fine-tune; a `-en` or `.en` suffix is
 * whisper.cpp's English-only naming; any other Whisper model is multilingual,
 * with no list to give. `tests/localModels.test.ts` holds every Whisper model
 * in `config/provider-registry.json` to this, so a new one with an unusual id
 * fails there rather than being mislabelled.
 */
export function whisperLanguages(id: string): ModelLanguages {
  if (id.startsWith("kb-whisper-")) return ["sv"];
  if (/[-.]en$/.test(id)) return ["en"];
  return "many";
}

/**
 * A Parakeet or Nemotron model's languages: the registry's list, else its one
 * language. `multilingual` with no list is many; no language at all is an
 * empty list, which no language filter keeps.
 */
export function sherpaLanguages(model: Pick<SherpaModelStatus, "language" | "supportedLanguages">): ModelLanguages {
  if (model.supportedLanguages && model.supportedLanguages.length > 0) return model.supportedLanguages;
  if (model.language === "multilingual") return "many";
  return model.language === null ? [] : [model.language];
}

const DISPLAY_NAMES = new Intl.DisplayNames(["en"], { type: "language" });

/**
 * A language code by name: the eight dictation offers as the language picker
 * names them, any other in English, and a code nobody names as itself.
 */
function languageName(code: string): string {
  const known = LANGUAGES.find(([c]) => c === code)?.[1];
  if (known) return known;
  try {
    return DISPLAY_NAMES.of(code) ?? code;
  } catch {
    return code;
  }
}

/** Every language a model lists, by name, for its Details. */
export function languageNames(codes: readonly string[]): string[] {
  return codes.map(languageName);
}

/** A model's languages in a few words: one or two by name, otherwise "Many languages". */
export function modelLanguageLabel(languages: ModelLanguages): string {
  if (languages === "many" || languages.length > 2) return "Many languages";
  if (languages.length === 0) return "Language not listed";
  return languages.map(languageName).join(" and ");
}

/** The rows a language filter keeps: those listing it, and those with many and no list. `null` keeps all. */
export function filterByLanguage(rows: readonly LocalModelRow[], code: string | null): LocalModelRow[] {
  if (code === null) return [...rows];
  return rows.filter((row) => row.languages === "many" || row.languages.includes(code));
}

/** Smaller first, an unknown size last, then by name. */
export function bySize(a: Pick<LocalModelRow, "name" | "sizeMb">, b: Pick<LocalModelRow, "name" | "sizeMb">): number {
  if (a.sizeMb !== b.sizeMb) {
    if (a.sizeMb === null) return 1;
    if (b.sizeMb === null) return -1;
    return a.sizeMb - b.sizeMb;
  }
  return a.name.localeCompare(b.name);
}

/**
 * Every local model this computer can run, in the order the screen lists
 * them: dictation models (Whisper and Parakeet offline) before the meetings
 * models, each smallest first. An engine not published for this platform
 * contributes no rows; the screen says why under Engines.
 */
export function localModelRows(local: LocalStatus, sherpa: SherpaStatus, route: RouteReport): LocalModelRow[] {
  const whisper: LocalModelRow[] =
    local.availability.state === "not-published"
      ? []
      : local.models.map((m) => {
          /*
            Two facts, kept apart on purpose. `loaded` is which model the
            running server has; `inUse` also requires the dictation route to
            resolve to it. A selected sherpa model outranks a running whisper
            server -- see `local_asr`'s module doc -- so the whisper row must
            not claim to be in use while one is selected, even though its
            server is up and the route is local.
          */
          const loaded = local.runningModel === m.id;
          const inUse = loaded && route.isLocal && sherpa.selected === null;
          const parakeet = sherpa.models.find((s) => s.id === sherpa.selected)?.name ?? sherpa.selected;
          return {
            id: m.id,
            name: m.name,
            engine: "whisper",
            usedFor: "dictation",
            languages: whisperLanguages(m.id),
            sizeMb: m.sizeMb,
            installed: m.installed,
            recommended: m.recommended,
            inUse,
            loadedButGoesTo:
              loaded && !inUse
                ? sherpa.selected !== null && route.isLocal
                  ? parakeet
                  : route.problem
                    ? "nowhere yet"
                    : destinationName(route.destination)
                : null,
            hint: whisperModelHint(m.sizeMb),
            verifiedBy: m.verifiedBy,
          };
        });

  const parakeet: LocalModelRow[] =
    sherpa.availability.state === "not-published"
      ? []
      : sherpa.models.map((m) => ({
          id: m.id,
          name: m.name,
          engine: "parakeet",
          usedFor: m.usedFor,
          languages: sherpaLanguages(m),
          sizeMb: m.sizeMb,
          installed: m.installed,
          recommended: false,
          // Which selection the row reads is decided by the engine the model
          // runs on. A streaming model is in use when meetings are set to it,
          // and dictation's route has nothing to say about that.
          inUse: m.usedFor === "meetings" ? sherpa.selectedStream === m.id : sherpa.selected === m.id && route.isLocal,
          loadedButGoesTo: null,
          hint: null,
          verifiedBy: null,
        }));

  const rank = (row: LocalModelRow) => (row.usedFor === "meetings" ? 1 : 0);
  return [...whisper, ...parakeet].sort((a, b) => rank(a) - rank(b) || bySize(a, b));
}

/**
 * The list in three parts: what is installed, what is recommended, and the
 * rest (KNEG-573).
 *
 * One flat list put fifteen models on the page with the installed ones
 * wherever their size happened to sort them. The parts answer the two
 * questions a person arrives with -- what do I have, what should I get --
 * before the catalogue, which the page folds.
 *
 * - **installed**: every installed model, the one in use first, then in the
 *   order `localModelRows` gave. A model that is loaded while dictation goes
 *   elsewhere is not "in use" and keeps its place.
 * - **recommended**: recommended models that are not installed. The flag is
 *   the registry's and nothing more: it is not sized to this computer, whose
 *   memory and processor this application does not read.
 * - **more**: the rest, in the order given.
 *
 * Every row is in exactly one part.
 */
export function localModelSections(rows: readonly LocalModelRow[]): {
  installed: LocalModelRow[];
  recommended: LocalModelRow[];
  more: LocalModelRow[];
} {
  const installed = rows.filter((row) => row.installed);
  return {
    installed: [...installed.filter((row) => row.inUse), ...installed.filter((row) => !row.inUse)],
    recommended: rows.filter((row) => !row.installed && row.recommended),
    more: rows.filter((row) => !row.installed && !row.recommended),
  };
}

/** The row's first line: name, size, languages, then recommended and the KNEG-407 hint. */
export function localModelLine(row: LocalModelRow): string {
  return [
    row.name,
    row.sizeMb !== null ? `${row.sizeMb} MB` : null,
    modelLanguageLabel(row.languages),
    row.recommended ? "recommended" : null,
    row.hint,
  ]
    .filter((part) => part !== null)
    .join(" · ");
}

/**
 * The row's second line: what the model is for, and whether it is installed.
 * A Whisper model serves meetings and imports as well as dictation (KNEG-407).
 */
export function localModelDetail(row: LocalModelRow): string {
  const purpose = row.engine === "whisper" ? "For dictation, imports and meetings" : useForDetail(row.usedFor);
  return `${purpose} · ${row.installed ? "Installed" : "Not installed"}`;
}

/** The row's button: Install until it is installed, then what choosing it does. */
export function localModelAction(row: LocalModelRow): string {
  return row.installed ? useForLabel(row.usedFor, row.inUse) : "Install";
}

/** One chip on a model's row. */
export interface ModelChip {
  text: string;
  tone: "plain" | "installed" | "recommended" | "warn";
}

/**
 * What a person chooses a model by, as chips: size, languages, what a sherpa
 * model is for, recommended, the KNEG-407 hint, whether it is installed, and
 * last the warning about a model loaded while dictation goes elsewhere. A
 * Whisper model serves dictation, meetings and imports alike, so it has no
 * purpose to name; the section's own sentence says so.
 */
export function localModelChips(row: LocalModelRow): ModelChip[] {
  const chips: ModelChip[] = [];
  if (row.sizeMb !== null) chips.push({ text: `${row.sizeMb} MB`, tone: "plain" });
  chips.push({ text: modelLanguageLabel(row.languages), tone: "plain" });
  if (row.engine !== "whisper") chips.push({ text: useForDetail(row.usedFor), tone: "plain" });
  if (row.recommended) chips.push({ text: "recommended", tone: "recommended" });
  if (row.hint !== null) chips.push({ text: row.hint, tone: "plain" });
  chips.push(row.installed ? { text: "Installed", tone: "installed" } : { text: "Not installed", tone: "plain" });
  if (row.loadedButGoesTo !== null) chips.push({ text: `Loaded, but dictation goes to ${row.loadedButGoesTo}`, tone: "warn" });
  return chips;
}

/** Which small picture a model's row shows, and which of that picture's variants. */
export interface ModelThumb {
  kind: "breath" | "dusk" | "streak";
  variant: number;
}

const THUMB_VARIANTS: Record<ModelThumb["kind"], number> = { breath: 5, dusk: 2, streak: 2 };

/** The pictures the design gives the models it draws; any other falls to its id. */
const THUMB_PICKS: ReadonlyArray<readonly [RegExp, number]> = [
  [/^kb-/, 2],
  [/turbo|large/, 1],
  [/medium/, 3],
  [/base/, 4],
  [/small/, 0],
  [/unified/, 0],
  [/tdt/, 1],
  [/nemotron-speech/, 0],
  [/nemotron-3/, 1],
];

/**
 * Decoration: it tells models apart and says nothing about them, beyond the
 * family the picture belongs to. From the id, so a row keeps its picture from
 * one render and one list order to the next.
 */
export function modelThumb(row: Pick<LocalModelRow, "id" | "engine" | "usedFor">): ModelThumb {
  const kind = row.engine === "whisper" ? "breath" : row.usedFor === "meetings" ? "streak" : "dusk";
  const picked = THUMB_PICKS.find(([pattern]) => pattern.test(row.id));
  let hash = picked ? picked[1] : 0;
  if (!picked) for (const char of row.id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return { kind, variant: hash % THUMB_VARIANTS[kind] };
}
