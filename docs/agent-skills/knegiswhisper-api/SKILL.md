---
name: knegiswhisper-api
description: Read and edit notes, dictation history and the recognition dictionary in a running KnegisWhisper on this machine, over its loopback HTTP API.
---

# KnegisWhisper local API

KnegisWhisper is a desktop dictation and meeting-notes application. When the
user switches its local API on, it serves a small HTTP interface **on loopback
only**, for tools running as the same user on the same machine.

There is no hosted service, no account, and no remote endpoint. If you cannot
reach `127.0.0.1`, there is nowhere else to try.

## Before anything else: is it on?

The local API is **off by default**. The user turns it on under
**Settings > Developer > Local API** in the application. Until they do, there is no discovery
file and no listener, and that is a configuration state rather than a fault —
say so instead of retrying.

## Discovery

Read `~/.knegiswhisper/local-api.json`:

```json
{ "version": 1, "port": 47821, "token": "…" }
```

- **`version`** is `1`. A file naming any other version was written by a build
  that does not match this skill; stop rather than guessing at the differences.
- **`port`** is allocated at start and changes between runs. Never cache it
  across launches.
- **`token`** is a bearer token minted per run.

The base URL is `http://127.0.0.1:{port}`. The file is deleted when the
application stops, so its absence means "not running", not "not installed".

**Treat the token as a credential.** Do not print it, log it, copy it into a
transcript, or write it anywhere the user did not ask for. The file's access
list is deliberately narrowed to the user, `SYSTEM` and administrators, and
re-read on every status request; leaking the contents defeats that.

## Authentication

Every request, including `/v1/health`:

```
Authorization: Bearer <token>
```

A missing or wrong token both answer `401` with the same body. That is
deliberate — the response does not distinguish them — so do not infer anything
from which you got.

## Endpoints

All under `/v1`. Successful responses wrap the payload: a single object comes
back as `{"data": …}`, a collection as a list under the same key.

| method | path | what it does |
| --- | --- | --- |
| `POST` | `/v1/notes` | Create an editable note: title, content, optional folder_id |
| `GET` | `/v1/notes/{id}/export` | Editable Markdown; transcript=true selects timed export |
| `GET` | `/v1/transcriptions/search?q=…` | Final dictation text; limit and after page newest first |
| `POST` | `/v1/jobs/transcriptions` | One multipart file field; returns HTTP 202 with job metadata |
| `GET` | `/v1/jobs/{id}` | Durable stage, route, timestamps, result note_id, error |
| `POST` | `/v1/jobs/{id}/cancel` | Request cancellation before saving |
| `POST` | `/v1/recording/start` | Meeting recording: required provider, optional system_audio |
| `POST` | `/v1/recording/stop` | Finish the application's active meeting |
| `GET` | `/v1/status` | Recording, route, and latest import metadata |
| `GET` | `/v1/models` | Models and configuration readiness, not connectivity proof |
| `GET` | `/v1/health` | Confirm the application is up and answering |
| `GET` | `/v1/notes/list` | Notes, most recently edited first |
| `GET` | `/v1/notes/search` | Full-text search over notes |
| `GET` | `/v1/notes/{id}` | One note in full |
| `PATCH` | `/v1/notes/{id}` | Change a note's title, content or folder |
| `DELETE` | `/v1/notes/{id}` | Move a note to Trash |
| `GET` | `/v1/folders/list` | Note folders |
| `GET` | `/v1/dictionary/list` | The custom dictionary that biases recognition |
| `POST` | `/v1/dictionary/update` | Add or remove dictionary words |
| `GET` | `/v1/transcriptions/list` | Dictation history, newest first |
| `GET` | `/v1/transcriptions/{id}` | One dictation |
| `DELETE` | `/v1/transcriptions/{id}` | Delete a dictation and its transcript |
| `DELETE` | `/v1/transcriptions/{id}/audio` | Delete only the kept recording, leaving the transcript |

While a meeting is recording, `/v1/status`'s `meeting.plan` says what it will be
saved as: `title` and `folderId` (null for none) and `deleteRecording`, whether
its recording is deleted once its transcript is saved. It is `null` otherwise.

### Bodies

`PATCH /v1/notes/{id}` takes any subset of three fields, in camelCase:

```json
{ "title": "…", "content": "…", "folderId": 3 }
```

**A body that would change nothing is refused**, not answered `200`. An empty
patch is almost always a mistyped field name or a body that never got sent, and
succeeding at it would hide that.

`POST /v1/dictionary/update` takes two lists, either of which may be omitted:

```json
{ "add": ["Knegis", "WASAPI"], "remove": ["typo"] }
```

### Query parameters

`notes/list` accepts a `limit` (default 20), a folder id, and a note type.
`notes/search` takes the query and a `limit`. Each hit has `id`, `title`, a
`snippet` with the match in `[` `]`, and `transcriptRow`: when every word is in
one row of a meeting transcript, `{ index, startSeconds }` names that row and
when it was said (seconds from the start, or `null` for an untimed row), and the
snippet is that row's; otherwise `null`. `transcriptions/list` takes a `limit`
and an `after` id to continue a previous page.

## Errors

```json
{ "error": { "code": "not_found", "message": "no such route" } }
```

`code` is one of `unauthorized`, `forbidden`, `not_found`, `validation_error`,
`payload_too_large`, `conflict`, `unavailable`, `internal_error`. Branch on the code; the message is for a
person and may change.

An `internal_error` message is deliberately vague. The detail went to the
application's log, because a storage failure can carry a file path or a column
name, and the API will not hand those to a caller.

## Things worth not getting wrong

See [workflow behavior](../../CLI_MCP_WORKFLOWS.md) for upload limits, job stages,
cancellation, editable versus timed exports, and recording side effects. Uploads
contain file bytes, never a path for the server to open. Recording controls the
existing meeting recorder; `models` reports configuration without testing cloud
connectivity.

- **A note delete goes to Trash; a dictation delete is real and
  immediate.** `DELETE /v1/notes/{id}` moves the note to Trash in the app, as
  Delete does there: its text and recording are kept until the user restores it
  or empties Trash, and nothing empties Trash on its own. From then on the API
  treats the note as gone -- `GET`, `PATCH` and export answer `404`, list and
  search leave it out, and a second `DELETE` is `404` too -- and there is no
  endpoint to restore it or delete it for good: both are the user's, in the
  app. `DELETE /v1/transcriptions/{id}` removes the user's own dictated text at
  once. Ask before calling either unless the user asked for exactly that. The
  `…/audio` variant is the gentler one: it drops the recording and keeps the
  text.
- **The port is not stable.** Re-read the discovery file rather than
  remembering a port from earlier in a session.
- **Loopback only, by design.** The server refuses anything that is not
  loopback before the router sees it. There is no configuration that changes
  this, and asking the user to expose it is not a workaround to offer.
