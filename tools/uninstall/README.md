# Windows uninstall template

`upstream.nsi` is the unmodified template from Tauri CLI **2.11.4**:
https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi

It is used under the upstream MIT license, reproduced in `LICENSE_MIT`.
`generate.py` verifies its SHA-256 and the locked CLI version, then applies the
small project patch to `apps/desktop/src-tauri/installer.nsi`.

Run `python tools/uninstall/generate.py` to regenerate. CI runs `--check` and
the contract tests. A CLI upgrade requires reviewing the new template, changing
the pin, regenerating, building NSIS and repeating the uninstall acceptance
matrix in `docs/RELEASING.md`.

The helper is a separate Cargo binary, enabled by `windows-uninstall` only in
`tauri.windows.conf.json`. It has no webview, application state or network
startup. Its production manifest is `asInvoker` with Common Controls v6.
The explicit helper tests use `tauri-test` to embed v6 in the test executable:

```
cargo test -p knegiswhisper --bin knegiswhisper-uninstall --features tauri-test,windows-uninstall
```

The tests delete only disposable fixture directories. None invokes the complete
uninstall or personal/shared cleanup entry points against the developer's profile.

`python tools/uninstall/test_windows.py -v` compiles the generated NSIS entry-point
removal fragment and executes it against disposable files. It covers a locked
uninstaller, a locked helper requiring rollback, and successful removal. Each
failure must leave both images intact and the helper launchable; retry must work
after unlocking. It uses Tauri's cached NSIS compiler, or `NSIS_MAKENSIS` when set.
The release workflow runs it after building the installer. It changes no registry
entries or personal data and does not test ARP/UAC or restart-delayed deletion.

Both the PE `asInvoker` manifest and the per-machine registration's
`NoElevateOnModify=1` are required. Programs and Features can otherwise elevate
the caller before starting the helper. Verify both Windows uninstall interfaces
in the disposable-machine release matrix, including an upgraded registration.
