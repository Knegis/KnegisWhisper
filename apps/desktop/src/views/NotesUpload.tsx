/**
 * Notes > Upload.
 *
 * Two importers behind one screen, because they answer two different
 * questions someone arrives here with. "I have a recording" is audio and
 * video import, both going through the same ffmpeg/transcription
 * pipeline `media.rs` implements. "I already used OpenWhispr" is
 * `storage::import::from_openwhispr` — a one-way, read-only copy of an
 * existing `transcriptions.db` that this project has had since early in the
 * build and, until this screen, nobody could reach.
 *
 * Two things this screen refuses to do, because getting either of them wrong
 * would be worse than not building the screen at all. (There were three; the
 * third was about not overstating what protects yt-dlp's own fetch, and it
 * went when URL import did.)
 *
 * It does not show a successful-looking result for a failed import. Every
 * failure is the actual message Rust returned — `ffmpeg could not convert
 * this file`, `yt-dlp could not fetch that url`, `no transcription route is
 * configured` and `the configured transcription provider failed` all read
 * differently on screen because they are different problems with different
 * fixes, and collapsing them into "Import failed" would erase exactly the
 * distinction the backend went to the trouble of keeping.
 *
 * It does not report an OpenWhispr import as "done" without saying what
 * was left behind. `ImportReport` carries which columns were dropped from
 * which table and which credentials moved into this system's credential
 * store, and this screen renders all of it — a copy that silently discarded
 * rows would be the same defect this whole project has spent its run
 * correcting.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { importActive, importProgressText, importStages, pollImport } from "../lib/importProgress";
import { Icon } from "../components/Icon";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { destinationName } from "../lib/controlBar";
import {
  getDictationRoute,
  getMediaImportStatus,
  getMediaImportSnapshot,
  type ImportSnapshot,
  importAudioFile,
  importOpenWhispr,
  importVideoFile,
  installMediaImportSidecar,
  onMediaImportProgress,
  pickImportFile,
  type ImportReport,
  type SidecarStatus,
} from "../lib/ipc";
import {
  droppedColumnsByTable,
  leftBehindTableLabels,
  deletedLeftOutLabel,
  importReportSummary,
  mediaImportProgressLabel,
  mediaImportSourceLabel,
  relocatedCredentialLabel,
  sidecarUnavailableReason,
} from "../lib/labels";
import { message, useRefresh } from "../lib/useRefresh";

export function NotesUpload({ onOpenNote }: { onOpenNote: (id: number) => void }) {
  const mounted = useRef(true);
  const importPending = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const load = useCallback(async () => {
    const [sidecars, route] = await Promise.all([
      getMediaImportStatus(),
      getDictationRoute(),
    ]);
    return { sidecars, route };
  }, []);
  const { data, error, guard } = useRefresh(load);

  const [progress, setProgress] = useState<string | null>(null);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onMediaImportProgress((p) => {
      setProgress(mediaImportProgressLabel(p));
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // Audio/video import.
  const [localPick, setLocalPick] = useState<
    { kind: "audio" | "video"; path: string } | null
  >(null);
  const [pending, setPending] = useState(false);
  const [snapshot, setSnapshot] = useState<ImportSnapshot | null>(null);
  const [snapshotError, setSnapshotError] = useState<string | null>(null);
  const [mediaError, setMediaError] = useState<string | null>(null);
  useEffect(() => pollImport(getMediaImportSnapshot,
    (value) => { setSnapshot(value); setSnapshotError(null); },
    (error) => setSnapshotError(`Could not check import progress: ${message(error)}`)), []);
  const mediaBusy = pending || importActive(snapshot);
  const stages = importStages(snapshot);
  const result = snapshot?.stage === "succeeded" ? snapshot.result : null;
  const mediaResult = result ? { noteId: result.noteId, result } : null;

  // OpenWhispr database import.
  const [dbPath, setDbPath] = useState<string | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [importReport, setImportReport] = useState<ImportReport | null>(null);

  const chooseLocalFile = async (kind: "audio" | "video") => {
    const path = await pickImportFile(kind);
    if (path !== null) {
      setLocalPick({ kind, path });
      setMediaError(null);
      setMediaError(null);
    }
  };

  const runLocalImport = async () => {
    if (!localPick || importPending.current) return;
    importPending.current = true;
    setPending(true);
    setMediaError(null);
    try {
      if (localPick.kind === "audio") await importAudioFile(localPick.path);
      else await importVideoFile(localPick.path);
      if (mounted.current) setLocalPick(null);
    } catch (e) {
      if (mounted.current) setMediaError(message(e));
    } finally {
      importPending.current = false;
      if (mounted.current) setPending(false);
    }
  };

  const chooseDatabaseFile = async () => {
    const path = await pickImportFile("open-whispr-database");
    if (path !== null) {
      setDbPath(path);
      setImportReport(null);
      setImportError(null);
    }
  };

  const runDatabaseImport = async () => {
    if (!dbPath) return;
    setImportBusy(true);
    setImportError(null);
    setImportReport(null);
    try {
      setImportReport(await importOpenWhispr(dbPath));
    } catch (e) {
      setImportError(message(e));
    } finally {
      setImportBusy(false);
    }
  };

  if (!data) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const { sidecars, route } = data;
  const ffmpeg = sidecars.find((s) => s.sidecar === "ffmpeg") ?? null;
  const ffmpegReady = ffmpeg?.installed === true;

  const sidecarRow = (
    label: string,
    status: SidecarStatus | null,
    onInstall: () => Promise<void>,
  ) => {
    if (!status) return null;
    const unavailable = sidecarUnavailableReason(status.availability);
    return (
      <div className="row" key={status.sidecar}>
        <span className="row-label">{label}</span>
        <span className="row-value controls">
          {status.installed ? (
            <span className="status-ok"><span className="status-dot" aria-hidden="true" />Installed</span>
          ) : unavailable ? (
            <span>Not available</span>
          ) : (
            <button
              type="button"
              disabled={mediaBusy}
              onClick={() => {
                setProgress(`Installing ${status.sidecar}…`);
                void guard(async () => {
                  try {
                    await onInstall();
                  } finally {
                    setProgress(null);
                  }
                });
              }}
            >
              Install {status.sidecar}
            </button>
          )}
          {unavailable ? <em className="row-detail">{unavailable}</em> : null}
        </span>
      </div>
    );
  };

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="notes.upload" className="import-file" aria-labelledby="import-file-title">
        <div className="picture-tile tile-streak import-tile" data-busy={mediaBusy}>
          <div className="tile-streak-art" aria-hidden="true" />
          <h2 id="import-file-title" className="tile-label">audio or video file</h2>
          <div className="tile-actions">
            <button type="button" className="glass-action import-choose" disabled={mediaBusy} onClick={() => { void chooseLocalFile("audio"); }}>
              Choose an audio file…
              <span className="glass-action-end" aria-hidden="true"><Icon name="plus" size={16} /></span>
            </button>
            <button type="button" className="glass-action import-choose" disabled={mediaBusy} onClick={() => { void chooseLocalFile("video"); }}>
              Choose a video file…
              <span className="glass-action-end" aria-hidden="true"><Icon name="plus" size={16} /></span>
            </button>
          </div>
          {/* Its name here, and all of it under the pointer: two files of one name are told apart by where they are. */}
          {localPick ? (
            <span className="import-picked" title={localPick.path}>
              <span className="import-picked-label">Selected</span>
              <span className="import-picked-name">{localPick.path.split(/[\\/]/).pop()}</span>
            </span>
          ) : null}
        </div>

        <div className="import-run">
          <p className="import-status" role="status">
            {snapshot && snapshot.stage !== "succeeded" ? `${importProgressText(snapshot)}${snapshot.provider ? ` · ${snapshot.provider}${snapshot.model ? ` / ${snapshot.model}` : ""}` : ""}` : ""}
          </p>
          <button
            type="button"
            className="primary"
            disabled={!localPick || !ffmpegReady || mediaBusy}
            onClick={() => {
              void runLocalImport();
            }}
          >
            Import and transcribe
          </button>
        </div>
        {!ffmpegReady && localPick ? (
          <p className="caveat">ffmpeg must be installed below before this can run.</p>
        ) : null}

        {stages === null ? null : (
          <ol className="import-stages">
            {stages.map((stage) => (
              <li key={stage.number} className="import-stage" data-state={stage.state} {...(stage.state === "current" ? { "aria-current": "step" as const } : {})}>
                <span className="import-stage-number">{stage.number}</span>
                {stage.label}
              </li>
            ))}
          </ol>
        )}
        {importActive(snapshot) ? <p className="muted">You can leave this screen and return while the import continues. Local transcription may take time; elapsed time does not measure how much audio has been processed.</p> : null}
        {snapshotError ? <p className="error">{snapshotError}</p> : null}
        {snapshot?.stage === "failed" ? <p className="error" role="alert">{snapshot.error}</p> : null}
        {snapshot?.stage === "interrupted" ? <p className="caveat" role="alert">{snapshot.error}</p> : null}
        {mediaResult ? (
          <div className="import-done" role="status">
            <span className="import-done-text">
              <span>Import complete</span>
              <span className="import-done-detail">
                {`Saved as note ${mediaResult.noteId} · ${mediaImportSourceLabel(mediaResult.result.source)} · ${mediaResult.result.provider}${
                  mediaResult.result.model ? ` / ${mediaResult.result.model}` : ""
                } · ${Math.round(mediaResult.result.durationMs / 1000)}s of audio · ${(mediaResult.result.retainedBytes / (1024 * 1024)).toFixed(1)} MB retained`}
              </span>
            </span>
            <button type="button" className="import-open" onClick={() => onOpenNote(mediaResult.noteId)}>Open imported note</button>
          </div>
        ) : null}
        {mediaError ? <p className="error">{mediaError}</p> : null}
      </section>

      <section aria-label="How it is transcribed">
        <Row
          label="Transcription goes to"
          value={route.problem ? "Nowhere usable" : destinationName(route.destination)}
          // The order Dictation's privacy line uses (`privacyLines`): a route
          // that does not leave the user's control is on this computer only
          // when it is the local model (KNEG-352).
          detail={
            route.problem
              ? route.problem
              : route.leavesDevice
                ? "audio leaves this computer"
                : route.isLocal
                  ? "audio stays on this computer"
                  : "audio goes to a server on your network"
          }
        />
        <LearnMore summary="How transcription is chosen">
          <p>
            This uses whichever transcription provider Settings &gt;
            Transcription already points at. Nothing here has a transcription
            provider of its own — a failed transcription here is the same
            failure a dictation would show, for the same reason.
          </p>
        </LearnMore>
      </section>

      <section aria-labelledby="import-sidecars">
        <h2 id="import-sidecars" className="section-label">sidecars</h2>
        {sidecarRow("ffmpeg", ffmpeg, () => installMediaImportSidecar("ffmpeg"))}
        {progress ? <Row label="Progress" value={progress} /> : null}
        <LearnMore summary="What ffmpeg is for">
          <p>
            Audio and video are converted to 16 kHz mono WAV with ffmpeg. It
            is downloaded on demand, the same way the local transcription
            server is on Settings &gt; Transcription &gt; Local.
          </p>
        </LearnMore>
        <p className="muted import-retained">Converted audio is retained for playback until you delete its note. Your original file is preserved.</p>
      </section>

      <section>
        <h2 className="section-label">import from openwhispr</h2>
        <p>
          Copies your notes, transcriptions and related records from
          OpenWhispr into this application; some OpenWhispr-only data is left
          behind.
        </p>
        <LearnMore summary="What is copied">
          {/* Category by category from `COPY_PLAN` (`crates/storage/src/import.rs`),
              which is what the import actually runs (KNEG-350). */}
          <p>
            Copies an existing OpenWhispr <code>transcriptions.db</code> into
            this application: notes and their folders, transcriptions, your
            custom dictionary, snippets, custom actions, agent conversations
            and their messages, speaker profiles, the speaker names
            kept with notes, contacts, and your Google, Microsoft and
            Apple calendars with their events.
          </p>
          <p>
            Left behind: anything you deleted in OpenWhispr, its{" "}
            <code>spaces</code> table and two internal queues, and every cloud
            and sync field. Speaker voice fingerprints kept with notes are also
            left behind. The report after the import lists every table that was
            not copied.
          </p>
          <p>
            The import only reads the OpenWhispr database and changes none of
            your notes or settings there. If OpenWhispr left its database in
            write-ahead mode, SQLite updates a small index file beside it (the
            one ending in <code>-shm</code>) when the database is read. That file
            holds none of your data, and OpenWhispr rebuilds it. If the result
            looks wrong, it is safe to delete this application's database and
            try again.
          </p>
        </LearnMore>

        <div className="row">
          <span className="row-label">Database file</span>
          <span className="row-value controls">
            <button
              type="button"
              disabled={importBusy}
              onClick={() => {
                void chooseDatabaseFile();
              }}
            >
              Choose transcriptions.db…
            </button>
          </span>
        </div>
        {dbPath ? <Row label="Selected" value={dbPath} /> : null}
        <button
          type="button"
          disabled={!dbPath || importBusy}
          onClick={() => {
            void runDatabaseImport();
          }}
        >
          {importBusy ? "Importing…" : "Import"}
        </button>

        {importError ? <p className="error">{importError}</p> : null}

        {importReport ? (
          <>
            <Row label="Copied" value={importReportSummary(importReport)} />
            <Row
              label="Search index"
              value={`${importReport.ftsRows} note(s) indexed`}
              detail="the FTS triggers ran during the import, not a separate rebuild"
            />
            {importReport.absentTables.length > 0 ? (
              <Row
                label="Not present in the source"
                value={importReport.absentTables.join(", ")}
                detail="this source predates those tables — there was nothing there to copy"
              />
            ) : null}

            <h3>What was left behind</h3>
            <p className="muted">Tables not copied</p>
            {leftBehindTableLabels(importReport).length === 0 ? (
              <p className="muted">No tables were left behind.</p>
            ) : (
              <ul className="list">
                {leftBehindTableLabels(importReport).map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            )}
            <p className="muted">Columns not copied</p>
            {droppedColumnsByTable(importReport).length === 0 ? (
              <p className="muted">No columns were dropped.</p>
            ) : (
              <ul className="list">
                {droppedColumnsByTable(importReport).map((d) => (
                  <li key={d.table}>
                    <strong>{d.table}</strong>
                    <span className="muted"> {d.columns.join(", ")}</span>
                  </li>
                ))}
              </ul>
            )}

            {importReport.relocatedCredentials.length > 0 ? (
              <>
                <h3>Credentials relocated to this system's credential store</h3>
                <p className="muted">Values are never shown, here or anywhere else.</p>
                <ul className="list">
                  {importReport.relocatedCredentials.map((c) => (
                    <li key={c.slot}>{relocatedCredentialLabel(c)}</li>
                  ))}
                </ul>
              </>
            ) : null}

            <h3>Per-table detail</h3>
            <ul className="list">
              {importReport.tables.map((t) => (
                <li key={t.table}>
                  <strong>{t.table}</strong>
                  <span className="muted">
                    {" "}
                    {t.copied} copied
                    {t.skipped > 0 ? `, ${t.skipped} skipped` : ""}
                    {deletedLeftOutLabel(t.deletedLeftOut)}
                  </span>
                  {t.skipReason ? <em className="row-detail">{t.skipReason}</em> : null}
                </li>
              ))}
            </ul>
          </>
        ) : null}
      </section>
    </div>
  );
}
