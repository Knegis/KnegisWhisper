/**
 * One list of local speech models on Settings > Transcription (KNEG-330).
 *
 * The Whisper and Parakeet models were two lists, each with its own in-use
 * rule. These pin the one list: every model once, dictation before meetings
 * and smaller before bigger, the two in-use rules exactly as they were, the
 * languages each model is listed for, and no model id on the row itself.
 *
 * The models are the dev fixtures' (`devFixtures.ts`), with their names and
 * sizes, plus the registry itself where a rule is derived from ids.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import type {
  LocalStatus,
  ModelStatus,
  RouteReport,
  SherpaModelStatus,
  SherpaStatus,
} from "../src/generated/ipc-types.js";
import {
  filterByLanguage,
  languageNames,
  localModelAction,
  localModelDetail,
  localModelLine,
  localModelChips,
  localModelRows,
  modelThumb,
  modelLanguageLabel,
  sherpaLanguages,
  whisperLanguages,
  type LocalModelRow,
} from "../src/lib/localModels.js";

function whisper(id: string, name: string, sizeMb: number | null, installed: boolean, recommended = false): ModelStatus {
  return { id, name, sizeMb, recommended, installed, verifiedBy: "digest" };
}

function sherpaModel(
  id: string,
  name: string,
  sizeMb: number | null,
  usedFor: "dictation" | "meetings",
  installed: boolean,
  language: string | null,
  supportedLanguages: string[] | null,
): SherpaModelStatus {
  return { id, name, sizeMb, language, supportedLanguages, installed, usedFor };
}

const TDT_LANGUAGES = ["bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk"];
const NEMOTRON_LANGUAGES = ["en", "es", "fr", "it", "pt", "nl", "de", "tr", "ru", "ar", "hi", "ja", "ko", "vi", "uk"];

const WHISPER = [
  whisper("whisper-large-v3-turbo-q5", "Whisper Large v3 Turbo (Q5)", 574, true, true),
  whisper("whisper-medium-q5", "Whisper Medium (Q5)", 1530, false),
  whisper("whisper-small-q5", "Whisper Small (Q5)", 488, true),
  whisper("kb-whisper-large", "KB Whisper Large (Swedish, Q5)", 1081, false),
  whisper("whisper-base-en", "Whisper Base (English)", null, false),
];

const SHERPA_MODELS = [
  sherpaModel("parakeet-unified-en-0.6b", "Parakeet Unified EN 0.6B", 630, "dictation", true, "en", ["en"]),
  sherpaModel("parakeet-tdt-0.6b-v3", "Parakeet TDT 0.6B v3", 660, "dictation", false, "multilingual", TDT_LANGUAGES),
  sherpaModel("nemotron-speech-streaming-en-0.6b", "Nemotron Speech Streaming EN 0.6B", 442, "meetings", true, "en", ["en"]),
  sherpaModel("nemotron-3.5-asr-streaming-0.6b", "Nemotron 3.5 ASR Streaming 0.6B", 650, "meetings", true, "multilingual", NEMOTRON_LANGUAGES),
];

function local(runningModel: string | null, over: Partial<LocalStatus> = {}): LocalStatus {
  return {
    runtimeMessage: "",
    availability: { state: "installable" },
    serverInstalled: true,
    serverFreshness: { state: "current" },
    models: WHISPER,
    port: runningModel === null ? null : 8178,
    runningModel,
    installDir: "",
    installDirFellBack: false,
    runtime: runningModel === null ? { state: "stopped" } : { state: "ready", model: runningModel, port: 8178 },
    ...over,
  };
}

function sherpa(selected: string | null, selectedStream: string | null, over: Partial<SherpaStatus> = {}): SherpaStatus {
  return {
    availability: { state: "installable" },
    cliInstalled: true,
    cliFreshness: { state: "current" },
    models: SHERPA_MODELS,
    selected,
    selectedStream,
    streamBindsLoopback: null,
    modelsDir: "",
    ...over,
  };
}

const LOCAL_ROUTE: RouteReport = { destination: "local model", isLocal: true, leavesDevice: false, problem: null, cloudLanguage: null };
const OPENAI_ROUTE: RouteReport = { destination: "openai", isLocal: false, leavesDevice: true, problem: null, cloudLanguage: "sv" };
const BROKEN_ROUTE: RouteReport = { destination: "nowhere", isLocal: false, leavesDevice: false, problem: "no provider", cloudLanguage: null };

const NOT_PUBLISHED = { state: "not-published", reason: "no build for this platform", tracking: "KNEG-0" } as const;

const row = (rows: LocalModelRow[], id: string): LocalModelRow => {
  const found = rows.find((r) => r.id === id);
  assert.ok(found, `no row for ${id}`);
  return found;
};

const names = (rows: LocalModelRow[]) => rows.map((r) => r.name);

/* ------------------------------------------------------------------ the list */

