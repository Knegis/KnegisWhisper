/**
 * Developer > MCP — the exact stdio command line for
 * `tools/knegiswhisper-mcp`, and the one fact that matters most about it:
 * this server has no backend of its own. It is a second front end over the
 * same local HTTP API `Developer > Local API` starts and `Developer > CLI`
 * talks to — one tool per route, using the CLI's own client and
 * discovery-file reader rather than a second implementation of either. So it
 * can only do anything while the Local API is on, and the screen says that
 * plainly rather than leaving it to be discovered as a stream of tool-call
 * failures in whatever client it is configured into.
 *
 * Deliberately no tool count anywhere on this screen: a specific number
 * ("thirteen tools") is a claim nothing here verifies, and it silently rots
 * the moment `tools/knegiswhisper-mcp/src/tools.rs` grows a fourteenth. "One
 * tool per route" is true for as long as that remains this server's design,
 * with nothing to keep in sync.
 *
 * The binary's location is a real filesystem check, next to this running
 * application and then on `PATH` — reported as not found rather than assumed
 * present, the same discipline `Developer > CLI` applies to
 * `knegiswhisper` itself.
 *
 * In plain words (KNEG-343): the binary and the configuration stay in view.
 * Its Local API row said tool calls fail "until this application is running";
 * they fail until the Local API is on, which is the condition that matters.
 * And it says that what a tool reads goes to the AI assistant, which may send
 * it off this computer.
 */

import { useCallback } from "react";

import { DiscoveryFileRows } from "../components/DiscoveryFileRows";
import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { getMcpStatus } from "../lib/ipc";
import { binaryLocationLabel, blockedLine } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsMcp() {
  const load = useCallback(() => getMcpStatus(), []);
  const { data: status, error } = useRefresh(load);

  if (!status) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
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
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      {status.blockedReason ? <FailureLine text={blockedLine(status.blockedReason)} detail={status.blockedReason} /> : null}

      <section data-surface="integrations.mcp">
        <p className="muted">
          <code>{status.binaryName}</code> lets an AI assistant that speaks MCP
          work with your notes, transcripts and dictionary. Your assistant runs
          it, and it works only while this app is running with the Local API
          turned on.
        </p>
        <LearnMore summary="How it works">
          <p>
            It is a stdio MCP server with one tool per local API route and no
            backend of its own: every tool call is a request to the same local
            API that Developer › Local API starts.
          </p>
        </LearnMore>

        <Row label="Binary" value={location.text} detail={location.detail} />
        <p className="muted">
          Tools can create and export notes, transcribe files, check or cancel
          jobs, list models and control meeting recording. What a tool reads goes
          to your AI assistant, which may send it off this computer. File
          transcription uses your Transcription setting.
        </p>
        <LearnMore summary="What the tools can reach">
          <p>
            Uploads read any file the MCP server process can read. Recording
            tools control the visible meeting recorder, including meetings
            started here.
          </p>
        </LearnMore>
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
              : "every tool call fails until it is on — turn it on under Developer › Local API"
          }
        />

        <p className="muted">
          Add this to your assistant's MCP settings. It needs no arguments: it
          finds this app by itself, the way the CLI does.
        </p>
        <pre className="preview">{config}</pre>
      </section>
    </div>
  );
}
