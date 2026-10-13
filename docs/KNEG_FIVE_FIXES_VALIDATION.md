# Five backlog fixes — implementation and verification

Date: 2026-10-12. Base: `dev` at `317f529`. Branch: `codex/kneg-453-454-456-421-399`.
All changes are in an isolated worktree. No merge, push or Linear status update was performed.

## Delivered behaviour

| Issue | Behaviour |
| --- | --- |
| KNEG-456 | The note-meta check expects the provider display name, Deepgram Realtime. |
| KNEG-454 | App reveals own their scroll events until scrollend or 150 ms of inactivity. Actual user motion interrupts them. Rename keeps Follow off until re-enabled after closing the form. |
| KNEG-453 | Speaker buttons no longer create a Tab stop for every turn. F2 renames the focused line's speaker, including untimed speaker lines, and closing the form restores line focus. Read-only lines do not advertise or execute rename. |
| KNEG-421 | File-backed deletion connections use a 100 ms contention timeout. Empty trash commits one confirmed note at a time, rechecks its current attachment/Trash state, reports partial completion and refreshes the UI after errors. |
| KNEG-399 | A read-only readiness command gates manual generation, identifies setup actions and accepts installed-but-stopped local models. Setup opens the existing form; Return to note restores selection and action focus. Progress labels use saved worker counts. |

## Compatibility and failure handling

- No database migration, summary prompt change or new external provider request shape.
- `summary_readiness_get` adds generated `SummaryReadiness` and `SummarySetupTarget` IPC types.
- Manual generation still works with automatic generation disabled. Readiness never starts or downloads a model, probes a provider or returns credentials.
- Setup reads may fail independently of loading the form. Both the note and form provide Retry without treating a failed read as proof of missing configuration.
- Deletion retains checkpoint-before-file-removal and file-removal-before-commit ordering. Earlier successful purges stay committed after a later failure; errors distinguish remaining files from recordings already removed before a failed commit.
- A single slow filesystem removal can still hold its transaction. The change bounds SQLite contention and removes transactions spanning an entire Trash list; it does not promise a hard filesystem deadline.

## Verification

| Gate | Result |
| --- | --- |
| Desktop TypeScript tests | 489 passed |
| Notes-core Rust tests | 169 passed |
| Desktop Rust tests, `tauri-test` | 539 passed, 14 ignored in the ordinary suite |
| Tool self-tests | 197 passed |
| Frontend typecheck and production build | Passed; Vite retains its existing large-chunk advisory |
| Clippy, notes-core and desktop, all targets with `tauri-test` | Passed with warnings denied |
| Rust formatting, generated IPC types, Git whitespace | Passed |
| Command and capability audits | Passed |
| Existing transcript-column browser suite | All 17 checks passed in light/dark |
| Follow/F2 browser regression | Passed at 1000 and 1440 px in light/dark |
| Summary setup browser regression | Passed at 720 and 1440 px in light/dark |
| Partial Trash browser regression | Partial completion surfaced; list refreshed; retry emptied the remainder |
| Note-meta browser check | 4/4 assertions in each of light/dark |
| Native recording retry | Explicitly run ignored test passed with a reopened temporary database |
| Native summary workflow | Explicitly run ignored test passed: readiness, rejected enqueue, hierarchical generation, references, retained edits on failed regeneration and cancellation |

The contention test holds an older WAL reader while deleting through the normal
five-second writer pool. Deletion and a concurrent save finish within one second,
the recording remains, and the ordinary connection timeout remains 5000 ms.
Other tests verify per-note save/restore interleaving, duplicate IDs, partial file
failure and retry, alongside existing commit-failure and replacement protections.

Browser reports and screenshots are under this worktree's ignored
`target/five-fixes/` directory. Native tests use synthetic data and credentials;
the summary provider is a local test server. The user's database and recordings
were not used for validation.

## Remaining evidence boundaries

These results do not establish real provider authentication, summary model quality,
macOS overlay-scrollbar behaviour, touchscreen interactions, Narrator output,
hosted CI or release readiness. Twelve other environment-dependent ignored desktop
tests were not run by this task. No required user input was deferred into execution.
