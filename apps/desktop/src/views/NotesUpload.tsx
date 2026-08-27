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
 * Three things this screen refuses to do, because getting any of them wrong
 * would be worse than not building the screen at all.
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

import { useCallback, useEffect, useState } from "react";

import { Row } from "../components/Row";
import {
  createNote,
  getDictationRoute,
  getMediaImportStatus,
  importAudioFile,
  importOpenWhispr,
  importVideoFile,
  installMediaImportSidecar,
  onMediaImportProgress,
  pickImportFile,
  type ImportReport,
  type MediaImportResult,
  type SidecarStatus,
} from "../lib/ipc";
import {
  droppedColumnsByTable,
  importReportSummary,
  mediaImportProgressLabel,
  mediaImportSourceLabel,
  relocatedCredentialLabel,
  sidecarUnavailableReason,
} from "../lib/labels";
import { message, useRefresh } from "../lib/useRefresh";

/** The file name without its directory or extension, for a default title. */
function baseName(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

/**
 * Turn a finished import into the note the screen shows.
 *
 * A single-segment transcript rather than leaving `transcript` empty: this
 * note genuinely was recorded, just with no per-utterance timestamps, and an
 * SRT or VTT export of it should say so with the real duration rather than
 * reading as a note that was only ever typed.
 */
async function saveImportedNote(
  result: MediaImportResult,
  fallbackTitle: string,
): Promise<number> {
  const durationSeconds = result.durationMs / 1000;
  const title =
    result.source.kind === "url"
      ? `Imported from ${result.source.host}`
      : fallbackTitle;

  return createNote({
    title,
    content: result.text,
    noteType: "import",
    folderId: null,
    transcript: JSON.stringify([
      { text: result.text, timestamp: 0, endTimestamp: durationSeconds },
    ]),
    participants: null,
    audioDurationSeconds: durationSeconds,
  });
}

export function NotesUpload() {
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
    void onMediaImportProgress((p) => {
      setProgress(mediaImportProgressLabel(p));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Audio/video import.
  const [localPick, setLocalPick] = useState<
    { kind: "audio" | "video"; path: string } | null
  >(null);
  const [mediaBusy, setMediaBusy] = useState(false);
  const [mediaError, setMediaError] = useState<string | null>(null);
  const [mediaResult, setMediaResult] = useState<{
    noteId: number;
    result: MediaImportResult;
  } | null>(null);

  // OpenWhispr database import.
  const [dbPath, setDbPath] = useState<string | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [importReport, setImportReport] = useState<ImportReport | null>(null);

  const chooseLocalFile = async (kind: "audio" | "video") => {
    const path = await pickImportFile(kind);
    if (path !== null) {
      setLocalPick({ kind, path });
      setMediaResult(null);
      setMediaError(null);
    }
  };

  const runLocalImport = async () => {
    if (!localPick) return;
    setMediaBusy(true);
    setMediaError(null);
    setMediaResult(null);
    try {
      const result =
        localPick.kind === "audio"
          ? await importAudioFile(localPick.path)
          : await importVideoFile(localPick.path);
      const noteId = await saveImportedNote(result, baseName(localPick.path));
      setMediaResult({ noteId, result });
      setLocalPick(null);
    } catch (e) {
      setMediaError(message(e));
    } finally {
      setMediaBusy(false);
      setProgress(null);
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
            <span>Installed</span>
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

      <section data-surface="notes.upload">
        <h2>Upload</h2>
        <p className="muted">
          Turn a recording into a note using whichever
          transcription provider Settings &gt; Transcription already points
          at. Nothing here has a transcription provider of its own — a failed
          transcription here is the same failure a dictation would show, for
          the same reason.
        </p>

        <Row
          label="Transcription goes to"
          value={route.problem ? "Nowhere usable" : route.destination}
          detail={
            route.problem
              ? route.problem
              : route.leavesDevice
                ? "audio leaves this machine"
                : "audio stays on this machine"
          }
        />

        <h3>Sidecars</h3>
        <p className="muted">
          Audio and video are converted to 16 kHz mono WAV with ffmpeg. It is
          downloaded on demand, the same way the local transcription server
          is on Settings &gt; Transcription &gt; Local.
        </p>
        {sidecarRow("ffmpeg", ffmpeg, () => installMediaImportSidecar("ffmpeg"))}
        {progress ? <Row label="Progress" value={progress} /> : null}

        <h3>Audio or video file</h3>
        <div className="row">
          <span className="row-label">File</span>
          <span className="row-value controls">
            <button
              type="button"
              disabled={mediaBusy}
              onClick={() => {
                void chooseLocalFile("audio");
              }}
            >
              Choose an audio file…
            </button>
            <button
              type="button"
              disabled={mediaBusy}
              onClick={() => {
                void chooseLocalFile("video");
              }}
            >
              Choose a video file…
            </button>
          </span>
        </div>
        {localPick ? (
          <Row label="Selected" value={localPick.path} detail={localPick.kind} />
        ) : null}
        <button
          type="button"
          disabled={!localPick || !ffmpegReady || mediaBusy}
          onClick={() => {
            void runLocalImport();
          }}
        >
          Import and transcribe
        </button>
        {!ffmpegReady && localPick ? (
          <p className="caveat">ffmpeg must be installed above before this can run.</p>
        ) : null}

        {mediaResult ? (
          <Row
            label="Imported"
            value={`Saved as note ${mediaResult.noteId}`}
            detail={`${mediaImportSourceLabel(mediaResult.result.source)} · ${mediaResult.result.provider}${
              mediaResult.result.model ? ` / ${mediaResult.result.model}` : ""
            } · ${Math.round(mediaResult.result.durationMs / 1000)}s of audio`}
          />
        ) : null}
        {mediaError ? <p className="error">{mediaError}</p> : null}
      </section>

      <section>
        <h2>Import from OpenWhispr</h2>
        <p className="muted">
          Copies an existing OpenWhispr <code>transcriptions.db</code> into
          this application: notes, transcriptions, folders, snippets, custom
          actions, agent conversations, speaker profiles and calendar events.
        </p>
        <p className="caveat">
          The source file is opened read-only and is never written to.
          Nothing this import does can change or corrupt your existing
          OpenWhispr install — if anything about the result looks wrong,
          nothing has been lost from the source, and it is safe to delete
          this application's database and try again.
        </p>

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
