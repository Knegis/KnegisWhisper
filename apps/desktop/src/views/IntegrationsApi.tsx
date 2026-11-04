/**
 * Developer > Local API — the local HTTP server, off by default.
 *
 * The permissions line is a read of the file, not an assumption about it, and
 * it is rendered through the same `DiscoveryPermissions` state and the same
 * `discoveryPermissionsLabel` in `lib/labels.ts` that `Developer > CLI`
 * uses — one vocabulary for one file, on both screens that describe it.
 *
 * This screen used to warn only when a boolean `discoveryPermissionsAsExpected`
 * was `false`, and that boolean was `true` for `InheritedFromProfile` — this
 * platform's profile-directory default, not a restriction anything here
 * verified. So the screen reassured on exactly the state a bench check found
 * two extra trustees able to read on a real machine. The boolean is gone;
 * `InheritedFromProfile` now gets the same honest sentence and caveat here
 * that it gets on `Developer > CLI`. See docs/BENCH_CHECK.md, 2026-08-26
 * finding 2.
 *
 * In plain words (KNEG-343): the address and the discovery file's path stay in
 * view, because on this screen they are what a developer needs. A failure is a
 * sentence with the reason under Details, and what the API exposes is said
 * without "binds" and "loopback".
 */

import { useCallback } from "react";

import { DiscoveryPermissionRows } from "../components/DiscoveryFileRows";
import { FailureLine } from "../components/FailureLine";
import { Row } from "../components/Row";
import { getLocalApiStatus, startLocalApi, stopLocalApi } from "../lib/ipc";
import { blockedLine } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsApi() {
  const load = useCallback(() => getLocalApiStatus(), []);
  const { data: api, error, guard } = useRefresh(load);

  if (!api) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="view">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      {api.blockedReason ? <FailureLine text={blockedLine(api.blockedReason)} detail={api.blockedReason} /> : null}

      <section data-surface="integrations.api" className="dev-page dev-page-api">
        <div className="api-hero" data-on={api.running}>
          <span className="section-label">state</span>
          <span className="api-hero-dot" aria-hidden="true"><span /></span>
          <p className="api-hero-state">
            {api.running ? `Listening on 127.0.0.1:${api.port}` : "Not listening"}
          </p>
          {api.enabled && !api.running ? (
            <p className="api-hero-detail">switched on but failed to start</p>
          ) : null}
          {/* Off, there is no port to show: the address says only where it would listen. */}
          <p className="api-hero-address" aria-hidden="true">
            {api.port !== null ? `127.0.0.1:${api.port}` : "127.0.0.1"}
          </p>
          <button
            type="button"
            className="api-hero-action"
            onClick={() => {
              void guard(api.running ? stopLocalApi : startLocalApi);
            }}
          >
            {api.running ? "Stop the local API" : "Start the local API"}
          </button>
        </div>
        <p>
          Off by default. While it is on, any program you run can read every
          note and transcript through it, so it listens only when you turn it on,
          and only to this computer.
        </p>
        <div className="row-group">
          {api.discoveryPath ? (
            <Row label="Discovery file" code value={api.discoveryPath} />
          ) : null}
          {api.discoveryPermissions ? <DiscoveryPermissionRows permissions={api.discoveryPermissions} /> : null}
        </div>
      </section>
    </div>
  );
}
