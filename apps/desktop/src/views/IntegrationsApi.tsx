/**
 * Integrations > API — the local HTTP server, off by default.
 *
 * The permissions line is a read of the file, not an assumption about it. On
 * Windows it says administrators can read it, because there is no `chmod` and
 * claiming owner-only would be false.
 */

import { useCallback } from "react";

import { Row } from "../components/Row";
import { getLocalApiStatus, startLocalApi, stopLocalApi } from "../lib/ipc";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsApi() {
  const load = useCallback(() => getLocalApiStatus(), []);
  const { data: api, error, guard } = useRefresh(load);

  if (!api) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="integrations.api">
        <h2>Local API</h2>
        <Row
          label="State"
          value={
            api.running ? `Listening on 127.0.0.1:${api.port}` : "Not listening"
          }
          detail={
            api.enabled && !api.running
              ? "switched on but failed to start"
              : undefined
          }
        />
        <p className="muted">
          Off by default. It exposes every note and transcript to anything
          running as you, so it only listens when you ask it to. It never binds
          anything but loopback.
        </p>
        {api.discoveryPath ? (
          <Row
            label="Discovery file"
            value={api.discoveryPath}
            detail={api.discoveryPermissions ?? undefined}
          />
        ) : null}
        {api.discoveryPermissionsAsExpected === false ? (
          <p className="error">
            The discovery file is more readable than intended. Anything that can
            read it can use the local API.
          </p>
        ) : null}
        <button
          type="button"
          onClick={() => {
            void guard(api.running ? stopLocalApi : startLocalApi);
          }}
        >
          {api.running ? "Stop the local API" : "Start the local API"}
        </button>
      </section>
    </div>
  );
}
