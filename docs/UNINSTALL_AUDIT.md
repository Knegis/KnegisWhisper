# Windows uninstall audit (KNEG-535)

This is the historical pre-helper audit. The subsequent implementation and its
current validation limits are in [UNINSTALL_IMPLEMENTATION.md](UNINSTALL_IMPLEMENTATION.md).

Audited 2026-11-10 on `codex/kneg-535`, from runtime source
`26220ba8df97c6f1c16c8324d421aef02d6c2a54`. This change documents existing
behaviour; it changes no application code, installer configuration or deletion policy.
The checkbox defect is tracked in [KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545).
Installed-app acceptance remains [KNEG-534](https://linear.app/axelalbertkarlsson/issue/KNEG-534).

## Evidence and limits

A new Windows x64 **debug** NSIS bundle built successfully with Rust 1.99.0,
Tauri CLI 2.11.4 and app version 0.1.1. The generated script was inspected and
its deletion paths/guards checked. The installer was **not executed**. This is
packaging and source evidence, not a clean-machine uninstall result.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/debug/nsis/x64/installer.nsi` | 31,636 | `ad587b6b5dd3bc3914be89575a64dd0a8af8136a2b4fa9700b966a8cc825aeba` |
| `target/debug/bundle/nsis/KnegisWhisper_0.1.1_x64-setup.exe` | 9,724,435 | `eae307b5c9d7e057da17400e88b963377d03666d0d5a87fd2be5e74e24dc3c30` |

The build used `npm.cmd --prefix apps/desktop run tauri:build -- --debug --bundles
nsis --config <worktree>/.validation/build-config.json`, with the shared Cargo
target directory and Rust 1.99.0. A local config disabled updater artifacts and
the duplicate frontend build hook. The frontend assets came from the previously
validated production build at the same runtime commit; no frontend/runtime code
changed for this audit. This is not the published v0.1.1 installer.

The local `v0.1.1` tag resolves to `c87b31ce0830288f45339783de27ab76820559b6`. Its storage paths,
recording paths, local API discovery paths, credential implementation and npm
lockfile are unchanged from the audited source. Its NSIS configuration still
uses `perMachine` with no custom template/hooks, and its CLI is also 2.11.4.
The Tauri configuration difference is the newly bundled license notices.
This supports the same retention expectations for v0.1.1 by source comparison;
it does not establish what an actual run of the published binary does.

## How the retention table was derived

- [Storage paths](../crates/storage/src/paths.rs): the database and ordinary
  models/runtimes/audio live under `%APPDATA%\KnegisWhisper`. The database includes
  notes, transcripts, summaries, history, settings and saved voice data.
- [Recording paths](../crates/notes-core/src/recordings.rs): retained recordings
  and recovery files use its `meetings` subdirectory. User-selected exports and
  original imports are outside the uninstaller's deletion list.
- Models and scratch audio can fall back to
  `%ProgramData%\KnegisWhisper\cache\models` and `cache\audio` when the preferred
  path is not ASCII. These paths are shared, not partitioned by Windows account.
  Sidecar binaries stay in the per-user `sidecars` directory.
- [Logging setup](../apps/desktop/src-tauri/src/lib.rs) writes application tracing
  to its output stream. Files such as the streaming-server diagnostic log can
  live in the audio scratch directory. Redirected logs elsewhere are not removed.
- [Credential store](../crates/security/src/lib.rs): the service is
  `com.knegis.knegiswhisper`. Keyring 3.6.3's Windows backend forms a generic
  credential target as `username.service`, with the registry slot as username.
  Provider Clear and calendar Disconnect remove their entries; NSIS has no
  credential-removal calls.
- [Local API discovery](../crates/local-api/src/discovery.rs) uses
  `%USERPROFILE%\.knegiswhisper\local-api.json`. Normal server shutdown removes
  discovery data; an abandoned file is outside the NSIS deletion list and may
  contain a token. No real discovery file was opened during this audit.
- In the generated `installer.nsi`, `Section Uninstall` deletes the installed
  executable, bundled resources, uninstaller, ordinary shortcuts and uninstall
  registration. It does not recursively delete arbitrary contents of the install
  directory. Unknown/locked files may leave the directory behind.
- The data-removal block requires `DeleteAppDataCheckboxState = 1` and
  `UpdateMode <> 1`, then sets shell context to `current`. It targets only
  `$APPDATA\com.knegis.knegiswhisper` and
  `$LOCALAPPDATA\com.knegis.knegiswhisper`. Those may contain Tauri/WebView
  interface state; neither is the main application-data directory.
- `un.ConfirmShow` creates an initially unchecked checkbox; `un.ConfirmLeave`
  captures it. Silent `/S` skips interactive pages; passive `/P` skips the
  confirmation page. Neither supplies a selected checkbox. `/UPDATE` guards
  data removal even if a checkbox state were set. No custom uninstall hook
  changes these rules in the generated script.
- The installer requests administrator rights. With another account's elevation,
  `current` shell folders may belong to that account. That is a source-derived
  expectation, still requiring the two-account acceptance case.

## Disposable credential probe

A scratch Rust program depended on this worktree's `kw-security` and keyring
3.6.3. It refused to overwrite any existing entry, used a unique
`kneg-535-audit-<random>` slot, and wrote a fictional value through
`OsCredentialStore`. `cmdkey /list:<exact-target>` reported:

```text
Target: kneg-535-audit-dd43b04619b242429c6022546f97ee43.com.knegis.knegiswhisper
Type: Generic
User: kneg-535-audit-dd43b04619b242429c6022546f97ee43
```

The probe also checked the stored username through keyring attributes, deleted
only that entry, and verified `NoEntry` afterward. It did not enumerate or read
real credentials. The initial sandbox attempt could not write because Windows
returned `ERROR_NO_SUCH_LOGON_SESSION`; the scoped run outside the sandbox passed.
This verifies naming/presence/removal of the dummy entry, not uninstall behaviour
or the Credential Manager UI itself.

## Delivery checks and remaining acceptance

The user guide contains the retention matrix and scoped manual cleanup steps;
Known limitations links to it. RELEASING now specifies checked/unchecked,
silent/passive, upgrade, reinstall, locked-file, non-ASCII-account and
alternate-administrator cases with fictional data and other-user sentinels.
All 19 local documentation links/anchors passed inspection, and `git diff --check`
passed. The four changed documents have no trailing whitespace.

No clean Windows VM or separate machine was used. All installed-app cases remain
unrun in KNEG-534, including the published asset; none is counted as a pass here.
No uninstall or retained-data cleanup was performed on the owner's installation.
Scratch build logs, script snapshot, manifest and credential probe are retained
under ignored `.validation/` in this worktree; the evidence and hashes above are
committed so they do not depend on those local files remaining available.
