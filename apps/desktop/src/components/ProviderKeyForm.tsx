/**
 * A provider's key, entered where the provider is chosen (KNEG-572).
 *
 * Until this existed the only key form was on Settings > Providers, so picking
 * a cloud service anywhere else meant leaving the screen, saving a key there
 * and coming back to pick it again. This is that form on its own: one slot, a
 * password field, Save, and Clear once a key is stored.
 *
 * It holds to the Providers page's rules. Nothing reads a secret back, so the
 * field is never seeded and is emptied the moment the key is stored. A stored
 * key is "Key saved", never "connected": storing proves only that the
 * credential store worked. And it sends nothing: a key goes to its provider
 * when that provider is used, not when it is saved here.
 *
 * Providers keeps its own form. That one opens one row at a time, moves the
 * row between lists and resets the page's connection test, none of which a
 * form standing under a select has any use for.
 *
 * Not for the `custom` slot. Saving that key authorizes it for the server
 * addresses set at the time (`provider_credential_set`), which the Providers
 * page explains beside its form.
 */

import { useRef, useState } from "react";

import { setProviderCredential } from "../lib/ipc";
import { message } from "../lib/useRefresh";
import { FailureLine } from "./FailureLine";
import { useSettingsDraft } from "./SettingsDrafts";

export function ProviderKeyForm({
  slot,
  name,
  stored,
  onSaved,
}: {
  /** The credential slot, from the provider's `credentialSlot`. Never written here as a literal. */
  slot: string;
  /** The provider's name, for the field's label and the sentences about it. */
  name: string;
  /** Whether a key is stored now. The caller re-reads it in `onSaved`. */
  stored: boolean;
  /** Called once a key has been stored or removed, so the caller can re-read what it shows. */
  onSaved: () => void | Promise<void>;
}) {
  const [draft, setDraft] = useState("");
  /** A stored key's field stays shut until Change is pressed; an empty slot's is open. */
  const [changing, setChanging] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ text: string; detail: string } | null>(null);
  const draftNow = useRef(draft);
  draftNow.current = draft;

  const persist = async () => {
    const submitted = draftNow.current;
    const typed = submitted.trim();
    if (!typed) throw new Error("Type a key first. Use Clear to remove the stored one.");
    await setProviderCredential(slot, typed);
    // Only if nothing was typed while the write ran: that would be a second key.
    if (draftNow.current === submitted) setDraft("");
    setChanging(false);
    setSaid("Saved. Nothing is sent until this provider is used.");
    await onSaved();
  };
  const keyDraft = useSettingsDraft({
    label: `Key for ${name}`,
    dirty: draft.length > 0,
    save: persist,
    discard: () => {
      setDraft("");
      setChanging(false);
    },
  });

  const save = () => {
    setFailure(null);
    void keyDraft.run(persist).catch((cause) => setFailure({ text: "Couldn't save the key.", detail: message(cause) }));
  };
  const clear = () => {
    setFailure(null);
    void keyDraft
      .run(async () => {
        await setProviderCredential(slot, "");
        setDraft("");
        setChanging(false);
        setSaid("Removed. This key is no longer stored.");
        await onSaved();
      })
      .catch((cause) => setFailure({ text: "Couldn't remove the key.", detail: message(cause) }));
  };

  const open = !stored || changing;
  return (
    <div className="provider-key" data-slot={slot}>
      {keyDraft.feedback}
      <span className="controls provider-key-form">
        {stored ? (
          <span className="model-chip provider-state provider-state-saved" data-tone="installed">
            Key saved
          </span>
        ) : null}
        {open ? (
          <>
            <input
              type="password"
              autoComplete="off"
              aria-label={`API key for ${name}`}
              placeholder={stored ? "Replace the stored key" : `Paste a key for ${name}`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && draft.trim() !== "") save();
                if (e.key === "Escape" && stored) {
                  setDraft("");
                  setChanging(false);
                }
              }}
            />
            <button type="button" className="action-iris" disabled={draft.trim() === ""} onClick={save}>
              Save key
            </button>
            {stored ? (
              <button type="button" className="secondary" onClick={clear}>
                Clear
              </button>
            ) : null}
          </>
        ) : (
          <button type="button" className="secondary" aria-label={`Change the key for ${name}`} onClick={() => setChanging(true)}>
            Change
          </button>
        )}
      </span>
      {said ? (
        <em className="row-detail" role="status">
          {said}
        </em>
      ) : null}
      {failure ? <FailureLine text={failure.text} detail={failure.detail} /> : null}
    </div>
  );
}