test("every model, Whisper and Parakeet, appears exactly once", () => {
  const rows = localModelRows(local(null), sherpa(null, null), OPENAI_ROUTE);
  const ids = rows.map((r) => r.id);
  assert.equal(new Set(ids).size, ids.length);
  assert.deepEqual([...ids].sort(), [...WHISPER, ...SHERPA_MODELS].map((m) => m.id).sort());
  assert.equal(row(rows, "kb-whisper-large").engine, "whisper");
  assert.equal(row(rows, "parakeet-tdt-0.6b-v3").engine, "parakeet");
  assert.equal(row(rows, "nemotron-3.5-asr-streaming-0.6b").engine, "parakeet");
});

test("dictation models come before meetings models, each smallest first, with an unknown size last", () => {
  const rows = localModelRows(local(null), sherpa(null, null), OPENAI_ROUTE);
  assert.deepEqual(names(rows), [
    "Whisper Small (Q5)",
    "Whisper Large v3 Turbo (Q5)",
    "Parakeet Unified EN 0.6B",
    "Parakeet TDT 0.6B v3",
    "KB Whisper Large (Swedish, Q5)",
    "Whisper Medium (Q5)",
    "Whisper Base (English)",
    "Nemotron Speech Streaming EN 0.6B",
    "Nemotron 3.5 ASR Streaming 0.6B",
  ]);
  assert.deepEqual(rows.map((r) => r.usedFor), [...Array(7).fill("dictation"), "meetings", "meetings"]);
});

test("models of the same size are in name order", () => {
  const rows = localModelRows(
    local(null, { models: [whisper("b", "Beta", 100, false), whisper("a", "Alpha", 100, false), whisper("c", "Gamma", null, false), whisper("d", "Delta", null, false)] }),
    sherpa(null, null, { models: [] }),
    OPENAI_ROUTE,
  );
  assert.deepEqual(names(rows), ["Alpha", "Beta", "Delta", "Gamma"]);
});

test("an engine that is not published here has its rows left out, and the other engine's stay", () => {
  const noWhisper = localModelRows(local(null, { availability: NOT_PUBLISHED }), sherpa(null, null), OPENAI_ROUTE);
  assert.deepEqual(noWhisper.map((r) => r.engine), ["parakeet", "parakeet", "parakeet", "parakeet"]);
  const noSherpa = localModelRows(local(null), sherpa(null, null, { availability: NOT_PUBLISHED }), OPENAI_ROUTE);
  assert.deepEqual(noSherpa.map((r) => r.engine), Array(5).fill("whisper"));
});

/* --------------------------------------------------------------- in use */

test("a Whisper model is in use when the server has it, the route is local and no Parakeet model is selected", () => {
  const rows = localModelRows(local("whisper-small-q5"), sherpa(null, null), LOCAL_ROUTE);
  assert.equal(row(rows, "whisper-small-q5").inUse, true);
  assert.equal(row(rows, "whisper-small-q5").loadedButGoesTo, null);
  assert.deepEqual(rows.filter((r) => r.inUse).map((r) => r.id), ["whisper-small-q5"]);
});

