# Persistent dictation microphone

On Windows, Settings > Transcription > Dictation microphone offers **Always show
dictation microphone**. It defaults to off. Enabling it replaces the dictation
recording box with a small 32px by 6px rectangle with a 2px corner radius while idle. Hover reveals a 20px microphone
inside a 52px by 28px button with a 2px corner radius; the hit target remains fixed during the
transition. Recording, processing, and error states remain expanded.
Click to start dictation and click again to finish. Existing hotkeys and the
meeting indicators retain their behavior.

The supplied microphone SVGs are bundled as theme-aware masks. Recording uses the
speaking icon even during silence, with the same blue selection color as settings. Transcription and insertion show a busy state
and reject duplicate actions. Failed operations show a dismissible message;
unknown backend state never presents a ready microphone.

Drag at least six CSS pixels to move the button without starting dictation.
Within 12 logical pixels of either side of the monitor work area, the button
turns vertical while its microphone icon remains upright. Moving away restores
the horizontal shape; orientation is derived again when restoring placement.
Placement is stored as logical offsets relative to the monitor, clamped to its
work area on restoration. A missing monitor falls back to bottom center. Reset
position clears the saved placement. Disabling the setting during recording
restores the original recording box without stopping recording.

The native presentation controller stores `dictation.floatingMicrophone` in the
settings database. Commands read/set preferences, reset position, and handle
dragging and message layout. Drag and message commands require the overlay
window. Dictation still runs through the existing state machine and insertion
pipeline. The window remains excluded from capture where supported. Windows
non-activation is configured in Tauri's window flags as well as checked through
the native extended style, so resizing does not silently make it focusable.

## Validation (Windows, 2026-09-10)

- Production frontend build and all 13 frontend tests passed.
- Desktop library tests with `tauri-test`: 387 passed, 6 ignored, none failed.
- Desktop strict all-target Clippy and native build passed using the CI profile.
- Settings, command, capability, feature, integration parity, and generated
  TypeScript contract checks passed.
- In a disposable app profile, native mouse clicks started/stopped recording
  while a separate text editor retained foreground focus. Dragging also retained
  focus without starting dictation. The idle and recording icons were inspected.
- Toggling off during recording restored the legacy indicator while recording
  continued. Saved placement survived restart; Reset position restored bottom
  center. The disabled idle overlay was hidden.
- A synthetic failure event exercised message presentation and dismissal. Native
  `WS_EX_NOACTIVATE` remained set after restart and message-window resizing.
  Diagnostic capture exclusion was reported active on this Windows system.

Screenshots and the native mouse harness log are local, ignored artifacts under
the repository's `target/microphone-validation/` directory. They are not shipped.
This profile had no configured transcription provider or local model: successful
speech-to-text insertion into another app is still a manual acceptance check.
Missing-monitor fallback and mixed-DPI clamping have code/unit-test coverage;
physical monitor disconnect and cross-DPI movement remain manual checks. Hosted
Linux/macOS CI has not been run for this commit; the setting is disabled there.
