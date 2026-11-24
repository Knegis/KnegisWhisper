# Changelog

User-facing changes are recorded here. Published release notes identify the exact
version, supported platforms, installation requirements and known limitations.

## Unreleased

No installer release is claimed by this entry. See
[Releases](https://github.com/Knegis/KnegisWhisper/releases) for published builds.

- A meeting's note is saved before its speakers are analysed, so a failure or a crash during
  speaker labels no longer leaves a meeting with no note. The note says what happened and
  offers Retry speaker labels or Continue without speaker labels (KNEG-562).
- Quit from the tray asks before ending a meeting that is being recorded or saved, and can
  quit once the meeting is saved. Quitting no longer deletes the recording of a meeting in
  progress (KNEG-563).
- Starting KnegisWhisper a second time shows the copy that is already running (KNEG-564).
- Search fields, the note title and other text fields no longer draw a blue ring when clicked.
  They show focus by lifting to white. Buttons and checkboxes keep the ring for keyboard use.
- Speaker labels can be installed again. The English speaker model's download address had
  started serving a different file, which the install rightly refused; it now downloads the
  original file from a fixed address. The model itself is unchanged (KNEG-546).
- Quitting while a dictation is being transcribed or inserted waits up to ten seconds for it
  to finish, so the text is inserted and kept in history. A dictation still being recorded
  when you quit is dropped, as before (KNEG-568).
- If KnegisWhisper cannot start, Windows now shows a message saying why and where the log
  file is. Before, the application simply did not open (KNEG-570).
- On Windows, local model servers end when KnegisWhisper quits. They used to keep running,
  holding their models in memory, until the next launch (KNEG-569).
- The OpenWhispr import lists the tables it did not copy, with row counts, and how many rows
  it passed over because they were deleted in OpenWhispr. Its description no longer says the
  source file is never touched: the import only reads your data and changes none of it, but
  SQLite may update a small index file beside the database (KNEG-507).

## 0.2.0 — 2026-11-13

The second Windows x64 preview.

- Connect Google Calendar or Outlook (Outlook.com, Hotmail and Microsoft 365) in
  Settings > Calendar. Builds that carry KnegisWhisper's own sign-in clients connect in
  one click. Any build can use a custom client under Advanced. A calendar syncs a
  bounded snapshot when you press Sync now, and when Meetings opens, at most once per
  5 minutes per provider. It never syncs at launch (KNEG-328, KNEG-551 to KNEG-555).
- Meetings lists upcoming events from the local calendar copy. A recording can be
  linked to the calendar event it was made in. The link is chosen at Start and can be
  changed or removed while recording. Overlapping events are offered, never guessed
  (KNEG-366).
- A personal Google account and a personal Outlook.com account have connected and synced
  in a development build. Outlook sign-ins are longer than one Windows credential entry can
  hold, so they are split across several entries; before this, Outlook Connect failed with
  "Could not save or read calendar data" (KNEG-559). Work or school Microsoft accounts may
  need an administrator to approve KnegisWhisper first. See the known limitations.
- Check GitHub once a day for a newer KnegisWhisper release and offer to open its release page. Turn it off with Check for updates automatically in Settings > Diagnostics. Nothing is downloaded or installed, prereleases count, and the check no longer needs an updater signing key (KNEG-558).
- A new Windows uninstall helper offers two choices, both unchecked: delete your own
  KnegisWhisper data, and delete shared downloads and temporary audio for all users.
  Upgrades and silent uninstalls keep all data. Its acceptance on a disposable machine
  is still pending (KNEG-545).
- Draw the remaining fields, dropdown lists and dialogs in the new style (KNEG-549, KNEG-520).

- Align settings labels and fields, use consistent control sizes and readable form values, keep company-account labels and hints visible, and show filled actions in the unsaved-settings dialog (KNEG-550).

- Keep scratch audio in a verified private directory for each user and profile, including accounts whose names contain non-ASCII characters.

- Bind custom server keys to their configured server addresses. Existing custom keys must be re-entered in Settings > Providers; changing a server address also requires re-entering the key.

- Escape terminal control characters when displaying notes and transcripts in the CLI; preserve original content in pipes and exported files.

- Reject import paths that were not selected in the file picker before accessing their filesystem.

- Keep opening imported notes responsive when participant fields contain long runs of whitespace.

- Reject malformed Corti environment settings before sending credentials or opening a connection.

- Use Ndot-57 for compact labels and dates, keeping Doto for the meeting clock and other large numeric displays.

- Keep a page-shaped loading layout when opening Dictation or Meetings, with a brief fade and reduced-motion support (KNEG-547).

- Return to the original task or note when closing Settings, while protecting unsaved changes (KNEG-548).

- Write release checksums with portable LF line endings (KNEG-536).
- Keep the library toggle clear of the narrow transcript header (KNEG-515).
- Bundle generated dependency notices and verify their freshness in CI (KNEG-533).
- Install speech and segmentation models from verified fixed-commit mirrors,
  preserving existing model weights and installed layouts (KNEG-532).

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