test("a selected Parakeet model outranks a running Whisper server, which says where dictation goes instead", () => {
  const rows = localModelRows(local("whisper-small-q5"), sherpa("parakeet-unified-en-0.6b", null), LOCAL_ROUTE);
  assert.equal(row(rows, "parakeet-unified-en-0.6b").inUse, true);
  assert.equal(row(rows, "whisper-small-q5").inUse, false);
  assert.equal(row(rows, "whisper-small-q5").loadedButGoesTo, "Parakeet Unified EN 0.6B");
  // Only the loaded model says it: the others are not loaded.
  assert.deepEqual(rows.filter((r) => r.loadedButGoesTo !== null).map((r) => r.id), ["whisper-small-q5"]);
});

test("off the local route nothing is in use for dictation, and the loaded model names where dictation goes", () => {
  const rows = localModelRows(local("whisper-small-q5"), sherpa("parakeet-unified-en-0.6b", null), OPENAI_ROUTE);
  assert.deepEqual(rows.filter((r) => r.usedFor === "dictation" && r.inUse).map((r) => r.id), []);
  assert.equal(row(rows, "whisper-small-q5").loadedButGoesTo, "OpenAI");
  const broken = localModelRows(local("whisper-small-q5"), sherpa(null, null), BROKEN_ROUTE);
  assert.equal(row(broken, "whisper-small-q5").loadedButGoesTo, "nowhere yet");
});

test("a meetings model is in use when meetings are set to it, whatever the dictation route", () => {
  for (const route of [OPENAI_ROUTE, LOCAL_ROUTE, BROKEN_ROUTE]) {
    const rows = localModelRows(local("whisper-small-q5"), sherpa("parakeet-unified-en-0.6b", "nemotron-speech-streaming-en-0.6b"), route);
    assert.equal(row(rows, "nemotron-speech-streaming-en-0.6b").inUse, true, route.destination);
    assert.equal(row(rows, "nemotron-3.5-asr-streaming-0.6b").inUse, false, route.destination);
    assert.equal(row(rows, "nemotron-speech-streaming-en-0.6b").loadedButGoesTo, null);
  }
});

/* -------------------------------------------------------------- buttons */

test("each row's button: Install until installed, then what the model is used for", () => {
  const rows = localModelRows(local("whisper-small-q5"), sherpa("parakeet-unified-en-0.6b", "nemotron-speech-streaming-en-0.6b"), LOCAL_ROUTE);
  assert.equal(localModelAction(row(rows, "whisper-medium-q5")), "Install");
  assert.equal(localModelAction(row(rows, "parakeet-tdt-0.6b-v3")), "Install");
  assert.equal(localModelAction(row(rows, "whisper-small-q5")), "Use for dictation");
  assert.equal(localModelAction(row(rows, "parakeet-unified-en-0.6b")), "In use for dictation");
  assert.equal(localModelAction(row(rows, "nemotron-speech-streaming-en-0.6b")), "In use for meetings");
  assert.equal(localModelAction(row(rows, "nemotron-3.5-asr-streaming-0.6b")), "Use for meetings");
  const whisperInUse = localModelRows(local("whisper-small-q5"), sherpa(null, null), LOCAL_ROUTE);
  assert.equal(localModelAction(row(whisperInUse, "whisper-small-q5")), "In use for dictation");
  assert.equal(localModelAction(row(whisperInUse, "whisper-large-v3-turbo-q5")), "Use for dictation");
});

/* ------------------------------------------------------------ languages */

test("a Whisper model's languages come from its id", () => {
  assert.deepEqual(whisperLanguages("kb-whisper-large"), ["sv"]);
  assert.deepEqual(whisperLanguages("kb-whisper-tiny"), ["sv"]);
  assert.deepEqual(whisperLanguages("whisper-base-en"), ["en"]);
  assert.deepEqual(whisperLanguages("base.en"), ["en"]);
  assert.equal(whisperLanguages("turbo"), "many");
  assert.equal(whisperLanguages("whisper-large-v3-turbo-q5"), "many");
  // An "en" that is not the suffix is not English-only.
  assert.equal(whisperLanguages("encoder-large"), "many");
});

