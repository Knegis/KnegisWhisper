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

import { verificationLabel } from "../lib/labels";
import {
  languageNames,
  localModelAction,
  localModelDetail,
  localModelLine,
  type LocalModelRow as Model,
} from "../lib/localModels";
import { LearnMore } from "./LearnMore";

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
  return (
    <li>
      {localModelLine(row)}
      <em className="row-detail">{localModelDetail(row)}</em>
      {row.loadedButGoesTo !== null ? (
        <em className="row-detail warn">Loaded, but dictation goes to {row.loadedButGoesTo}</em>
      ) : null}
      <LearnMore summary="Details">
        <p>
          {row.engine === "whisper"
            ? "Runs on the speech engine (whisper.cpp)."
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
      </LearnMore>
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
      {progress ? <em className="row-detail">{progress}</em> : null}
    </li>
  );
}
