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
 * One entry per screen the rail can reach, `notes` excepted.
 *
 * `notes` is `null` rather than a page with an empty intro: the Notes screen is
 * the writing space itself -- the library down one side, and at the top of the
 * other the open note's own title field, or "Your writing space" when no note
 * is open. A page title above that would push the note down to name a screen
 * the rail already marks as current.
 *
 * Every title is the rail's current label for that screen (`lib/navigation.ts`),
 * not a new name -- a screen that read one thing in the rail and another at the
 * top of its own content would be telling a colleague there are two screens
 * where there is one. A rename changes both together.
 *
 * One exception, for the opposite reason: `meeting-settings` is "Meetings" in
 * the rail, where it sits inside Settings and cannot be mistaken for anything
 * else, and "Meeting settings" here, because a header reading "Meetings" on
 * both it and the recording page would be telling a colleague there is one
 * screen where there are two.
 *
 * `satisfies` rather than a `: Record<ViewId, Page | null>` annotation: it
 * still checks every key against `ViewId` -- so a screen added to the rail
 * without an entry here is a compile error -- without widening this constant's
 * type to the annotation.
 */
export const PAGES = {
  dictation: {
    title: "Dictation",
    intro: "Speak, and your words are typed where your cursor is.",
  },
  meetings: {
    title: "Meetings",
    intro: "Record a meeting and get a transcript you can turn into notes.",
  },
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