test("every Whisper model in the registry: KB Whisper is Swedish, the rest many, and the names agree", () => {
  const registry = JSON.parse(
    readFileSync(new URL("../../../../config/provider-registry.json", import.meta.url), "utf8"),
  ) as { localAsr: { whisper: { id: string; name: string }[] } };
  const models = registry.localAsr.whisper;
  assert.ok(models.length > 0);
  for (const { id, name } of models) {
    if (id.startsWith("kb-whisper-")) {
      assert.deepEqual(whisperLanguages(id), ["sv"], id);
      assert.match(name, /Swedish/, id);
    } else {
      assert.equal(whisperLanguages(id), "many", id);
      // A model named for one language would be mislabelled by the id rule.
      assert.doesNotMatch(name, /Swedish|English/, id);
    }
  }
});

test("a Parakeet model's languages are its supported list, else its one language", () => {
  assert.deepEqual(sherpaLanguages({ language: "multilingual", supportedLanguages: TDT_LANGUAGES }), TDT_LANGUAGES);
  assert.deepEqual(sherpaLanguages({ language: "en", supportedLanguages: null }), ["en"]);
  assert.equal(sherpaLanguages({ language: "multilingual", supportedLanguages: null }), "many");
  assert.deepEqual(sherpaLanguages({ language: null, supportedLanguages: null }), []);
});

test("a language label names one or two languages, and says many for more", () => {
  assert.equal(modelLanguageLabel(["sv"]), "Swedish");
  assert.equal(modelLanguageLabel(["en"]), "English");
  assert.equal(modelLanguageLabel(["sv", "en"]), "Swedish and English");
  assert.equal(modelLanguageLabel("many"), "Many languages");
  assert.equal(modelLanguageLabel(TDT_LANGUAGES), "Many languages");
  assert.equal(modelLanguageLabel([]), "Language not listed");
  // A code with no name is shown as itself rather than dropped.
  assert.equal(modelLanguageLabel(["xx"]), "xx");
});

test("the full list names every language, beyond the eight dictation offers", () => {
  assert.deepEqual(languageNames(["sv", "bg", "en", "ja"]), ["Swedish", "Bulgarian", "English", "Japanese"]);
});

test("the Swedish filter keeps KB Whisper, multilingual Whisper and Parakeet TDT, and drops the English-only and both Nemotrons", () => {
  const rows = localModelRows(local(null), sherpa(null, null), OPENAI_ROUTE);
  assert.deepEqual(names(filterByLanguage(rows, "sv")), [
    "Whisper Small (Q5)",
    "Whisper Large v3 Turbo (Q5)",
    "Parakeet TDT 0.6B v3",
    "KB Whisper Large (Swedish, Q5)",
    "Whisper Medium (Q5)",
  ]);
  assert.deepEqual(filterByLanguage(rows, null), rows);
  // English keeps the multilingual Nemotron, which lists it.
  assert.ok(names(filterByLanguage(rows, "en")).includes("Nemotron 3.5 ASR Streaming 0.6B"));
  assert.ok(!names(filterByLanguage(rows, "en")).includes("KB Whisper Large (Swedish, Q5)"));
});

/* ----------------------------------------------------------------- words */

test("a row's line: name, size, languages, then recommended and the slower hint", () => {
  const rows = localModelRows(local(null), sherpa(null, null), OPENAI_ROUTE);
  assert.equal(
    localModelLine(row(rows, "whisper-large-v3-turbo-q5")),
    "Whisper Large v3 Turbo (Q5) · 574 MB · Many languages · recommended · slower; best for imports and meetings",
  );
  assert.equal(localModelLine(row(rows, "whisper-small-q5")), "Whisper Small (Q5) · 488 MB · Many languages");
  assert.equal(localModelLine(row(rows, "kb-whisper-large")), "KB Whisper Large (Swedish, Q5) · 1081 MB · Swedish · slower; best for imports and meetings");
  assert.equal(localModelLine(row(rows, "whisper-base-en")), "Whisper Base (English) · English");
  // Parakeet models are not given the Whisper hint, whatever their size.
  assert.equal(localModelLine(row(rows, "parakeet-tdt-0.6b-v3")), "Parakeet TDT 0.6B v3 · 660 MB · Many languages");
  assert.equal(localModelLine(row(rows, "parakeet-unified-en-0.6b")), "Parakeet Unified EN 0.6B · 630 MB · English");
});

