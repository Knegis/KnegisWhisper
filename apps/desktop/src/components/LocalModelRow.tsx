/**
 * One row of Settings > Transcription's local speech models (KNEG-330).
 *
 * Presentational: the words come from `lib/localModels.ts`, and the buttons'
 * work is the screen's. The settings gate's rule F looks for the IPC wrappers
 * in `SettingsTranscription.tsx` itself, so they stay there and arrive here
 * as `onUse` and `onInstall`.
 *
 * The model id, the engine's name and how a download is checked go under
 * Details (KNEG-343): the row leads with what a person chooses by.
 */

import { useState } from "react";

import { verificationLabel } from "../lib/labels";
import {
  languageNames,
  localModelAction,
  localModelChips,
  modelThumb,
  type LocalModelRow as Model,
} from "../lib/localModels";

export function LocalModelRow({
  row,
  progress,
  onUse,
  onInstall,
}: {
  row: Model;
  /** The install progress line, on the row whose Install started it. */
  progress: string | null;
  onUse: () => void;
  onInstall: () => void;
}) {
  const [open, setOpen] = useState(false);
  const thumb = modelThumb(row);
  const detailsId = `model-details-${row.id}`;
  return (
    <li className="model-row">
      <span className="model-thumb" data-kind={thumb.kind} data-variant={thumb.variant} aria-hidden="true" />
      <span className="model-row-name">{row.name}</span>
      <span className="model-row-action">
        {row.installed ? (
          <button
            type="button"
            className={row.inUse ? "marked" : undefined}
            aria-pressed={row.inUse}
            onClick={onUse}
          >
            {localModelAction(row)}
          </button>
        ) : (
          <button type="button" onClick={onInstall}>
            {localModelAction(row)}
          </button>
        )}
      </span>
      <span className="model-row-chips">
        {localModelChips(row).map((chip) => (
          <span key={chip.text} className="model-chip" data-tone={chip.tone}>{chip.text}</span>
        ))}
        <button
          type="button"
          className="model-chip model-chip-details"
          aria-expanded={open}
          aria-controls={detailsId}
          onClick={() => setOpen((was) => !was)}
        >
          Details
        </button>
      </span>
      <div id={detailsId} className="model-row-details learn-more-body" hidden={!open}>
        <p>
          {row.engine === "whisper"
            ? "For dictation, imports and meetings. Runs on the speech engine (whisper.cpp)."
            : "Runs on the Parakeet engine (sherpa-onnx)."}{" "}
          Model id <code>{row.id}</code>.
          {row.verifiedBy !== null
            ? ` ${row.installed ? "When it was downloaded" : "When it is downloaded"}: ${verificationLabel(row.verifiedBy)}.`
            : null}
        </p>
        {/* The row says "Many languages" for more than two; here is which. */}
        {row.languages !== "many" && row.languages.length > 2 ? (
          <p>Languages: {languageNames(row.languages).join(", ")}.</p>
        ) : null}
      </div>
      {progress ? <em className="row-detail model-row-progress">{progress}</em> : null}
    </li>
  );
}
