/**
 * Integrations > CLI — where `knegiswhisper` is, where it looks for the
 * running application, and the real state of the file that lets it in.
 *
 * The one thing this screen exists to get right after `Integrations > API`
 * got it wrong: the discovery file's permissions are shown as the actual
 * state and its evidence, never as a boolean. A bench check found that the
 * backend's own `Permissions::is_as_expected()` returned `true` for
 * `InheritedFromProfile` — correct as "the best this build currently
 * achieves", and wrong read as "verified restricted", since nothing checked
 * what that inherited ACL actually grants. That helper has since been
 * deleted. This screen switches on the real state
 * (`discoveryPermissionsLabel` in `lib/labels.ts`), which now has a sentence
 * for each of the ten states a read-back can produce rather than a green tick
 * borrowed from a boolean that could not tell them apart.
 *
 * The binary's location is a real filesystem check too, next to this running
 * application and then on `PATH` — never assumed present because the CLI is
 * "supposed to be there".
 */

import { useCallback } from "react";

import { DiscoveryFileRows } from "../components/DiscoveryFileRows";
import { Row } from "../components/Row";
import { getCliStatus } from "../lib/ipc";
import { binaryLocationLabel } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsCli() {
  const load = useCallback(() => getCliStatus(), []);
  const { data: status, error } = useRefresh(load);

  if (!status) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const location = binaryLocationLabel(status.binaryLocation);
  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      {status.blockedReason ? <p className="error" role="alert">{status.blockedReason}</p> : null}

      <section data-surface="integrations.cli">
        <p className="muted">
          <code>{status.binaryName}</code> is a compiled command-line client over
          the same local API as this settings window — notes, transcripts, the
          dictionary, file transcription jobs, exports and meeting recording. It talks to this running application over the discovery
          file below, so it can only do anything while the application is
          running.
        </p>

        <Row label="Binary" value={location.text} detail={location.detail} />
        <p className="muted">
          Start with <code>knegiswhisper status</code> or <code>knegiswhisper models list</code>.
          Use <code>transcribe file PATH</code> to upload media, then <code>jobs status ID</code> to retrieve the result.
          File transcription uses your configured provider. <code>recording start --provider ID</code> starts a visible microphone meeting recording.
        </p>
        <DiscoveryFileRows
          discoveryPath={status.discoveryPath}
          permissions={status.discoveryPermissions}
        />
        <Row
          label="Local API"
          value={status.localApiRunning ? "Running" : "Not running"}
          detail={
            status.localApiRunning
              ? undefined
              : "the discovery file above does not exist while this is off — see Developer > Local API"
          }
        />
      </section>
    </div>
  );
}
