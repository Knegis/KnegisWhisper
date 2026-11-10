# Windows uninstall implementation (KNEG-545)

Implemented on `codex/kneg-545`, based on documentation commit `44fa669` and
runtime source `26220ba`. This is an unreleased change. The installed-machine
acceptance matrix remains pending in [KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545).
The [earlier audit](UNINSTALL_AUDIT.md) describes v0.1.1 and the pre-helper code.

## Behaviour and boundaries

Windows Installed apps launches `knegiswhisper-uninstall.exe`. The shipped PE
manifest explicitly says `asInvoker`; it also includes Common Controls v6 for
the native dialogs. Two unchecked options and a final confirmation precede UAC.
Already-elevated interactive launches fail without deleting anything.

The original process performs selected personal cleanup under its own token.
It obtains the roaming/local/profile paths from Windows Known Folders, never
from a caller's arguments or the administrator's environment. Scope:

- `%APPDATA%\KnegisWhisper`, including the database, voice data, recordings,
  recovery files, runtimes, downloads and scratch audio.
- `%APPDATA%\com.knegis.knegiswhisper` and its LocalAppData equivalent.
- Only `local-api.json` inside `%USERPROFILE%\.knegiswhisper`.
- Generic Credential Manager entries whose target is exactly
  `<slot>.com.knegis.knegiswhisper` and whose username is that valid slot. This
  includes retired slots. The native API has only prefix filtering, so the helper
  filters target/user metadata without reading blobs or logging credentials.
- Only the `KnegisWhisper` value in this user's Run and StartupApproved/Run keys.

The elevated broker performs separately selected cleanup of
`%ProgramData%\KnegisWhisper\cache`, then runs the installed NSIS uninstaller.
It never removes an administrator's personal folders or credentials on behalf
of another user. Other users' personal data, exports and original imports are
outside the cleanup scope. Unchecked options, `--quiet`, direct NSIS invocation
and upgrades retain personal/shared data, including startup settings.

The helper checks the executable's location against the machine's registered
installation. A random, local-only named pipe has a restricted DACL and first-
instance protection. The parent checks the elevated child PID against its
retained process handle. The broker pins the parent process, checks its image
path and pipe-server PID, and rejects an elevated interactive parent. Client
SQOS prevents the pipe server from impersonating the elevated broker. The only
request byte is KEEP or DELETE SHARED; the protocol accepts no paths or commands.
UAC refusal occurs before any data deletion.

Filesystem cleanup opens each ancestor and target without write/delete sharing,
rejects all reparse points (including dangling ones), and deletes through the
validated object handle. Attribute-only access proved insufficient in a native
regression test; the implementation uses actual read access for sharing rules.
It never recursively deletes a profile, AppData or ProgramData root. Locked,
read-only or inaccessible objects produce errors, not success. Retry is safe
after partial deletion; Cancel cannot restore deleted files.

The helper checks for active app/speech processes and does not kill them. This
check is conservative: a running process with a known speech-runtime executable
name can block uninstall even if another application started it. The NSIS
uninstall section also refuses an active desktop app rather than killing it.

## Installer integration

`tools/uninstall/generate.py` verifies the pristine Tauri CLI 2.11.4 template
hash and locked CLI version, then emits the customized template. Upgrades read
`NSISUninstallString`, falling back to the legacy entry for older installations.
Direct NSIS uninstall offers no data-deletion checkbox and says it retains data.
There is no elevated HKCU data cleanup in the generated script.

Running helper/uninstaller images are renamed to unique temporary filenames
before delayed deletion. This avoids scheduling deletion of a new helper after
a reinstall at the same path. Reboot-required completion returns 3010 and never
restarts Windows automatically. Known packaged files that cannot be removed
cause failure before uninstall registration is removed. Installed-account and
reboot behaviour still require the matrix below.

The Windows-only build feature keeps the helper out of non-Windows bundles.
CI checks template freshness and helper tests/Clippy. Release builds additionally
inspect the actual helper PE manifest and generated NSIS inclusion/entry points.
Upstream template attribution and its MIT license are included in installed NOTICE.

## Validation and release gate

Local validation on Windows, Rust 1.99.0:

- Helper tests cover deletion bounds/idempotency, non-ASCII paths, missing
  ancestors, locks/read-only files, pinned-directory rename exclusion, root/
  child/ancestor/dangling junction rejection, exact credential matching, real
  local pipe exchange and rejection of an unrelated peer executable. The test
  child entry is fixture-only; no test calls whole-profile or full uninstall.
- A separate interactive-session test created random, fictional credentials,
  removed matching active/obsolete targets and retained an unrelated sentinel.
  Existing application credentials could not match its random test prefix.
- Template contract tests, CI selection tests, feature/binary/domain checks,
  formatting, helper/test Clippy, frontend typecheck and production build, and
  the pinned cargo-about notices freshness check passed.
- A new unsigned Windows x64 debug NSIS installer was built. Generated-script
  and PE-manifest verification are recorded with its artifact hashes below.

This is **not** a completed installed-machine uninstall test. No uninstall ran
against the owner's application, profiles or credentials. No disposable VM tool
was available on PATH. Same-user and alternate-admin UAC, real Windows Settings
entry, interactive choices/cancel, upgrade/reinstall, locked data, restart
completion, other-user retention and silent/direct modes remain mandatory in
[RELEASING.md](RELEASING.md#windows-uninstall-acceptance). Keep KNEG-545 open until
these pass. The separate published-v0.1.1 tests remain in KNEG-534.

The local debug artifact keeps the source version 0.1.1 for validation only.
Ship this change under a new release version; do not replace the published
v0.1.1 asset. Local build logs and fixture outputs are under `.validation/`.

### Local debug artifact, 2026-11-10

The frontend was built and typechecked explicitly with Node. The local Tauri
override skipped the redundant frontend hook and disabled updater artifacts;
installer settings and helper code came from this worktree. The npm wrapper
could not resolve `tsc` through the reused node_modules junction.

The PE/script verification passed. A production-helper startup probe with
`--quiet --unsupported-load-probe` returned the expected argument error (1)
before any installation lookup or cleanup. This verifies loading, not uninstall.

| Artifact under target/debug | Bytes | SHA-256 |
| --- | ---: | --- |
| `bundle\nsis\KnegisWhisper_0.1.1_x64-setup.exe` | 9,881,031 | `17312497f8e52c2e7b99753ad1f550f7ce8a1e900f77783a9554064ae4627fc9` |
| `knegiswhisper-uninstall.exe` | 524,288 | `7b141d36c2060c7d8cc6076dbd35a64024dceed22df816cac5324799a6343c6f` |
| `nsis\x64\installer.nsi` | 30,667 | `6617d5bab982acce3f3fc2af411fad05ac776fa4cdd5f0dd9610a185c8ca9980` |