test("a row's detail: what it is for, and whether it is installed", () => {
  const rows = localModelRows(local(null), sherpa(null, null), OPENAI_ROUTE);
  assert.equal(localModelDetail(row(rows, "whisper-small-q5")), "For dictation, imports and meetings · Installed");
  assert.equal(localModelDetail(row(rows, "whisper-medium-q5")), "For dictation, imports and meetings · Not installed");
  assert.equal(localModelDetail(row(rows, "parakeet-tdt-0.6b-v3")), "For dictation · Not installed");
  assert.equal(localModelDetail(row(rows, "nemotron-3.5-asr-streaming-0.6b")), "For meetings, word by word · Installed");
});

test("no row's words contain its model id", () => {
  const rows = localModelRows(local("whisper-small-q5"), sherpa("parakeet-unified-en-0.6b", "nemotron-speech-streaming-en-0.6b"), LOCAL_ROUTE);
  for (const r of rows) {
    for (const words of [localModelLine(r), localModelDetail(r), localModelAction(r), r.loadedButGoesTo ?? ""]) {
      assert.ok(!words.toLowerCase().includes(r.id.toLowerCase()), `${r.id}: ${words}`);
    }
  }
});

const modelRow = (over: Partial<LocalModelRow> = {}): LocalModelRow => ({
  id: "whisper-small-q5",
  name: "Whisper Small (Q5)",
  engine: "whisper",
  usedFor: "dictation",
  languages: "many",
  sizeMb: 488,
  installed: true,
  recommended: false,
  inUse: false,
  loadedButGoesTo: null,
  hint: null,
  verifiedBy: null,
  ...over,
});

test("a model's chips run size, languages, recommended, hint, then whether it is installed", () => {
  assert.deepEqual(
    localModelChips(modelRow({ recommended: true, hint: "slower; best for imports and meetings" })),
    [
      { text: "488 MB", tone: "plain" },
      { text: "Many languages", tone: "plain" },
      { text: "recommended", tone: "recommended" },
      { text: "slower; best for imports and meetings", tone: "plain" },
      { text: "Installed", tone: "installed" },
    ],
  );
});

test("a sherpa model's chips say what it is for; a Whisper model serves all of it and does not", () => {
  const purposes = (over: Partial<LocalModelRow>) => localModelChips(modelRow(over)).map((chip) => chip.text);
  assert.ok(purposes({ engine: "parakeet", usedFor: "dictation" }).includes("For dictation"));
  assert.ok(purposes({ engine: "parakeet", usedFor: "meetings" }).includes("For meetings, word by word"));
  assert.deepEqual(purposes({ engine: "whisper" }), ["488 MB", "Many languages", "Installed"]);
});

test("an uninstalled model says so without the mint, and a model with no known size has no size chip", () => {
  assert.deepEqual(localModelChips(modelRow({ installed: false, sizeMb: null })), [
    { text: "Many languages", tone: "plain" },
    { text: "Not installed", tone: "plain" },
  ]);
});

test("a model loaded while dictation goes elsewhere ends on that warning", () => {
  const chips = localModelChips(modelRow({ loadedButGoesTo: "OpenAI" }));
  assert.deepEqual(chips.at(-1), { text: "Loaded, but dictation goes to OpenAI", tone: "warn" });
});

test("a model's thumbnail follows its engine and purpose and does not change between renders", () => {
  assert.equal(modelThumb(modelRow()).kind, "breath");
  assert.equal(modelThumb(modelRow({ engine: "parakeet", usedFor: "dictation" })).kind, "dusk");
  assert.equal(modelThumb(modelRow({ engine: "parakeet", usedFor: "meetings" })).kind, "streak");
  assert.deepEqual(modelThumb(modelRow({ id: "a" })), modelThumb(modelRow({ id: "a" })));
  for (const id of ["a", "bb", "whisper-large-v3-turbo-q5", "kb-whisper-large-q5"]) {
    const breath = modelThumb(modelRow({ id }));
    assert.ok(breath.variant >= 0 && breath.variant < 5);
    const dusk = modelThumb(modelRow({ id, engine: "parakeet" }));
    assert.ok(dusk.variant >= 0 && dusk.variant < 2);
  }
});
