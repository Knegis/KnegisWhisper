#!/usr/bin/env node
import { waitFor, reload, pressText } from "./interactions.mjs";
/**
 * Measures, against dev fixtures, people in Notes (KNEG-362): adding a person
 * to a note on its meta row, the library's People list counting them, pressing
 * them to narrow the library to their notes, Show all notes bringing the tree
 * back, removing them again, a refused email staying in view, and the people
 * item at 720x520.
 *
 * Emails and names are typed with CDP's `Input.insertText` into the focused
 * field, and every button pressed with a real pointer press. The page's
 * `invoke` is wrapped (`installInvokeStub`) to log what reached
 * `notes_set_participants` and `notes_list_summaries`. The fixtures answer as Rust's
 * `notes_core::people` does -- checked, tidied, refused in the same words --
 * but which emails Rust accepts, how it counts and names people, and that it
 * filters by email in any case are pinned by `cargo test` (`people.rs`,
 * `commands.rs`), not here.
 *
 * Checks 1 to 4 are one sequence from one fresh load, as a person would go
 * through them; 5 and 6 each start from a fresh load.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseShellArgs, shellUsage } from "./args.mjs";
import { launchEdge, killBrowser } from "./browser.mjs";
import { connectCdp } from "./cdp.mjs";
import { createReport, addCheck, failedChecks, renderMarkdown } from "./report.mjs";
import * as snippets from "./snippets.mjs";
import { isEntryPoint } from "../ci/entry.mjs";

const delay = (ms) => new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
const isMain = isEntryPoint(import.meta.url);

const NAV_PILL = "button.nav-pill";
const WINDOW = { width: 1000, height: 720 };
const SMALL = { width: 720, height: 520 };
/** Fixture notes (`apps/desktop/src/lib/devFixtures.ts`): 103 has nobody on it, 101 has two people. */
const SCRATCH = { id: 103, title: "Scratch" };
const WEEKLY = "Weekly sync — platform";
const MAJA = { name: "Maja Holm", email: "maja.holm@example.com" };
/** The fixtures' people before anyone is added: each on one note, ordered by name. */
const FIXTURE_PEOPLE = ["Anna Lindqvist", "Jonas Berg", "Ravi Menon"];
/** Rust's refusal for an address that isn't one (`people::PeopleError::NotAnEmail`). */
const REFUSED = "'maja@' isn't an email address.";

const field = (snippet, path) => `(${snippet})?.${path}`;
const noteOpen = (title) => `(() => { const input = document.querySelector('.note-detail textarea[aria-label="Title"]'); return Boolean(input) && !input.disabled && input.value === ${JSON.stringify(title)}; })()`;
const FOCUS = "document.activeElement?.className ?? null";





async function viewport(cdp, { width, height }) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
}

/** A real pointer press at the centre of the element matching `selector` and reading `text` (any, when null), which must be on screen. */


const state = (cdp) => cdp.evaluate(snippets.peopleState());
const names = (people) => people.map((person) => person.name);

/** A fresh load at 1000x720 with the shell drawn, the fixture banner gone and the invoke stub installed. */
async function freshShell(cdp) {
  await viewport(cdp, WINDOW);
  await reload(cdp);
  await waitFor(cdp, "Boolean(document.querySelector('.app-titlebar') && document.querySelector('.split-main, .content'))", "the fixture shell");
  await cdp.evaluate(snippets.removeFixtureBanner());
  await cdp.evaluate(snippets.installInvokeStub());
}

/** Notes from its pill, then the note titled `title` from its library, with its people item drawn. */
async function openNote(cdp, title) {
  await cdp.evaluate(snippets.clickText(NAV_PILL, "Notes"));
  await waitFor(cdp, "Boolean(document.querySelector('.list-panel .notes-entry'))", "the Notes library");
  await cdp.evaluate(snippets.openNotesEntry(title));
  await waitFor(cdp, noteOpen(title), `note "${title}" to open`);
  await waitFor(cdp, field(snippets.peopleState(), "note"), "the note's people item");
  await delay(200);
}

