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
**Settings > Developer > Local API**. Until then both tools have nothing to connect to, which
is a configuration state and not a fault.

Where the binaries are, and the command to paste into an MCP client's
configuration, are both shown in the application under **Settings > Developer > CLI**
and **Settings > Developer > MCP**.

## The CLI

```
knegiswhisper health
knegiswhisper status
knegiswhisper models list
knegiswhisper notes create --title TITLE --content MARKDOWN [--folder ID]
knegiswhisper notes export ID [--format markdown] [--transcript] [--output PATH] [--force]
knegiswhisper transcribe file PATH
knegiswhisper jobs status ID
knegiswhisper jobs cancel ID
knegiswhisper recording start --provider ID [--system-audio]
knegiswhisper recording stop
knegiswhisper history search QUERY [--limit N] [--after ID]

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

**`history list` has a `status` column, and a failed row explains itself** on a
marked continuation line beneath it:

```
id  when                 status     provider  text
19  2026-08-26 09:51:11  failed     -         -
      ↳ provider returned 429
```

So `completed`, `failed` and `cancelled` are three answers rather than one, and
a row with no text is never ambiguous between "it broke", "I cancelled it" and
"the microphone heard nothing". Parse `--json` rather than this table if you are
branching on it — the human form is free to change and nothing machine-readable
is built on it.

**`history get` prints the transcript and nothing else**, so it can be
redirected to a file. A row with no transcript prints its status and reason
instead of the blank line it used to.

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
`knegiswhisper-mcp`. Tools map to the local API:

```
health
app_status  models_list
notes_create  notes_export
transcription_create  jobs_get  jobs_cancel
recording_start  recording_stop
transcriptions_search
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

- **`notes delete` moves the note to Trash; `history delete` is real and
  immediate.** A note in Trash keeps its text and recording until the user
  restores it or empties Trash in the app -- nothing empties it on its own --
  and from then on both tools treat it as gone: `notes get` exits 3, and list
  and search leave it out. Neither tool can restore a note or delete one for
  good. The content is the user's own either way: ask before `notes delete`,
  `history delete`, or their tool equivalents, unless that is exactly what was
  requested.
- **The two permanent MCP tools need `confirm: true`.** `transcriptions_delete`
  and `transcriptions_forget_audio` answer a JSON-RPC error without it and change
  nothing. Send it only after the user has said yes: the field records that step,
  it does not stand in for it.
- **Never print the bearer token** out of `~/.knegiswhisper/local-api.json`, and
  do not read that file yourself when a CLI command will do — both tools already
  handle it.
- **There is no remote mode.** Both tools reach `127.0.0.1` and nothing else.

## What has and has not been exercised

On 2026-09-07, the compiled CLI and the official Python MCP SDK exercised an
isolated running Windows app with local Whisper tiny. MCP negotiated protocol
`2024-11-05`, listed 23 tools, created/exported notes, searched history, read
status/models and transcribed a fixture into a saved note. CLI checks also
covered cancellation, interrupted-job recovery and microphone meeting start/stop.
See [workflow behavior and validation](../../CLI_MCP_WORKFLOWS.md). Cloud
credentials, system-audio capture and third-party editor configuration were
outside this run.
