/**
 * Integrations > CLI — where `knegiswhisper` is, where it looks for the
 * running application, and the real state of the file that lets it in.
 *
 * The one thing this screen exists to get right after `Integrations > API`
 * got it wrong: the discovery file's permissions are shown as the actual
 * state and its evidence, never as a boolean. A bench check found that the
 * backend's own `Permissions::is_as_expected()` returns `true` for
 * `InheritedFromProfile` — correct as "the best this build currently
 * achieves", and wrong read as "verified restricted", since nothing checked
 * what that inherited ACL actually grants. This screen switches on the real
 * state (`discoveryPermissionsLabel` in `lib/labels.ts`) and gives
 * `InheritedFromProfile` its own honest sentence rather than a green tick
 * borrowed from a boolean that cannot tell the difference.
 *
 * The binary's location is a real filesystem check too, next to this running
 * application and then on `PATH` — never assumed present because the CLI is
 * "supposed to be there".
 */

import { useCallback } from "react";

import { Row } from "../components/Row";
import { getCliStatus } from "../lib/ipc";
import { binaryLocationLabel, discoveryPermissionsLabel } from "../lib/labels";
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
  const permissions = status.discoveryPermissions
    ? discoveryPermissionsLabel(status.discoveryPermissions)
    : null;

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="integrations.cli">
        <h2>CLI</h2>
        <p className="muted">
          <code>{status.binaryName}</code> is a compiled command-line client over
          the same local API as this settings window — notes, transcripts, the
          dictionary. It talks to this running application over the discovery
          file below, so it can only do anything while the application is
          running.
        </p>

        <Row label="Binary" value={location.text} detail={location.detail} />
        <Row label="Discovery file" value={status.discoveryPath} />
        <Row
          label="Local API"
          value={status.localApiRunning ? "Running" : "Not running"}
          detail={
            status.localApiRunning
              ? undefined
              : "the discovery file above does not exist while this is off — see Integrations > API"
          }
        />
        {permissions ? (
          <Row
            label="Discovery file permissions"
            value={permissions.text}
            detail={permissions.detail}
          />
        ) : (
          <p className="muted">
            Nothing to report about the discovery file's permissions: it does
            not exist while the local API is not running.
          </p>
        )}
        {status.discoveryPermissions?.state === "inherited-from-profile" ? (
          <p className="caveat">
            This is not the same as the file having been locked down on its
            own. Anything that can read files your profile can read — which,
            on this platform, includes administrators — can use the local API.
          </p>
        ) : null}
      </section>
    </div>
  );
}
