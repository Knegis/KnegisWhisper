# Meeting summaries

Implemented on 2026-09-20. Tracking: [KNEG-278](https://linear.app/axelalbertkarlsson/issue/KNEG-278/implement-editable-meeting-summaries-with-durable-generation-and), child of the OpenWhispr recommendations (KNEG-275). Committed and pushed as `c04491a`; release validation remains outstanding.

## Use

In Settings > Cleanup & summaries > Meeting summaries, select a provider/model and save.
Automatic generation is off by default. Enable it to summarize newly saved meetings
and completed audio/video imports, including imports submitted through the local API.
Turning it off cancels queued automatic jobs; already-running jobs have a Cancel action.
Older notes can use Generate summary without enabling automatic generation.

When saved configuration is incomplete, a note offers **Set up summaries** instead
of queueing a request that cannot work (KNEG-399). It opens the summary settings form;
**Return to note** restores the same note and focuses its summary action. Missing keys
or local model assets have links to their setup pages. Returning never generates a
summary automatically. Installed local models can be started by the worker, so a
stopped model does not require setup again. Readiness describes configuration, not
successful provider authentication or model quality.

Generation reports Queued, Preparing transcript, Summarising transcript, Combining
summary and Saving summary from its durable job state. Part counts reflect actual
worker steps, including saved steps on retry; they are not an ETA or percentage.
Failed, interrupted and cancelled jobs keep their explicit states and recovery actions.

The Notes tab shows personal notes followed by a separate AI summary section. Summaries cover overview, decisions,
actual commitments and unresolved questions, omitting empty sections. The default
language follows the meeting; Swedish and English can be explicitly selected.
Timestamp links open the Transcript tab, select the supporting row and use existing recording
playback when retained audio is available. References are navigation aids, not proof
that a model's claim is correct. Generated Markdown is escaped before rendering.

Summary edits autosave independently. Regeneration asks before replacing edited or
imported summaries and keeps the old content until successful. Copy, Export Markdown
and Export text apply to the summary. Transcript changes mark a summary outdated and
disable its old source links, and so do changes to the note's people, which can be
edited since KNEG-362. Only a summary being made when the people change is stopped,
with "Generation stopped because the note, summary or job changed." under Retry
summary; the retry starts over, since its saved progress was for the old people. A
summary still queued -- the automatic one queued when a meeting is saved, which is when
people get added -- takes the note's people as they are when it starts, keeping its
saved progress only if they are unchanged. Imported legacy summaries remain editable.

The model is told the note's people as spelling help: their display names,
comma-separated and at most 256 characters, and never an email address (KNEG-362). A
name holding an `@` is taken for an address and left out.

## Implementation

- `crates/notes-core/src/summary.rs`: durable job/state operations, optimistic revisions,
  source snapshots/hash, editing, retry, cancellation and guarded final writes.
- `crates/storage/migrations/0005_meeting_summaries.sql`: summary metadata and jobs,
  with cascading note deletion. Prose uses existing `notes.enhanced_content` and FTS.
- `crates/notes-core/src/notes.rs`: schedule inside the final note-save transaction.
  Foreign OpenWhispr database imports do not trigger automatic generation.
- `crates/reasoning-core/src/summary.rs`: evidence IDs, bounded Unicode-safe chunks,
  structured-output validation, language aggregation and Markdown formatting.
- `apps/desktop/src-tauri/src/summaries.rs`: one desktop-owned sequential worker,
  cancellation/invalidation, checkpoint reuse, provider routing and native exports.
- `SummaryPane.tsx`, `SummarySettingsPanel.tsx`: separate settings, autosave and actions.

Long transcripts are extracted in bounded sections, then combined pairwise until one
summary remains. Completed extracts are persisted. Queued work survives restart;
processing jobs become Interrupted and require explicit Retry. Checkpoints are reused
only with unchanged settings and source. Stale or cancelled results cannot overwrite
newer edits, changed sources or deleted notes. Moving a note to Trash cancels a summary
queued or being made for it, and a note in Trash can't be summarized until it is restored
(KNEG-401). A failed summary never rolls back the saved transcript, note or recording.

Summary requests have a separate 600-second timeout per request. Empty, refused,
truncated, invalid-JSON and unknown-source responses fail visibly without automatic
request replay or provider fallback. An optional JSON code fence is accepted; malformed
JSON is not repaired. Error messages omit raw provider bodies and transcript content.
The saved setting contains no provider credential; existing credential storage is used.

Local generation uses installed models only and caps context at the runtime's 8,192
tokens. A shared request lease excludes cleanup/model switching/idle shutdown while
inference is active and restores the preceding local model after summary use. Local
health checks now wait for a successful response instead of accepting loading-time
HTTP 503 as readiness. If no local model was previously running, the summary model
is unloaded before releasing the request lease. Cloud and enterprise routes reuse the existing transports and
network policy. No generation API/CLI/MCP endpoint, custom templates or version history
was added.

## Verification

Passed locally:

- `cargo test --workspace --lib` before the final small readiness/parser changes;
  affected crate tests repeated afterward.
- Notes-core tests cover automatic/manual scheduling, atomic import failure, checkpoint
  recovery, deduplication, edited-summary protection, stale results, deletion and FTS.
- Reasoning tests cover Unicode chunk boundaries, invalid references, no-substance
  output, code-fenced JSON, refusal and truncation. Sidecar readiness regression checks
  both 503 and 200 responses while preserving generic listener-health behavior.
- Desktop TypeScript check/production build and all 24 frontend tests.
- Clippy with warnings denied; settings, command-grant and capability audits.
- Native Windows WebView2 in a disposable profile with a synthetic HTTP provider:
  automatic generation, long-transcript hierarchical requests, separate persistence,
  edited-summary protection on failure, cancellation, timestamp display and source-row
  selection. Native editor changes also persisted through an immediate window-close
  request, verified by reading the saved database afterward. Reproducible probe: `tools/visual-check/summary-smoke.mjs --isolated-profile`.
  This probe creates synthetic notes and changes settings; never use a real user profile.

Real installed Qwen2.5 1.5B inference was exercised with a short synthetic Swedish
meeting. It demonstrated local transport, but omitted commitments/decisions and once
returned English prose marked as Swedish. Other attempts failed structured-output
validation and correctly left the saved note untouched. Structured validation cannot establish
factual completeness or correct language. This model is not a validated recommendation
for meeting summaries. No real meeting accuracy claim is made.

Still open under KNEG-278: quality evaluation on representative Swedish/English
meetings and a suitable model, a credentialed BYOK round trip, retained-audio source
playback and native save-dialog export. The isolated
profile has no cloud credentials. Automated storage/flow checks do not establish these
product behaviors. macOS/Linux native execution and release/hosted CI were not tested.
