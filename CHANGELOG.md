# Changelog

User-facing changes are recorded here. Published release notes identify the exact
version, supported platforms, installation requirements and known limitations.

## Unreleased

- Keep a page-shaped loading layout when opening Dictation or Meetings, with a brief fade and reduced-motion support (KNEG-547).

- Return to the original task or note when closing Settings, while protecting unsaved changes (KNEG-548).

- Write release checksums with portable LF line endings (KNEG-536).
- Keep the library toggle clear of the narrow transcript header (KNEG-515).
- Bundle generated dependency notices and verify their freshness in CI (KNEG-533).
- Install speech and segmentation models from verified fixed-commit mirrors,
  preserving existing model weights and installed layouts (KNEG-532).

No installer release is claimed by this entry. See
[Releases](https://github.com/Knegis/KnegisWhisper/releases) for published builds.

## 0.1.1 — 2026-11-09

The first Windows x64 preview. `v0.1.0` was tagged and built as a draft but never
published; this version replaces it.

- Product-focused README, first-use guides and contribution templates.
- A Windows x64 preview release process with version checks, checksums and draft review.
- Parakeet TDT 0.6B installs from its current upstream download, which was rebuilt
  on 2026-11-07. The unpublished `v0.1.0` build expected the old file and refused it.
- Scrollbars are a slim rounded handle with no arrows, and stay inside the rounded
  note and Settings sheets.
- The note editor's add and drag icons are centred in their highlight and stay
  visible when pointed at.
- Settings > Meetings has a Known voices section, for recognising remembered
  participants after a meeting. It is Windows-only and can't be switched on in this
  version: its matching hasn't been validated on labelled recordings yet.
- Upgrading deletes the per-meeting voice samples an OpenWhispr import copied in,
  and the import no longer copies them. Voice profiles and names saved in notes are
  kept.
