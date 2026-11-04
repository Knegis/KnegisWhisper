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

import { BinaryRow, DiscoveryFileRows, LocalApiRow } from "../components/DiscoveryFileRows";
import { FailureLine } from "../components/FailureLine";
import { LearnMore } from "../components/LearnMore";
import { Row } from "../components/Row";
import { getMcpStatus } from "../lib/ipc";
import { blockedLine } from "../lib/labels";
import { useRefresh } from "../lib/useRefresh";

export function IntegrationsMcp({ onGoToApi }: { onGoToApi: () => void }) {
  const load = useCallback(() => getMcpStatus(), []);
  const { data: status, error } = useRefresh(load);

  if (!status) {
    return (
      <div className="view">
        {error ? <FailureLine text="This page couldn't load." detail={error} /> : <p className="muted">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="view">
      {error ? <FailureLine text="That didn't work." detail={error} /> : null}

      {status.blockedReason ? <FailureLine text={blockedLine(status.blockedReason)} detail={status.blockedReason} /> : null}

      <section data-surface="integrations.mcp" className="dev-page">
        {/* The wires only run while the thing they lead to is up. */}
        <figure className="mcp-figure" data-live={status.localApiRunning}>
          <div className="mcp-node">
            <span className="mcp-disc mcp-disc-assistant" aria-hidden="true" />
            <span>Your assistant</span>
          </div>
          <div className="mcp-wire" aria-hidden="true"><span /></div>
          <div className="mcp-node">
            <span className="mcp-disc mcp-disc-server" aria-hidden="true" />
            <span className="mcp-node-code">knegiswhisper-mcp</span>
            <span className="mcp-node-note">stdio</span>
          </div>
          <div className="mcp-wire" aria-hidden="true"><span /></div>
          <div className="mcp-node">
            <span className="mcp-disc mcp-disc-api" aria-hidden="true" />
            <span>Local API</span>
          </div>
        </figure>

        <p>
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

        <div className="row-group">
          <BinaryRow location={status.binaryLocation} />
          <Row
            label="Tools"
            value={
              <span className="row-para">
                Tools can create and export notes, transcribe files, check or cancel
                jobs, list models and control meeting recording. What a tool reads goes
                to your AI assistant, which may send it off this computer. File
                transcription uses your Transcription setting.
              </span>
            }
          >
            <LearnMore summary="What the tools can reach">
              <p>
                Uploads read any file the MCP server process can read. Recording
                tools control the visible meeting recorder, including meetings
                started here.
              </p>
            </LearnMore>
          </Row>
          <DiscoveryFileRows
            discoveryPath={status.discoveryPath}
            permissions={status.discoveryPermissions}
          />
          <LocalApiRow
            running={status.localApiRunning}
            detail="every tool call fails until it is on — turn it on under Developer › Local API"
            onOpen={onGoToApi}
          />
        </div>

        <div className="mcp-config">
          <p>
            Add this to your assistant's MCP settings. It needs no arguments: it
            finds this app by itself, the way the CLI does.
          </p>
          <pre className="preview code-panel" tabIndex={0} aria-label="MCP settings">
            <code>
              <span className="code-punct">{"{"}</span>
              {"\n  "}
              <span className="code-key">"command"</span>
              <span className="code-punct">:</span>{" "}
              <span className="code-string">{JSON.stringify(status.command)}</span>
              <span className="code-punct">,</span>
              {"\n  "}
              <span className="code-key">"args"</span>
              <span className="code-punct">:</span>{" "}
              <span className="code-punct">[]</span>
              {"\n"}
              <span className="code-punct">{"}"}</span>
            </code>
          </pre>
        </div>
      </section>
    </div>
  );
}
