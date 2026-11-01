import { useEffect, useRef, useState } from "react";

import { splitQuestion } from "../lib/question";

type Request = { message: string; confirm: string; destructive: boolean; resolve: (yes: boolean) => void };

let show: ((request: Request) => void) | null = null;

/**
 * Ask a yes-or-no question in the application's own dialog, and resolve to
 * whether the answer was yes.
 *
 * It replaces `window.confirm`, which drew the operating system's grey box --
 * no design, an "OK" that names no action -- at exactly the moments that carry
 * the most weight: discarding a recording, deleting a note forever, emptying
 * Trash. The question is the message's first sentence, set as the dialog's
 * title; the sentences after it are the detail. `confirm` names the action,
 * and `destructive` draws it in the error colour.
 *
 * Like `window.confirm` it holds the caller until it is answered, so every
 * caller's re-check after the question (Meetings' "is this still the meeting
 * I asked about") still applies. A window that mounts no `ConfirmHost` falls
 * back to `window.confirm`, so a caller never waits on a dialog that is not
 * there.
 */
export function askConfirm(message: string, options: { confirm: string; destructive?: boolean }): Promise<boolean> {
  const present = show;
  if (present === null) return Promise.resolve(window.confirm(message));
  return new Promise((resolve) => present({ message, confirm: options.confirm, destructive: options.destructive ?? false, resolve }));
}

/**
 * The one dialog `askConfirm` draws into, mounted once per window. A modal
 * `<dialog>`: focus is held inside it, Escape is a no, and focus starts on
 * Cancel, so an Enter pressed out of habit keeps what was about to go.
 * A second question while one is open answers the first one no.
 */
export function ConfirmHost() {
  const [request, setRequest] = useState<Request | null>(null);
  const current = useRef<Request | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    show = (next) => {
      current.current?.resolve(false);
      current.current = next;
      setRequest(next);
    };
    return () => { show = null; };
  }, []);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (request && !element.open) {
      element.showModal();
      cancel.current?.focus();
    }
    if (!request && element.open) element.close();
  }, [request]);

  const answer = (yes: boolean) => {
    const asked = current.current;
    current.current = null;
    setRequest(null);
    asked?.resolve(yes);
  };

  const { title, detail } = splitQuestion(request?.message ?? "");
  return (
    <dialog
      ref={dialog}
      className="confirm-dialog"
      aria-labelledby="confirm-dialog-title"
      aria-describedby={detail ? "confirm-dialog-detail" : undefined}
      onCancel={(event) => { event.preventDefault(); answer(false); }}
    >
      <h2 id="confirm-dialog-title" className="confirm-dialog-title">{title}</h2>
      {detail ? <p id="confirm-dialog-detail" className="confirm-dialog-detail">{detail}</p> : null}
      <div className="confirm-dialog-actions">
        <button ref={cancel} type="button" onClick={() => answer(false)}>Cancel</button>
        <button type="button" className={request?.destructive ? "danger" : "primary"} onClick={() => answer(true)}>{request?.confirm}</button>
      </div>
    </dialog>
  );
}
