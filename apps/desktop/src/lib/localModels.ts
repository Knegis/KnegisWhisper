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
function bySize(a: LocalModelRow, b: LocalModelRow): number {
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
