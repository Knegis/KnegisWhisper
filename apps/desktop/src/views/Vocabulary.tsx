import { useSettingsDraft, useSettingsNavigation } from "../components/SettingsDrafts";
import { useCallback, useEffect, useRef, useState } from "react";
import { getVocabulary, updateVocabulary, editVocabulary, deleteVocabulary, restoreVocabulary, reviewVocabulary, getDictationRoute, getLocalStatus, getSherpaStatus, getMeetingStatus, getMeetingProviders, type VocabularyReview } from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

function Usage({ title, review }: { title: string; review: VocabularyReview }) {
  return <section className="vocabulary-usage">
    <h3>{title}{review.model ? ` · ${review.model}` : ""}</h3>
    <p>{review.supported ? `${review.included.length} of ${review.included.length + review.omitted.length} terms included` : "Not supported by this integration"}</p>
    {review.supported && <p className="muted">{review.limit}</p>}
    {review.omitted.length > 0 && <details><summary>{review.omitted.length} omitted terms</summary><ul>{review.omitted.map(item => <li key={item.word}><strong>{item.word}</strong> — {item.reason}</li>)}</ul></details>}
  </section>;
}

export function Vocabulary() {
  const [meetingPreview, setMeetingPreview] = useState<string | null>(null);
  const load = useCallback(async () => {
    const [words, route, local, sherpa, meeting, providers] = await Promise.all([getVocabulary(), getDictationRoute(), getLocalStatus(), getSherpaStatus(), getMeetingStatus(), getMeetingProviders()]);
    const provider = meetingPreview ?? meeting.provider ?? "local";
    const [dictationReview, meetingReview] = await Promise.all([
      reviewVocabulary(route.isLocal && !sherpa.selected ? "whisper" : "unsupported", route.isLocal ? sherpa.selected ?? local.runningModel : null),
      reviewVocabulary(provider),
    ]);
    return { words, dictationReview, meetingReview, providers, provider, route, meeting };
  }, [meetingPreview]);
  const { data, error, reload, guard, fail } = useRefresh(load);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<number | null>(null);
  const [editText, setEditText] = useState("");
  const [removed, setRemoved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const replaceDraft = useSettingsNavigation();
  const persistAdd = async () => { const submitted = draft; await updateVocabulary([submitted], []); setDraft(current => current === submitted ? "" : current); await reload(); };
  const editNow = useRef(editText); editNow.current = editText;
  const persistEdit = async () => { if (editing === null) return; const submitted = editText; await editVocabulary(editing, submitted); if (editNow.current === submitted) setEditing(null); await reload(); };
  const addGuard = useSettingsDraft({ label: "New vocabulary term", dirty: draft.length > 0, save: persistAdd, discard: () => setDraft("") });
  const editGuard = useSettingsDraft({ label: "Vocabulary edit", order: 1, dirty: editing !== null && editText !== data?.words.find(word => word.id === editing)?.word, save: persistEdit, discard: () => setEditing(null) });
  const act = async (work: () => Promise<void>) => { if (busy) return; setBusy(true); try { await guard(work); } finally { setBusy(false); } };
  useEffect(() => { const refresh = () => { void reload(); }; window.addEventListener("focus", refresh); return () => window.removeEventListener("focus", refresh); }, [reload]);
  const saveEdit = () => editGuard.run(persistEdit).catch(cause => { fail(cause); });
  return <div className="view vocabulary-view" data-surface="vocabulary">{addGuard.feedback}{editGuard.feedback}
    <section>
      <p>Add names, words and phrases you want transcription to recognize. Saved terms apply automatically where supported.</p>
      <p className="muted">Hints do not guarantee spelling or train the model. Keep the list focused on important names and terminology. Local Whisper keeps them on this device. Supported cloud providers receive applicable terms. Changes apply to the next dictation request or new meeting.</p>
      {error && <p role="alert" className="error">{error}</p>}
      <form className="controls vocabulary-add" onSubmit={event => { event.preventDefault(); void addGuard.run(persistAdd).catch(fail); }}>
        <input aria-label="Word or phrase" placeholder="e.g. KnegisWhisper or Åsa Karlsson" value={draft} onChange={event => setDraft(event.target.value)} maxLength={100} disabled={busy} />
        <button type="submit" disabled={busy || !draft.trim()}>Add</button>
      </form>
      <input type="search" aria-label="Search vocabulary" placeholder="Search vocabulary" value={query} onChange={event => setQuery(event.target.value)} />
      {removed !== null && <p role="status">Removed “{removed}”. <button type="button" disabled={busy} onClick={() => { void act(async () => { await restoreVocabulary(); setRemoved(null); }); }}>Undo</button></p>}
      {!data ? <p>Loading vocabulary…</p> : <>
        <p className="muted">{data.words.length} saved terms</p>
        <ul className="vocabulary-list">{data.words.filter(word => word.word.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map(word => <li key={word.id}>
          {editing === word.id ? <>
            <input aria-label="Edit word or phrase" value={editText} autoFocus maxLength={100} disabled={busy} onChange={event => setEditText(event.target.value)} onKeyDown={event => { if (event.key === "Escape") setEditing(null); if (event.key === "Enter") { event.preventDefault(); void saveEdit(); } }} />
            <button type="button" disabled={busy || !editText.trim()} onClick={() => { void saveEdit(); }}>Save</button>
            <button type="button" disabled={busy} onClick={() => setEditing(null)}>Cancel</button>
          </> : <>
            <span className="vocabulary-word">{word.word}</span>
            <button type="button" disabled={busy} aria-label={`Edit ${word.word}`} onClick={() => { void replaceDraft(() => { setEditing(word.id); setEditText(word.word); }); }}>Edit</button>
            <button type="button" disabled={busy} aria-label={`Delete ${word.word}`} onClick={() => { void act(async () => { await deleteVocabulary(word.id); setRemoved(word.word); }); }}>Delete</button>
          </>}
        </li>)}</ul>
        {data.words.length === 0 && <p>No vocabulary yet. Add a name or phrase above.</p>}
      </>}
    </section>
    {data && <div className="vocabulary-support">
      <Usage title="Next dictation" review={data.dictationReview} />
      <section><label>Meeting provider preview <select aria-label="Vocabulary meeting provider" value={data.provider} onChange={event => setMeetingPreview(event.target.value)}>{data.providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}</select></label>
        <p className="muted">Preview only; this does not change your meeting provider.{data.meeting.provider ? " An active meeting retains its original vocabulary until it ends." : ""}</p>
        <Usage title="Next meeting" review={data.meetingReview} />
      </section>
    </div>}
  </div>;
}
