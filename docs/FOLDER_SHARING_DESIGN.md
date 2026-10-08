# KNEG-411: local folder sharing

Status: implemented locally on 2026-10-07; integration and release validation are separate.
Issue: https://linear.app/axelalbertkarlsson/issue/KNEG-411
Baseline: dev at b24415ce8e61bb7554dd4cf36b7fa38fa90174df.

## Original investigation

The exploration below records the starting rationale. The implemented contract and
confirmed product decisions are described at the end of this document.

### User goal and working proposal

Make it easy to give a colleague a folder of notes, transcripts and recordings,
with an easy import into their local KnegisWhisper installation. The app does
not need a hosted service to export and import a file. Delivery can use an
existing file transfer channel chosen by the user.

Proposed first version: share a snapshot of one folder as one portable package.
The recipient receives an independent local copy. Subsequent edits require a
new export. Continuous shared folders and collaborative editing need separate
transport and conflict decisions.

## Proposed interaction

1. Folder menu: Share folder...
2. Preview included notes and choose transcripts, summaries, personal notes and
   recordings. Show estimated size and clearly identify omitted content.
3. Save one package using a native save dialog.
4. Send the package through the user's existing file transfer channel.
5. Recipient selects Import shared folder, previews contents, then imports.

An app menu file picker is sufficient for an initial experiment. Double-click
file association and drag-and-drop can follow. Whether recipients should also
be able to browse without installing KnegisWhisper remains undecided.

## Findings from the baseline source

| Existing component | Consequence for sharing |
| --- | --- |
| `crates/notes-core/src/notes.rs`: folders have numeric IDs, names and sort order; notes have numeric IDs and one optional folder ID | IDs need remapping on import. The present model is flat; nested folders are not part of this proposal. |
| `crates/notes-core/src/recordings.rs`: `note_recordings` relates a note to an application-owned audio filename | Package the actual audio bytes and assign fresh destination filenames. Paths on the sender's computer cannot provide recipient playback. |
| `crates/notes-core/src/speakers.rs`: renamed speakers live in a separate mapping | Preserve mappings alongside the timed transcript. |
| `crates/storage/migrations/0005_meeting_summaries.sql` and `crates/notes-core/src/summary.rs`: summaries have source snapshots, references and revision metadata | Copying summary prose alone loses source-link and outdated-state information. Define the portable summary subset. |
| `crates/notes-core/src/notes.rs`: `create` enqueues automatic summaries | Import needs a dedicated path that restores supplied data without generating summaries or making provider calls. |
| `apps/desktop/src-tauri/src/commands.rs`: `note_export_to_file` uses a native Rust save dialog | Reuse the dialog boundary for package export/import. Current transcript export is not a complete note backup. |
| `crates/storage/migrations/0006_note_workspace.sql`: search indexes transcript text via database triggers | Rebuild recipient search from imported records; do not export search-index internals. |
| Root `Cargo.toml` already declares ZIP, SHA-256 and UUID dependencies | A ZIP-based package can reuse existing dependency declarations; concrete API feasibility still needs a prototype. |

## Proposed package contract to explore

A versioned ZIP container with a provisional `.knegis` extension:

```text
manifest.json
notes/<portable-note-id>.json
recordings/<portable-recording-id>.wav
```

The manifest would carry format version, package identity, export timestamp,
folder metadata, inclusion choices, and content sizes/digests. Note documents
would contain selected user content, original dates, transcript timing and
speaker mappings, plus the selected summary metadata. Relative recording entries
would connect each note to its included audio. Do not export the full application
database, credentials, provider configuration or background jobs.

Local numeric IDs cannot identify notes across installations. Investigate stable
export identities and retained import provenance before promising repeated-export
deduplication. A new package ID alone detects only the identical package being
imported twice; it cannot recognise a revised export of the same folder.

ZIP compression does not imply encryption. Password protection, if wanted,
needs its own format and usability decision.

## Import and export properties to verify in a prototype

- Export reads a consistent snapshot of the selected notes and related data.
  Audio copying must account for deletion or missing files during export.
- Export flushes pending edits before capture and publishes the final file only
  after completion. Missing recordings require an explicit result, not silent loss.
- Import validates format version, entry names, sizes, counts, references and
  digests before making notes visible. Archive entries never choose destination paths.
- Recordings are streamed through bounded staging rather than loaded in memory.
  Failed/cancelled imports clean up staged files and leave no partial visible folder.
- Database records commit together. A filesystem/database coordination strategy
  must also handle process interruption; a SQL transaction alone is insufficient.
- Imported notes get fresh local IDs and recording names. Summary links still
  resolve against the preserved transcript, including outdated summaries.
- Import does not automatically retranscribe, regenerate summaries or call a
  provider. Previewing a package also does not modify the library.
- Re-import never silently overwrites recipient edits or merges same-name folders.

## Product decisions still open

1. Do recipients normally have KnegisWhisper, or is offline browser viewing required?
2. Which content is included by default? Personal notes need a deliberate choice;
   recordings materially affect size. Should individual notes be deselectable?
3. Is one-folder snapshot sharing sufficient initially, or are repeated updates
   part of the first version? If updates are required, choose skip/copy/replace rules.
4. When a folder name already exists, create a distinct imported folder or offer a
   destination choice? Name equality must not imply shared identity.
5. Does the first version need password protection, or will users rely on their
   chosen delivery channel?

## Next investigation

After settling the recipient and snapshot/update expectations, prototype an
export/import round trip on disposable data: two notes, Unicode titles, speaker
renames, an edited summary with source links, and one recording. Verify playback,
search and re-import behaviour. Exercise truncated packages, missing audio,
cancelled imports and process interruption before proposing production integration.

