# CLI and MCP workflows

The desktop app owns all operations. Enable its local API, then use the compiled
`knegiswhisper` CLI or `knegiswhisper-mcp` stdio server. Existing discovery-file
authentication, loopback binding and permission checks also protect the new routes.

## Commands

```powershell
knegiswhisper status
knegiswhisper models list
knegiswhisper notes create --title "Meeting notes" --content "## Actions"
knegiswhisper transcribe file .\meeting.wav
knegiswhisper jobs status JOB_ID
knegiswhisper jobs cancel JOB_ID
knegiswhisper history search "budget review" --limit 20
knegiswhisper notes export NOTE_ID --output .\notes.md
knegiswhisper notes export NOTE_ID --format srt --transcript --output .\captions.srt
knegiswhisper recording start --provider local
knegiswhisper recording stop
```

Use `--json` for automation. Export returns content on stdout unless `--output`
is supplied; an existing output file is refused unless `--force` is explicit.
Interactive terminal output escapes control characters that could change the terminal
display or state, retaining line breaks and tabs. Pipes, redirection and files saved
with `--output` retain the original content. Interactive JSON remains valid JSON.

Markdown exports the current editable note and its title by default. `--transcript`
exports immutable timed transcript segments. Text, JSON, SRT and VTT exports use
those segments and report a validation error when they are unavailable.

The 10 new MCP tools are `app_status`, `models_list`, `notes_create`,
`notes_export`, `transcription_create`, `jobs_get`, `jobs_cancel`,
`transcriptions_search`, `recording_start` and `recording_stop` (23 total).
`transcription_create.path` refers to a file accessible to the MCP server process.
The CLI/MCP client streams its bytes to the app; the HTTP API never accepts a
client-selected server filesystem path. Consult `tools/list` for argument schemas.

## Durable file imports

`POST /v1/jobs/transcriptions` accepts exactly one multipart `file` field and
returns HTTP 202 with a job ID. Poll `GET /v1/jobs/{id}`. A job advances through
`preparing`, `converting`, `transcribing` and `saving`, then ends in `succeeded`,
`failed` or `cancelled`. Fast transitions may not appear in every poll.

UI imports and API imports share one active slot. There is no queue: a second
import returns conflict. Jobs record timestamps, provider/model, a sanitized error
and the saved note ID. The successful job and note are committed together. A note
moved to Trash -- `DELETE /v1/notes/{id}` does that, as Delete in the app does -- keeps
its reference in the job, though the API answers `404` for the note itself; deleting it
forever from Trash in the app clears the reference. Completed metadata survives app restarts;
unfinished work becomes failed with an interruption reason and is never retried
automatically.

Cancellation is acknowledged with `cancellation_requested`; poll until terminal.
It interrupts conversion or transcription and removes temporary media. Once saving
begins, cancellation returns conflict so an acknowledged cancellation cannot race
with committing the note. The existing UI can observe the same import progress.

Uploads are limited to 500 MiB, with a 60-second inactivity timeout and a separate
multipart envelope allowance. Ordinary JSON requests retain their 1 MiB limit.
Partial/disconnected uploads are removed. Startup also removes decoded scratch
audio from directories identified by durable job IDs, preserving unknown contents.
FFmpeg receives generated local paths,
restricted media demuxers and file/pipe protocols; playlist/network inputs are
excluded. Decoded audio is capped at 512 MiB and an input that reaches the cap
fails instead of saving a truncated transcript. Original filenames are only labels.
The app uses its configured transcription route, including cloud transmission when
the user has configured a cloud provider.

History search indexes final dictation text, including existing rows migrated from
older databases. It does not index raw pre-cleanup text. Literal word queries use
the same safe FTS query handling as note search. Results page newest first with
`after`/`next_cursor`; editing/deleting history updates the index.

## Recording and discovery

Recording commands control the existing visible meeting recorder, including a
meeting started in the desktop UI. `recording start` requires a provider and uses
the microphone. System audio is optional (`--system-audio`) and the response reports
whether capture is available. The existing 30-minute bound and meeting stop/save
behavior apply. These commands do not start dictation or insert text into another app.

`status` reports dictation/meeting state, the configured route and the latest
in-memory import's durable metadata. `models list` reports provider configuration,
local installations and meeting-provider readiness. `connectivity_verified: false`
means this discovery call has not tested cloud credentials or provider connectivity.
An unavailable desktop runtime returns HTTP 503; overlapping operations return 409.

## Windows validation, 2026-09-07

The worktree's compiled binaries were exercised against a separate debug profile,
using copied local Whisper tiny/FFmpeg sidecars and the repository's public speech
fixture. The normal user database and OS credential slots were not used.

- CLI: status/models, create/editable export, fixture upload into a saved note,
  history search, cancellation during transcription and upload cleanup.
- Restart: successful/cancelled jobs survived; an interrupted fixture job became
  failed with an actionable reason, and its temporary upload was removed.
- Official Python MCP SDK 2.2.0: stdio initialization negotiated `2024-11-05`,
  listed 23 tools, created/exported a note, read status/models, searched history
  and uploaded/transcribed the fixture into a saved note. Protocol stdout remained
  parseable and diagnostics used stderr.
- CLI meeting start/stop: local microphone capture returned a saved note with an
  attached recording. A separate meeting indicator window appeared; Windows
  reported its capture exclusion active. Its excluded screenshot does not verify
  the indicator's text or controls.
- CLI and MCP integration screens: both displayed binary discovery, running API
  status and matching Windows permission/readback/administrator caveat text.

Cloud-provider execution, system-audio capture and an editor-specific MCP setup
were not exercised in this run. Existing provider/device validation requirements
still apply. Automated tests cover the HTTP/CLI/MCP contracts, migration backfill,
restart persistence, atomic note saving and cancellation boundaries.

Validation gates passed: `cargo test --workspace --features knegiswhisper/tauri-test`
(1,795 passed, 15 opt-in tests ignored), followed by the final desktop regression
run after scratch recovery (382 passed, 5 ignored); strict workspace Clippy with
the same feature; frontend tests and production build; formatting, generated types,
and repository contract audits. The frontend build retained its existing large-chunk
warning.

To isolate a future **debug** smoke run, set `KW_AUTOMATION_PROFILE` to an absolute
directory and run `cargo run -p kw-storage --example smoke_profile` once. It refuses
an existing database. Debug builds then use that profile for data/discovery and an
in-memory credential store; release builds ignore the override. Supply local
sidecars/models separately. Set `WEBVIEW2_USER_DATA_FOLDER` to a separate directory
when another desktop instance is open, and give the test Vite/Tauri dev URL its own
port. Temporary SDK installations and smoke reports belong under ignored `target/`.
