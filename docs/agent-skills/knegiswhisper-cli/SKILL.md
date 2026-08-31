---
name: knegiswhisper-cli
description: Drive a running KnegisWhisper from the shell with the knegiswhisper command, or connect to it as an MCP server with knegiswhisper-mcp.
---

# KnegisWhisper CLI and MCP server

Two ways into the same place. `knegiswhisper` is a command; `knegiswhisper-mcp`
is an MCP server over stdio. Both are thin clients over the loopback HTTP API
described in [`knegiswhisper-api`](../knegiswhisper-api/SKILL.md) — same
discovery file, same bearer token, same endpoints. Neither reads the database
directly, and neither invents an endpoint the local API does not serve.

Prefer these over raw HTTP: they handle discovery, authentication and exit codes
for you.

## Before anything else

The local API is **off by default**. The user switches it on under
**Integrations › API**. Until then both tools have nothing to connect to, which
is a configuration state and not a fault.

Where the binaries are, and the command to paste into an MCP client's
configuration, are both shown in the application under **Integrations › CLI**
and **Integrations › MCP**.

## The CLI

```
knegiswhisper health

knegiswhisper notes list   [--limit N] [--folder ID] [--type TYPE]
knegiswhisper notes get    <ID>
knegiswhisper notes search <QUERY…> [--limit N]
knegiswhisper notes set    <ID> [--title T] [--content C] [--folder ID]
knegiswhisper notes delete <ID>

knegiswhisper folders list

knegiswhisper dictionary list
knegiswhisper dictionary add    <WORD…>
knegiswhisper dictionary remove <WORD…>

knegiswhisper history list          [--limit N] [--after ID]
knegiswhisper history get           <ID>
knegiswhisper history delete        <ID>
knegiswhisper history forget-audio  <ID>
```

`--limit` defaults to 20 wherever it appears. `history list --after ID`
continues from a previous page.

**`notes set` with no flags changes nothing and is refused** rather than
reported as a success — the same rule the API applies to an empty patch, for the
same reason: it is nearly always a mistyped flag.

**`history forget-audio` is the gentle deletion.** It drops the kept recording
and leaves the transcript. `history delete` removes both.

Read the exit code rather than parsing prose out of stderr. A non-zero exit
distinguishes "the application is not running" from "the request was refused",
which matters when deciding whether to ask the user to start it.

## The MCP server

`knegiswhisper-mcp` speaks JSON-RPC over stdio and advertises itself as
`knegiswhisper-mcp`. Thirteen tools, one per endpoint:

```
health
notes_list  notes_get  notes_search  notes_patch  notes_delete
folders_list
dictionary_list  dictionary_update
transcriptions_list  transcriptions_get  transcriptions_delete
transcriptions_forget_audio
```

### Two kinds of failure, and they mean different things

- **A JSON-RPC error** means the call was malformed — an `id` that is not an
  integer, a required `query` that is missing. That is a bug in the caller or in
  the tool description, not an answer about the user's data.
- **A tool result with `isError: true`** means the local API considered the
  request and refused it: a folder id that does not exist, a patch that changes
  nothing. That *is* the answer to the question asked, and it is worth showing
  the user rather than retrying.

The server validates only the shape of an argument. Business rules belong to the
local API, which already has exactly one implementation of each, and a second
copy here would be a second thing to keep in step.

## Things worth not getting wrong

- **Deletions are real and immediate**, and the content is the user's own
  dictation. Ask before `notes delete`, `history delete`, or their tool
  equivalents, unless that is exactly what was requested.
- **Never print the bearer token** out of `~/.knegiswhisper/local-api.json`, and
  do not read that file yourself when a CLI command will do — both tools already
  handle it.
- **There is no remote mode.** Both tools reach `127.0.0.1` and nothing else.

## What has and has not been exercised

The CLI has been run against a live application. **No MCP client has ever
connected to `knegiswhisper-mcp`** — the protocol handling is unit-tested and
has never had a real client on the other end (KNEG-91). If something in the
handshake behaves unlike this file says, believe the wire and report it.
