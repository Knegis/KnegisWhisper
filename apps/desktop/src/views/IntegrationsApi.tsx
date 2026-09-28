/**
 * Integrations > API — the local HTTP server, off by default.
 *
 * The permissions line is a read of the file, not an assumption about it, and
 * it is rendered through the same `DiscoveryPermissions` state and the same
 * `discoveryPermissionsLabel` in `lib/labels.ts` that `Integrations > CLI`
 * uses — one vocabulary for one file, on both screens that describe it.
 *
 * This screen used to warn only when a boolean `discoveryPermissionsAsExpected`
 * was `false`, and that boolean was `true` for `InheritedFromProfile` — this
 * platform's profile-directory default, not a restriction anything here
 * verified. So the screen reassured on exactly the state a bench check found
 * two extra trustees able to read on a real machine. The boolean is gone;
 * `InheritedFromProfile` now gets the same honest sentence and caveat here
 * that it gets on `Integrations > CLI`. See docs/BENCH_CHECK.md, 2026-08-26
 * finding 2.
 */

import { useCallback } from "react";

import { Row } from "../components/Row";
import { getLocalApiStatus, startLocalApi, stopLocalApi } from "../lib/ipc";
import { discoveryPermissionsLabel } from "../lib/labels";
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

  const permissions = api.discoveryPermissions
    ? discoveryPermissionsLabel(api.discoveryPermissions)
    : null;

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      {api.blockedReason ? <p className="error" role="alert">{api.blockedReason}</p> : null}

      <section data-surface="integrations.api">
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
          <Row label="Discovery file" value={api.discoveryPath} />
        ) : null}
        {permissions ? (
          <Row
            label="Discovery file permissions"
            value={permissions.text}
            detail={permissions.detail}
          />
        ) : null}
        {permissions?.caveat ? (
          <p className="caveat">{permissions.caveat}</p>
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
