# Local API, CLI and MCP setup

## Local API, CLI and MCP

Keep the desktop app running and enable **Settings > Developer > Local API**. Both clients use its
authenticated loopback API; the desktop owns transcription, storage and recording.
The default discovery file is `~/.knegiswhisper/local-api.json` (under your Windows user
profile on Windows). It contains the port and bearer token and should not be shared.

Build the clients from the repository root:

```sh
cargo build -p kw-cli -p kw-mcp
cargo run -p kw-cli -- health
cargo run -p kw-cli -- status
cargo run -p kw-cli -- models list
cargo run -p kw-cli -- transcribe file "meeting.wav"
cargo run -p kw-cli -- jobs status JOB_ID
cargo run -p kw-cli -- notes search "budget review"
cargo run -p kw-cli -- notes export NOTE_ID --output "notes.md"
```

Add `--json` for machine-readable CLI output. File transcription returns a job ID to poll
or cancel; it uses the app's configured transcription route. Clients upload file bytes,
so the API does not accept arbitrary server filesystem paths. Recording commands control
the visible meeting recorder, including sessions started in the UI.

Configure your MCP client's stdio command with the absolute path to
`target/debug/knegiswhisper-mcp.exe` on Windows, or `target/debug/knegiswhisper-mcp` on
macOS/Linux. No arguments are needed. Protocol messages use stdout and diagnostics use
stderr. **Settings > Developer > CLI** and **MCP** provide discovery and setup information.

See [CLI and MCP workflows](CLI_MCP_WORKFLOWS.md) for export formats, upload limits,
cancellation, restart behavior and dated Windows validation, and [agent skills](agent-skills/)
for the HTTP routes and client/tool reference.