## Implemented contract

The owner selected app-to-app sharing, independent snapshot copies, all content
checked by default, individual note selection, and no password protection in v1.
Browser viewing, synchronization, merging, stable cross-installation identities,
file associations and delivery integrations remain outside this version.

Notes exposes Share folder from its folder actions and Import shared folder beside
media import. Both use native file dialogs; filesystem paths are never input from
the renderer. Export flushes pending edits. Import previews staged contents, allows
a destination name, creates a fresh folder, expands it and opens its first note.
Cancel cleans unfinished file work; Keep working allows navigation during processing.
One application-owned sharing operation runs at a time, independent of transcription.

### Package version 1

The `.knegis` file is a ZIP with `manifest.json`, `notes/<uuid>.json` and optional
`recordings/<uuid>.wav`. Manifest and note keys use snake_case; native IPC types use
camelCase. The manifest carries `format_version`, `package_id`, `exported_at`,
`folder_name`, `options`, `notes` (note entry names), and `entries` (path, bytes,
sha256). Entry IDs exist only inside a package.

Note documents carry title, content, note_type, enhanced_content, transcript,
participants, audio_duration_seconds, created_at, updated_at, speakers, summary and
recording. Summary preserves committed provenance, references and edited/outdated
state when transcripts are included. Without transcripts, summary source links are
removed and metadata/source snapshots are excluded. No generation jobs are copied.
Recording entries retain the original audio bytes, duration and a basename only for
the original filename. Account settings, credentials, machine paths, calendar
connections and voice profiles are excluded. Where a note was transcribed
(`transcribed_by`, KNEG-368) is not carried either, so the format is unchanged and a
shared note arrives saying nothing about it. Export reads every `Note` column through
`notes::COLUMNS`, so a column added to notes does not break it.

Limits: 10,000 notes; 20,001 entries; 32 GiB uncompressed total; 4 GiB per recording;
16 MiB per note; 256 MiB note documents in aggregate; 8 MiB manifest. WAV recordings
must be nonempty 16-bit PCM, one or two channels. Digests and streamed byte counts
are checked. Unsafe, duplicate, unlisted, encrypted, linked or unreferenced entries
and unsupported versions fail import. Text estimates are approximate and include all
text, even when some text options are unchecked.

### Storage and recovery

Migration 0009 adds `folder_share_operations`, keyed by operation UUID. It journals
the stage, application-owned staging path, generated recording keys, result folder,
export path/digest and error. It has no AUTOINCREMENT ID and does not alter the
OpenWhispr importer’s empty-database rule. Storage's build script tracks the migration
directory so incremental builds embed new migrations.
Version 0008 is reserved for the concurrent transcript-search worktree.

Imports stage and validate before confirmation, promote files under new filenames,
and commit folder/notes/recordings/speaker mappings/summary metadata together with
the success marker. Startup cleans files from uncommitted operations, retaining
attached recordings. Imported rows bypass automatic summary enqueueing; existing
search triggers index restored content. Same-name folders get a numbered suffix.
Repeated imports always create independent copies.

Exports write and flush a temporary file beside the chosen destination, then
atomically publish it. Before publication the journal records the final archive’s
digest; recovery can identify a completed publication even if the success update
was interrupted. Dismissal is durable, so old success dialogs do not reappear.

### Code and checks

`kw_notes_core::sharing` owns the package and database contract. Desktop `sharing`
owns dialogs, lifecycle and the eight `folder_share_*` commands. Sharing IPC types
have their own generated file and one Rust writer, keeping unrelated concurrent
changes to the main IPC types separate.

```powershell
cargo test -p kw-notes-core -p kw-storage
cargo test -p knegiswhisper --lib --features tauri-test sharing::
$env:KW_WRITE_IPC_TYPES='1'; cargo test -p knegiswhisper --lib sharing::types
node tools/visual-check/folder-sharing.mjs http://127.0.0.1:5197/
cargo test -p knegiswhisper --lib --features tauri-test native_folder_sharing_round_trip -- --ignored --nocapture
```

The native smoke test uses an in-memory database, disposable recordings and its own
WebView profile. Its helper fills only dialogs belonging to the test process. The
browser check uses invented fixtures and writes screenshots/report below the ignored
`tools/visual-check/out/kneg-411` directory. Native picker/playback evidence and browser
layout evidence must be reported separately.

### Local validation on 2026-10-07

- Notes/storage: 85 unit tests, 2 transcript integration tests and 69 storage tests passed.
  Sharing covers all 16 content combinations, deselected notes, source references,
  independent repeat imports, search, rollback, cancellation, damaged packages and
  startup recovery before and after export publication.
- Desktop: 461 library tests passed with `tauri-test`; 7 interactive tests were
  excluded from that ordinary run. Sharing IPC checks the window and operation
  identity, one-time confirmation, durable dismissal and generated types.
- The explicitly run Windows sharing smoke passed with real save/open dialogs,
  export/import IPC, byte-identical recordings, range serving and WebView playback.
  It used an in-memory database and temporary files; the normal app database was
  not used.
- Frontend typecheck/build and all 303 tests passed. Headless Edge passed 9 sharing
  checks, including minimum-window layout, keyboard focus, navigation during an
  operation, reconnection and opening the imported note. Those checks use fixtures.
- Rust formatting and Clippy with warnings denied passed. Command grants, window
  capabilities, development-bundle, domain and integration-parity audits passed.

These are local Windows and fixture checks. Hosted CI, packaged builds and native
macOS/Linux behavior have not been validated in this worktree.