/** Add person, the email typed into the field it focuses, the name into Name, then Add. */
async function addPerson(cdp, email, name) {
  await pressText(cdp, ".note-people-add", "Add person");
  await waitFor(cdp, "document.activeElement?.matches('.note-people-form input[type=\"email\"]') ?? false", "the Email field to take focus");
  await cdp.send("Input.insertText", { text: email });
  if (name !== "") {
    await pressText(cdp, ".note-people-form input[type=\"text\"]", null);
    await cdp.send("Input.insertText", { text: name });
  }
  await pressText(cdp, ".note-people-form button[type=\"submit\"]", "Add");
}

const lastCall = async (cdp, command) => (await cdp.evaluate(snippets.stubCalls(command))).at(-1) ?? null;
const storedParticipants = async (cdp, id) => (await cdp.evaluate(snippets.invoke("notes_get", { id })))?.participants ?? null;

const SETUP_NOTE = "1000x720 unless stated. Checks 1-4 run in sequence from one fresh load; 5 and 6 each start from a fresh load. Text is typed with CDP Input.insertText into the focused field; buttons are pressed with real CDP pointer presses, a People row after scrolling it into view as a person scrolls the panel. invoke is wrapped to log notes_set_participants and notes_list_summaries.";

export async function runPeople(options) {
  const out = resolve(options.out);
  await mkdir(out, { recursive: true });
  let browser;
  let cdp;
  try {
    browser = await launchEdge({ ...options, out });
    cdp = await connectCdp(browser.target.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    const report = createReport({
      url: options.url,
      edgeVersion: browser.version.Browser ?? browser.version,
      fixtureBannerRemoved: true,
      timestamp: new Date().toISOString(),
      measurementScope: "measured in headless Edge against dev fixtures, not in the Tauri window",
    });

    /** Runs one check; a step that throws is the check measured false, with the people state as it was. */
    const check = async ({ id, claim, expectation }, run) => {
      try {
        const result = await run();
        addCheck(report, { id, claim, expectation, note: SETUP_NOTE, ...result });
      } catch (error) {
        let people = null;
        try { people = await state(cdp); } catch { /* no page to read */ }
        addCheck(report, { id, claim, expectation, measured: { failedAt: error.message, people }, matches: false, note: SETUP_NOTE });
      }
    };

    let tree = null;

    await check({
      id: "1",
      claim: "A person added on a note's meta row shows as a chip there, and the library's People list counts them once.",
      expectation: [
        `fixture note ${SCRATCH.id} ("${SCRATCH.title}", nobody on it): Add person, "${MAJA.email}" in Email and "${MAJA.name}" in Name, Add;`,
        `notes_set_participants sent { id: ${SCRATCH.id}, participants: [{ displayName: "${MAJA.name}", email: "${MAJA.email}" }] };`,
        `one chip "${MAJA.name}" with a button "Remove ${MAJA.name}", the form closed, no refusal, focus back on Add person;`,
        `People lists ${JSON.stringify([...FIXTURE_PEOPLE.slice(0, 2), MAJA.name, FIXTURE_PEOPLE[2]])}, "${MAJA.name}" with 1 and none pressed`,
      ].join(" "),
    }, async () => {
      await freshShell(cdp);
      await openNote(cdp, SCRATCH.title);
      const before = await state(cdp);
      tree = await cdp.evaluate(snippets.notesFolders());
      await cdp.evaluate(snippets.clearStubCalls());
      await addPerson(cdp, MAJA.email, MAJA.name);
      await waitFor(cdp, `${field(snippets.peopleState(), "people")}?.some((person) => person.name === ${JSON.stringify(MAJA.name)})`, "the People list to name the new person");
      const after = await state(cdp);
      const sent = await lastCall(cdp, "notes_set_participants");
      const focus = await cdp.evaluate(FOCUS);
      const shot = "01-person-added-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      const maja = after.people.find((person) => person.name === MAJA.name);
      return {
        measured: { before, after, sent, focus },
        matches: before.note.chips.length === 0
          && JSON.stringify(names(before.people)) === JSON.stringify(FIXTURE_PEOPLE)
          && JSON.stringify(sent) === JSON.stringify({ id: SCRATCH.id, participants: [{ displayName: MAJA.name, email: MAJA.email }] })
          && JSON.stringify(after.note.chips) === JSON.stringify([{ name: MAJA.name, remove: `Remove ${MAJA.name}` }])
          && !after.note.formOpen && after.note.error === null
          && typeof focus === "string" && focus.includes("note-people-add")
          && JSON.stringify(names(after.people)) === JSON.stringify([...FIXTURE_PEOPLE.slice(0, 2), MAJA.name, FIXTURE_PEOPLE[2]])
          && maja?.count === 1 && after.people.every((person) => !person.pressed),
        screenshots: [shot],
      };
    });

    await check({
      id: "2",
      claim: "Pressing a person in People shows only their notes, fetched by their email, in place of the folders.",
      expectation: [
        `press "${MAJA.name}" in People: notes_list_summaries sent with person "${MAJA.email}";`,
        `a section "notes with ${MAJA.name}" holding exactly ["${SCRATCH.title}"] and the action "Show all notes";`,
        "no folder groups and no folders or not in a folder heading; the person shown pressed"
      ].join(" "),
    }, async () => {
      await cdp.evaluate(snippets.clearStubCalls());
      // People sits under the whole tree, below the panel's fold at 1000x720 with the fixtures' notes.
      const scrolled = await cdp.evaluate(snippets.scrollToText(".list-panel button.notes-person .notes-person-name", MAJA.name));
      if (!scrolled) throw new Error(`No People row reads ${MAJA.name}`);
      await delay(150);
      await pressText(cdp, ".list-panel button.notes-person .notes-person-name", MAJA.name);
      await waitFor(cdp, `${field(snippets.peopleState(), "filtered")}?.titles.length > 0`, "the person's notes");
      const after = await state(cdp);
      const asked = await cdp.evaluate(snippets.stubCalls("notes_list_summaries"));
      const shot = "02-notes-with-person-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, asked },
        matches: asked.some((args) => args?.person === MAJA.email && args?.folderId === null && args?.noteType === null)
          && after.filtered?.label === `notes with ${MAJA.name}`
          && JSON.stringify(after.filtered?.titles) === JSON.stringify([SCRATCH.title])
          && JSON.stringify(after.filtered?.actions) === JSON.stringify(["Show all notes"])
          && after.folders === 0 && !after.sections.includes("folders") && !after.sections.includes("not in a folder")
          && after.people.find((person) => person.name === MAJA.name)?.pressed === true,
        screenshots: [shot],
      };
    });

    await check({
      id: "3",
      claim: "Show all notes brings the library's tree back as it was.",
      expectation: "press Show all notes: no \"notes with\" section; the folders, not in a folder and people headings back in that order, People under the whole tree; the folders and their notes as before check 2 (notesFolders); nobody pressed; the open note unchanged",
    }, async () => {
      await cdp.evaluate(snippets.scrollToText(".list-panel .panel-action", "Show all notes"));
      await delay(150);
      await pressText(cdp, ".list-panel .panel-action", "Show all notes");
      await waitFor(cdp, `${field(snippets.peopleState(), "folders")} > 0`, "the folders to come back");
      const after = await state(cdp);
      const restored = await cdp.evaluate(snippets.notesFolders());
      const title = await cdp.evaluate("document.querySelector('.note-detail textarea[aria-label=\"Title\"]')?.value ?? null");
      return {
        measured: { after, restored, before: tree, title },
        matches: after.filtered === null
          && JSON.stringify(after.sections.filter((label) => label !== null)) === JSON.stringify(["folders", "not in a folder", "people"])
          && JSON.stringify(restored) === JSON.stringify(tree)
          && after.people.every((person) => !person.pressed)
          && title === SCRATCH.title,
      };
    });

    await check({
      id: "4",
      claim: "Removing the person takes their chip away, saves the note with nobody on it, and drops them from People.",
      expectation: [
        `press "Remove ${MAJA.name}": notes_set_participants sent { id: ${SCRATCH.id}, participants: [] };`,
        `no chips; the note's stored participants null (notes_get); People back to ${JSON.stringify(FIXTURE_PEOPLE)}; focus on Add person`,
      ].join(" "),
    }, async () => {
      await cdp.evaluate(snippets.clearStubCalls());
      await pressText(cdp, `.note-person-remove[aria-label="Remove ${MAJA.name}"]`, null);
      await waitFor(cdp, `${field(snippets.peopleState(), "people")}?.every((person) => person.name !== ${JSON.stringify(MAJA.name)})`, "the person to leave People");
      const after = await state(cdp);
      const sent = await lastCall(cdp, "notes_set_participants");
      const stored = await storedParticipants(cdp, SCRATCH.id);
      const focus = await cdp.evaluate(FOCUS);
      const shot = "04-person-removed-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, sent, stored, focus },
        matches: JSON.stringify(sent) === JSON.stringify({ id: SCRATCH.id, participants: [] })
          && after.note.chips.length === 0 && stored === null
          && JSON.stringify(names(after.people)) === JSON.stringify(FIXTURE_PEOPLE)
          && typeof focus === "string" && focus.includes("note-people-add"),
        screenshots: [shot],
      };
    });

    await check({
      id: "5",
      claim: "An email Rust refuses is said in its words under the row, the form stays open with what was typed, and nothing is saved.",
      expectation: `fixture note ${SCRATCH.id}: Add person, "maja@" in Email, Add; the item's alert reads "${REFUSED}"; the form open with "maja@" still in Email; no chips; stored participants null; People still ${JSON.stringify(FIXTURE_PEOPLE)}`,
    }, async () => {
      await freshShell(cdp);
      await openNote(cdp, SCRATCH.title);
      await addPerson(cdp, "maja@", "");
      await waitFor(cdp, `${field(snippets.peopleState(), "note")}?.error !== null`, "the refusal");
      const after = await state(cdp);
      const stored = await storedParticipants(cdp, SCRATCH.id);
      const shot = "05-refused-email-1000x720.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, stored },
        matches: after.note.error === REFUSED && after.note.formOpen && after.note.email === "maja@"
          && after.note.chips.length === 0 && stored === null
          && JSON.stringify(names(after.people)) === JSON.stringify(FIXTURE_PEOPLE),
        screenshots: [shot],
      };
    });

    await check({
      id: "6",
      claim: "At 720x520 a note's people, with Add person's form open, stay inside the window.",
      expectation: `fixture note "${WEEKLY}" (two people): Add person opened, then 720x520: two chips, the form open, the people item's right edge and every meta item inside the window, and nothing scrolling sideways`,
    }, async () => {
      await freshShell(cdp);
      await openNote(cdp, WEEKLY);
      await pressText(cdp, ".note-people-add", "Add person");
      await waitFor(cdp, field(snippets.peopleState(), "note.formOpen"), "the form to open");
      await viewport(cdp, SMALL);
      await delay(500);
      const after = await state(cdp);
      const meta = await cdp.evaluate(snippets.noteMetaState());
      const shot = "06-people-form-720x520.png";
      await cdp.screenshot(resolve(out, shot));
      return {
        measured: { after, meta },
        matches: after.note.chips.length === 2 && after.note.formOpen
          && after.note.right <= after.innerWidth && after.scrollWidth <= after.innerWidth
          && meta.items.every((item) => item.right <= meta.innerWidth),
        screenshots: [shot],
      };
    });

    const markdown = renderMarkdown(report);
    await writeFile(resolve(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    await writeFile(resolve(out, "report.md"), markdown);
    process.stdout.write(markdown);
    return report;
  } finally {
    try {
      await cdp?.close();
    } finally {
      await killBrowser(browser?.child);
    }
  }
}

if (isMain) {
  try {
    const options = parseShellArgs(process.argv.slice(2));
    if (options.help) process.stdout.write(`${shellUsage().replace("shell-title-bar.mjs", "people.mjs")}\n`);
    else {
      const failed = failedChecks(await runPeople(options));
      // The run completed; a check measured false still fails it, so a wrapper or CI step sees it.
      if (failed.length) {
        console.error(`people: ${failed.length} check(s) false: ${failed.join(", ")}`);
        process.exitCode = 1;
      }
    }
  } catch (error) {
    console.error(`people failed: ${error.message}`);
    process.exitCode = 1;
  }
}
