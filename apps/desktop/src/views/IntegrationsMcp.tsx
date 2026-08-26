/**
 * Integrations > MCP — the exact stdio command line for
 * `tools/knegiswhisper-mcp`, and the one fact that matters most about it:
 * this server has no backend of its own. It is a second front end over the
 * same local HTTP API `Integrations > API` starts and `Integrations > CLI`
 * talks to — one tool per route, using the CLI's own client and
 * discovery-file reader rather than a second implementation of either. So it
 * can only do anything while this application is running, and the screen
 * says that plainly rather than leaving it to be discovered as a stream of
 * tool-call failures in whatever client it is configured into.
 *
 * Deliberately no tool count anywhere on this screen: a specific number
 * ("thirteen tools") is a claim nothing here verifies, and it silently rots
 * the moment `tools/knegiswhisper-mcp/src/tools.rs` grows a fourteenth. "One
 * tool per route" is true for as long as that remains this server's design,
 * with nothing to keep in sync.
 *
 * The binary's location is a real filesystem check, next to this running
 * application and then on `PATH` — reported as not found rather than assumed
 * present, the same discipline `Integrations > CLI` applies to
 * `knegiswhisper` itself.
 */

import { useCallback } from "react";

import { Row } from "../components/Row";
import { getMcpStatus } from "../lib/ipc";
import { binaryLocationLabel } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsMcp() {
  const load = useCallback(() => getMcpStatus(), []);
  const { data: status, error } = useRefresh(load);

  if (!status) {
    return (
      <div className="view">
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  const location = binaryLocationLabel(status.binaryLocation);
  const config = JSON.stringify(
    { command: status.command, args: [] },
    null,
    2,
  );

  return (
    <div className="view">
      {error ? <p className="error">{error}</p> : null}

      <section data-surface="integrations.mcp">
        <h2>MCP</h2>
        <p className="muted">
          <code>{status.binaryName}</code> is a stdio MCP server exposing this
          application's notes, transcripts and dictionary — one tool per local
          API route, with no backend of its own. It must be run by an MCP
          client, and it does nothing at all unless this application is
          running: every tool call is a request to the same local API that{" "}
          <code>Integrations &gt; API</code> starts.
        </p>

        <Row label="Binary" value={location.text} detail={location.detail} />
        <Row
          label="Local API"
          value={status.localApiRunning ? "Running" : "Not running"}
          detail={
            status.localApiRunning
              ? undefined
              : "every tool call will fail until this application is running — see Integrations > API"
          }
        />

        <p className="muted">
          Configure the client with this command and no arguments; it finds the
          running application's discovery file on its own, the same way the
          CLI does.
        </p>
        <pre className="preview">{config}</pre>
      </section>
    </div>
  );
}
