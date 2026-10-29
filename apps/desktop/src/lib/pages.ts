import type { ViewId } from "./navigation.js";

/**
 * A screen's title and the one sentence that says what it is for.
 *
 * `intro` is deliberately one sentence: it is what a colleague who is not
 * technical reads before anything else on the screen, and a paragraph in that
 * slot is a paragraph nobody reads. Anything longer belongs behind a
 * `LearnMore` disclosure instead of here.
 */
export type Page = { title: string; intro: string };

/**
 * One entry per screen the pills or the Settings sheet can reach, `dictation`,
 * `meetings` and `notes` excepted.
 *
 * `notes` is `null` rather than a page with an empty intro: the Notes screen is
 * the writing space itself -- the library down one side, and at the top of the
 * other the open note's own title field, or "Your writing space" when no note
 * is open. A page title above that would push the note down to name a screen
 * the title bar's Notes pill already marks as current.
 *
 * `dictation` and `meetings` are `null` because each page is a task hero,
 * which already says what the page is for, beside a list panel titled with
 * the page's name. The intro sentences that used to sit here moved into the
 * heroes: "Speak, and your words are typed where your cursor is." under
 * Dictation's shortcut (`views/Dictation.tsx`), and Meetings' "Record a
 * meeting and get a transcript you can turn into notes." as the canvas'
 * "You get a transcript and a summary, saved to Notes." under "Record a
 * meeting" -- without "and a summary" while automatic summaries are off
 * (`views/Meetings.tsx`).
 *
 * Every title is the screen's current label in `lib/navigation.ts` -- the one
 * the pills or the Settings sheet show -- not a new name: a screen that read
 * one thing there and another at the top of its own content would be telling
 * a colleague there are two screens where there is one. A rename changes both
 * together.
 *
 * One exception, for the opposite reason: `meeting-settings` is "Meetings" in
 * the Settings sheet, where it sits under the sheet's own "Settings" title and
 * cannot be mistaken for anything else, and "Meeting settings" here, because a
 * header reading "Meetings" on both it and the recording page would be telling
 * a colleague there is one screen where there are two.
 *
 * `satisfies` rather than a `: Record<ViewId, Page | null>` annotation: it
 * still checks every key against `ViewId` -- so a screen added to the navigation
 * without an entry here is a compile error -- without widening this constant's
 * type to the annotation.
 */
export const PAGES = {
  dictation: null,
  meetings: null,
  notes: null,
  vocabulary: {
    title: "Vocabulary",
    intro: "Names and terms that help transcription spell them right.",
  },
  "notes-upload": {
    title: "Import audio or video",
    intro: "Turn a recording into a note with a transcript.",
  },
  shortcuts: {
    title: "Shortcuts",
    intro: "Choose the keys that start dictation and the other actions.",
  },
  "meeting-settings": {
    title: "Meeting settings",
    intro: "Choose how meetings are transcribed, detected and labelled.",
  },
  providers: {
    title: "Providers",
    intro: "API keys for cloud services. You only need one if you use a cloud service.",
  },
  transcription: {
    title: "Transcription",
    intro: "Choose where speech becomes text: on this computer or with a cloud service.",
  },
  intelligence: {
    title: "Cleanup & summaries",
    intro: "Tidy dictated text and summarise meetings with an AI model.",
  },
  "local-llm": {
    title: "Local text models",
    intro: "Run an AI text model on this computer for cleanup and summaries.",
  },
  calendar: {
    title: "Calendar",
    intro: "Connect a calendar account.",
  },
  api: {
    title: "Local API",
    intro: "An HTTP interface on this computer for other programs.",
  },
  cli: {
    title: "CLI",
    intro: "Use KnegisWhisper from a terminal.",
  },
  mcp: {
    title: "MCP",
    intro: "Let AI assistants that speak MCP work with your notes, transcripts and dictionary.",
  },
  diagnostics: {
    title: "Diagnostics",
    intro: "Technical details for troubleshooting.",
  },
} satisfies Record<ViewId, Page | null>;
