import { useEffect, useRef, useState } from "react";

import { getNote, setNoteParticipants } from "../lib/ipc";
import { addParticipant, participantLabel, readParticipants, removeParticipant, type Participant } from "../lib/people";
import { message } from "../lib/useRefresh";
import { Icon } from "./Icon";

/**
 * A note's people, as one item of its meta row (KNEG-362): each person a chip
 * with a button that removes them, then Add person, which opens Email and an
 * optional Name under the row. Always shown, so a note with nobody on it can
 * be given someone; the owner's answer was people added here, by email.
 *
 * The list is the note's stored `participants`, sent back whole on each
 * change. Rust checks and tidies it, and what it refuses -- an email that
 * isn't one -- stays in view under the row, the form still open with what was
 * typed, until the next change. After a save the list is read back from Rust,
 * so it shows what was stored (an email lower-cased, a second entry for one
 * email dropped) even for a note the library's list doesn't hold, and
 * `onSaved` reloads the library, whose People list counts the change.
 */
export function NotePeople({ noteId, participants, onSaved }: {
  noteId: number;
  /** `Note.participants`, as stored. */
  participants: string | null;
  onSaved: () => Promise<void>;
}) {
  const [list, setList] = useState(() => readParticipants(participants));
  useEffect(() => { setList(readParticipants(participants)); }, [participants]);
  const [adding, setAdding] = useState(false);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const addButton = useRef<HTMLButtonElement>(null);
  const emailField = useRef<HTMLInputElement>(null);
  useEffect(() => { if (adding) emailField.current?.focus(); }, [adding]);
  const formId = `note-${noteId}-add-person`;

  const save = async (next: Participant[]): Promise<boolean> => {
    setBusy(true); setError(null);
    try {
      if (!await setNoteParticipants(noteId, next)) throw new Error("This note no longer exists.");
      const stored = await getNote(noteId);
      setList(readParticipants(stored?.participants ?? null));
      await onSaved();
      return true;
    } catch (cause) {
      setError(message(cause));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const close = () => { setAdding(false); setEmail(""); setName(""); setError(null); addButton.current?.focus(); };

  return (
    <li className="note-people" data-open={adding || undefined} data-error={error !== null || undefined}>
      <Icon name="meetings" size={15} />
      {list.length === 0 ? null : (
        <ul className="note-people-list" aria-label="People">
          {list.map((participant, index) => {
            const label = participantLabel(participant);
            return (
              <li key={`${index}:${participant.email ?? ""}:${participant.displayName ?? ""}`} className="note-person"
                // The address behind a name, for telling two people of one name apart.
                title={participant.displayName !== null && participant.email !== null ? participant.email : undefined}>
                <span className="note-person-name">{label}</span>
                <button type="button" className="note-person-remove" aria-label={`Remove ${label}`} title={`Remove ${label}`} disabled={busy}
                  // The chip goes with its button, so focus moves to Add person rather than to the page.
                  onClick={() => { void save(removeParticipant(list, index)).then(saved => { if (saved) addButton.current?.focus(); }); }}>
                  <Icon name="close" size={12} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <button ref={addButton} type="button" className="note-people-add" aria-expanded={adding} aria-controls={adding ? formId : undefined}
        onClick={() => { if (adding) close(); else { setError(null); setAdding(true); } }}>
        <Icon name="plus" size={14} />Add person
      </button>
      {adding ? (
        // `noValidate`: whether the address is one is Rust's to say, in its own words.
        <form id={formId} className="note-people-form" noValidate aria-label="Add person"
          onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); close(); } }}
          onSubmit={event => {
            event.preventDefault();
            const added = addParticipant(list, email, name);
            if ("error" in added) { setError(added.error); return; }
            void save(added.list).then(saved => { if (saved) close(); });
          }}>
          <label>Email<input ref={emailField} type="email" autoComplete="off" value={email} onChange={event => setEmail(event.target.value)} /></label>
          <label>Name (optional)<input type="text" autoComplete="off" value={name} onChange={event => setName(event.target.value)} /></label>
          <span className="note-people-form-actions">
            <button type="submit" className="secondary" disabled={busy || !email.trim()}>Add</button>
            <button type="button" onClick={close}>Cancel</button>
          </span>
        </form>
      ) : null}
      {error === null ? null : <p className="note-people-error error" role="alert">{error}</p>}
    </li>
  );
}
