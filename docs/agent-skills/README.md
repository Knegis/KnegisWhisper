# Agent skills

Two skills that let a coding agent drive KnegisWhisper on the machine it is
running on: one for the HTTP local API, one for the `knegiswhisper` CLI and the
`knegiswhisper-mcp` server.

They replace the two upstream skills recorded under `developer:agent-skills` in
`config/integration-manifest.json`, which addressed a hosted service. **There is
no hosted service here.** Every endpoint in these skills is served by the
desktop application on loopback, authenticated by a token that never leaves the
machine, and none of it works unless the user has switched the local API on.

## The rule these follow

Everything in both files was read out of the code that serves it — the route
table in `crates/local-api/src/server.rs`, the request shapes in
`crates/local-api/src/wire.rs`, the `clap` definitions in
`tools/knegiswhisper/src/main.rs`, and the `TOOLS` list in
`tools/knegiswhisper-mcp/src/tools.rs`. Nothing is described from memory of what
an endpoint like this usually does.

On 2026-09-07, the official Python MCP client connected over stdio to the
compiled server and exercised tools against an isolated running Windows app.
The CLI also completed local transcription and recording workflows. See
[`../CLI_MCP_WORKFLOWS.md`](../CLI_MCP_WORKFLOWS.md) for behavior and the precise
validation scope; these checks do not establish cloud-provider connectivity.

| file | what it covers |
| --- | --- |
| [`knegiswhisper-api/SKILL.md`](knegiswhisper-api/SKILL.md) | The HTTP API: discovery, authentication, notes, history and desktop workflows |
| [`knegiswhisper-cli/SKILL.md`](knegiswhisper-cli/SKILL.md) | The `knegiswhisper` command and the `knegiswhisper-mcp` stdio server |
