/**
 * People on a note (KNEG-362): reading a note's stored `participants`, what a
 * person is shown by, and the add and remove its meta row makes.
 *
 * `Note.participants` is a JSON array of `{displayName, email}`, kept raw and
 * read leniently, as Rust reads it (`notes_core::people`): a value that does
 * not parse is no people rather than an error, and an entry that is not an
 * object, or has neither a name nor an email, is left out. The list is sent
 * back whole by `setNoteParticipants`, and Rust checks and tidies it -- an
 * email that isn't one comes back in plain words, to be shown as it is.
 *
 * Pure, with no React: `tests/people.test.ts` imports this directly.
 */
import type { Person } from "../generated/ipc-types.js";

export type Participant = { displayName: string | null; email: string | null };

/** `value` trimmed, when it is text with something left; `null` otherwise. */
function text(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export function readParticipants(json: string | null): Participant[] {
  if (json === null) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const people: Participant[] = [];
  for (const entry of parsed) {
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) continue;
    const { displayName, email } = entry as { displayName?: unknown; email?: unknown };
    const participant = { displayName: text(displayName), email: text(email) };
    if (participant.displayName !== null || participant.email !== null) people.push(participant);
  }
  return people;
}

/** What a note's person is shown by: their name, or their email where they have none. */
export function participantLabel(participant: Participant): string {
  return participant.displayName ?? participant.email ?? "";
}

/** What a person in the People list is shown by: their name, or their email. */
export function personLabel(person: Person): string {
  return person.name ?? person.email;
}

/**
 * An email as Rust tells people apart: trimmed and ASCII lower-case (SQLite's
 * `lower()` folds nothing else), so "Anna@" and "anna@" are one person here too.
 */
export function emailKey(email: string): string {
  return email.trim().replace(/[A-Z]/g, (letter) => letter.toLowerCase());
}

/**
 * `list` with one more person, or why not. An email is needed, and one already
 * on the note is said so rather than sent: Rust would drop the second entry in
 * silence. Whether it is an email at all is left to Rust, which says so.
 */
export function addParticipant(list: readonly Participant[], email: string, name: string): { list: Participant[] } | { error: string } {
  const address = email.trim();
  if (address === "") return { error: "Enter an email address." };
  if (list.some((participant) => participant.email !== null && emailKey(participant.email) === emailKey(address))) {
    return { error: `${address} is already on this note.` };
  }
  return { list: [...list, { displayName: text(name), email: address }] };
}

/** `list` without the person at `index`. */
export function removeParticipant(list: readonly Participant[], index: number): Participant[] {
  return list.filter((_, at) => at !== index);
}

/** A count of notes as a screen reader hears it after a person's name. */
export function noteCount(count: number): string {
  return count === 1 ? "1 note" : `${count} notes`;
}
