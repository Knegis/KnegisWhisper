/**
 * Developer > CLI — where `knegiswhisper` is, where it looks for the
 * running application, and the real state of the file that lets it in.
 *
 * The one thing this screen exists to get right after `Developer > Local API`
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
 *
 * In plain words (KNEG-343): the binary, its path and the commands stay in
 * view, because they are what a developer types. It said the CLI works "while
 * the application is running"; it needs the Local API on as well, which only
 * the row at the foot said, so the first sentence says it now.
 */

import { useCallback } from "react";

import { DiscoveryFileRows } from "../components/DiscoveryFileRows";
import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { getCliStatus } from "../lib/ipc";
import { binaryLocationLabel, blockedLine } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsCli() {
  const load = useCallback(() => getCliStatus(), []);
  const { data: status, error } = useRefresh(load);

  if (!status) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const location = binaryLocationLabel(status.binaryLocation);
  return (
    <div className="view">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      {status.blockedReason ? <FailureLine text={blockedLine(status.blockedReason)} detail={status.blockedReason} /> : null}

      <section data-surface="integrations.cli">
        <p className="muted">
          <code>{status.binaryName}</code> is a command-line client for your
          notes, transcripts, dictionary, file transcription, exports and meeting
          recording. It works only while this app is running with the Local API
          turned on.
        </p>
        <LearnMore summary="How it connects">
          <p>
            It talks to the same local API as this window, and finds it through
            the discovery file below.
          </p>
        </LearnMore>

        <Row label="Binary" value={location.text} detail={location.detail} />
        <p className="muted">
          Start with <code>knegiswhisper status</code> or <code>knegiswhisper models list</code>.
          Use <code>transcribe file PATH</code> to upload media, then <code>jobs status ID</code> to retrieve the result.
          File transcription uses your Transcription setting: with a cloud provider, the audio leaves this computer.{" "}
          <code>recording start --provider ID</code> starts a visible microphone meeting recording.
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
              : "turn it on under Developer › Local API; the discovery file above exists only while it runs"
          }
        />
      </section>
    </div>
  );
}
