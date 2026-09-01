# Integration Parity

Integration parity with the OpenWhispr baseline is a **hard acceptance criterion**, not an
aspiration. No upstream integration may be silently omitted.

This document explains the mechanism. The data itself lives in two files, and those files — not
this prose — are the contract:

| File | Role |
| --- | --- |
| `config/upstream-integration-baseline.json` | Generated, frozen record of the upstream surface at `c4000db9`. Never hand-edited. |
| `config/integration-manifest.json` | Hand-maintained declaration of intent: one entry per integration, each with a status and, where a user configures it, a `reachable` claim. |
| `config/ui-surfaces.json` | Hand-maintained. Every UI surface an entry may name, the component that renders it, and whether that component exists. |

## The gate

```sh
node tools/integration-audit/verify-parity.mjs
```

Reads only committed JSON and source files, never the upstream checkout, so it runs anywhere
with no dependencies. It fails the build when:

| Rule | Fails when |
| --- | --- |
| `missing-upstream-provider` | A baseline provider, model family, streaming provider, or secret slot has no manifest entry. |
| `dropped-registry-model` | A model ID present in the baseline is absent from its manifest entry. |
| `missing-status` | An entry exists without a status. |
| `unknown-status` | A status outside `statusVocabulary`. |
| `incomplete-deferral` | A `temporarily-deferred` entry lacks `why`, `dependencies`, `interface`, `expectedPhase`, or `upstream`. |
| `undeclared-removal` | A `removed-vendor-service` entry lacks a `detail`, or a vendor service is quietly re-declared as active. |
| `frontend-without-backend` | An entry claims a working implementation but names no backend crate. |
| `backend-without-ui` | A user-configurable integration names no UI surface. |
| `incomplete-documentation` | A `kind: "documentation"` entry names no target path. |
| `unknown-ui-surface` | An entry names a `ui` value absent from `config/ui-surfaces.json`. |
| `missing-reachability` | A user-configurable entry does not declare `reachable`. Absent is a violation, not a default. |
| `unreachable-surface` | An entry claims `reachable: true` through a surface that is not built. |
| `incomplete-surface` | A declared surface lacks `surfaceId`, `component`, or `built`. |
| `phantom-surface` | A surface is marked `built` but its component does not exist, or does not render its `data-surface` marker. |
| `baseline-drift` | The manifest's pinned baseline commit no longer matches the baseline file. |

Each rule has a negative test: the gate has been verified to actually fail for every class
above, because a gate that only ever passes proves nothing. For the UI-surface rules that
verification is itself committed and runs in CI:

```sh
node --test tools/integration-audit/verify-parity.test.mjs
```

## `built`, `reachable`, and why they are two fields

`backend-without-ui` — rule 5, "a backend integration exists but cannot be configured in the
UI" — only ever checked that the `ui` field was a **non-empty string**. Sixty-four entries named
screens like `Settings > Meetings` and `Notes > Upload` that nobody had written, and the gate
reported green throughout. A gate that cannot tell a screen from a string is the same defect it
exists to catch, one level up.

Two fields now answer two different questions.

**`built`** is a property of a surface, and it is mechanical. `config/ui-surfaces.json` names the
component, and the gate requires that file to exist and to render
`data-surface="<surfaceId>"` on its root element. A source-text marker rather than a build
artefact, so the check stays in the cheap contracts job that gates the rest of CI.

**`reachable`** is a property of an entry, and it is a claim: a user can configure *this*
integration through that surface. It may only be true where the surface is built.

`Settings > Transcription` is the case that forced the split. The section existed, listed every
provider, said whether a key was stored, and ran a real connection test — while **nothing anywhere
in the application could store an API key**. `setProviderCredential` was exported from
`apps/desktop/src/lib/ipc.ts` and never called. So the surface was built and not one provider was
reachable through it, and collapsing the two fields would have had to lie about one or the other.

`Settings > Providers` closed that, and the same screen immediately produced the next instance of
the split: a key can now be stored for all seventeen slots (grew from fifteen once
`credential:assemblyai` and `credential:deepgram` were added for meeting streaming), and one of the
seven transcription providers is still unreachable — Tinfoil, declined rather than unfinished; the
other six all resolve to a real client now. Built, configurable, and — for the six — honestly
usable; Tinfoil is built, configurable-through-the-screen-as-a-refusal, and deliberately not
usable. Three separate facts, still not collapsed into one.

## The backlog

On success the gate prints what is declared unreachable, grouped by surface — quoted here as it
actually printed, not restated by hand:

```
8 integrations are declared unreachable:
  Settings > Transcription > Local      4  (surface built)
  Settings > Transcription              1  (surface built)
  Settings > Meetings                   1  (surface built)
  Integrations > Calendar               1  (surface built)
  Notes > Upload                        1  (surface built)
```

Down from 19 unreachable and 5 unbuilt surfaces when this section was last written, to 8 unreachable
and **zero** unbuilt surfaces — no "surface not built" entry remains anywhere in the manifest. It
read 7 at the close of the plan and went back up by one when URL import was withdrawn, which makes
it the only entry on this list that regressed rather than never having been built. Every one of the
eight is unreachable for a specific, named reason rather than "not done yet":

- **4 — the sherpa/Parakeet local-ASR models.** The runtime is real, measured, and proven: a probe
  installed the pinned CLI and both pinned model archives and transcribed the same fixture whisper's
  `tiny` model does, correctly. What is missing is a `Settings > Transcription > Local` selector,
  deliberately not built as part of the measurement task that proved the runtime — see
  `docs/BUILD_PLAN.md` chunk B4.
- **1 — `transcription:tinfoil`.** Declined, not unfinished: an unattested HTTPS request would
  transcribe correctly while presenting as Tinfoil, a confidentiality claim it did not earn.
- **1 — `meeting-stream:tinfoil-realtime`.** The identical refusal, for the streaming client.
- **1 — `calendar:apple`.** EventKit is described in `kw_calendar_core::apple` and implemented
  nowhere, on any platform, for lack of a macOS machine to write or verify a binding against.
- **1 — `media:url-import`.** Withdrawn, not unfinished. It was reachable, and the command was
  unregistered because the guard this application can run — a literal-host check on the URL as
  typed — cannot see where a hostname resolves at connect time, nor any redirect or
  extractor-generated request `yt-dlp` makes inside its own process. The backend is dormant rather
  than deleted; see `docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import".

**59 integrations are reachable today**, up from 8 when this plan was written and 45 when this
section was last written mid-run: the seventeen credential slots, six of seven transcription
providers, seven reasoning providers, three enterprise connectors, six local LLM families, the six
whisper models, five of six meeting-stream entries, two of three calendar providers, the local REST
API, the CLI and MCP screens, transcript export, and audio/video/OpenWhispr import.

*Reachable* is still not *verified*. The three enterprise connectors, the six meeting/calendar/media
wire shapes and the newly-added meeting-stream clients can all be configured, and every one of them
still needs a live call nobody has made — no key, no client ID, or no organisational credential
exists on any machine this project has touched. `docs/PLATFORM_SUPPORT.md` and
`docs/BENCH_CHECK.md` keep that distinction per provider and per capability; **none of chunks E
onward has been exercised by a person at all**, only by tests and code review.

The jump from 45 came from five chunks and no shared subsystem: three new meeting clients plus the
one that already existed, a calendar crate, two new sidecars for media import, an MCP server, and
two settings screens for developer tooling — each closing a specific "built, tested backend with no
way in" the same way chunks A–C already had for reasoning and transcription.

## Status vocabulary

| Status | Meaning |
| --- | --- |
| `ported` | Upstream code reused essentially as-is. |
| `ported-with-new-rust-implementation` | Same capability, reimplemented in Rust. |
| `rewired-to-direct-byok` | Same provider, but credentials and endpoints come straight from the user — never through a vendor service. |
| `replaced-with-local-equivalent` | Vendor capability swapped for a local one. |
| `platform-specific` | Available only on some platforms; see `PLATFORM_SUPPORT.md`. |
| `temporarily-deferred` | Not yet implemented. Requires full deferral detail. |
| `removed-vendor-service` | Deliberately gone. Requires a `detail` naming what was removed. |
| `not-applicable` | Has no meaning in this architecture. |

## Current state

Quoted from the gate's own output, not recomputed by hand — **102 entries** against baseline
`c4000db9802d`:

| Status | Count |
| --- | --- |
| `ported-with-new-rust-implementation` | 52 |
| `rewired-to-direct-byok` | 21 |
| `removed-vendor-service` | 18 |
| `temporarily-deferred` | 8 |
| `ported` | 2 |
| `platform-specific` | 1 |

This was 99 entries and different counts when this section was last written. Three entries were
added in between — `credential:assemblyai` and `credential:deepgram` (new BYOK slots for meeting
streaming), and `media:openwhispr-import` (an importer that existed and was tested from early in
the build but was deliberately kept out of the manifest until it had a screen, per the
"backend-without-ui" rule this document describes above; both landed together in chunk G). Four
status corrections made during Task 19's audit of every `ACTIVE`-vocabulary entry account for the
rest of the movement in the table below.

### The `ACTIVE`-status audit

`verify-parity.mjs` defines `ACTIVE` — `ported`, `ported-with-new-rust-implementation`,
`rewired-to-direct-byok`, `replaced-with-local-equivalent`, `platform-specific` — as "statuses that
assert a working implementation exists." Mid-run, `platform:aec` was corrected from
`ported-with-new-rust-implementation` to `temporarily-deferred` because nothing implementing it
exists at all. Task 19 audited every other `ACTIVE` entry against the same test and found three more
instances of the identical overclaim, plus one adjacent case outside the `ACTIVE` set entirely:

| Entry | Carried | Reality |
| --- | --- | --- |
| `meeting-stream:tinfoil-realtime` | `ported` | No Tinfoil meeting client exists anywhere in the codebase |
| `transcription:tinfoil` | `rewired-to-direct-byok` | `clients::shape()` returns `ClientNotImplemented`; nothing else implements it |
| `calendar:apple` | `platform-specific` | `AppleCalendarSource` is "deliberately unimplemented by anything in this crate," on every platform |
| `developer:agent-skills` (`kind: "documentation"`, outside `ACTIVE`) | `ported`, target `docs/agent-skills/` | That directory does not exist; the rewrite was never done |

All four now carry `temporarily-deferred`. For the two Tinfoil entries this is a disclosed
compromise, not a fit: this manifest's `statusVocabulary` has no "declined on principle" term, and
each entry's `deferral.why` says so rather than letting the status imply a resource-blocked,
resumable wait. `calendar:apple` and `developer:agent-skills` fit the term's ordinary meaning —
a macOS machine and a person to write documentation, respectively, are named dependencies rather
than open questions.

## Deferred integrations

Eight, each with dependencies and a named interface in the manifest — up from three when this
section was last written, once the four corrections above joined the four that were already
deferred for a genuine resource or scope reason.

| Integration | Phase | Why | Degradation |
| --- | --- | --- | --- |
| `search:semantic` | 8 | Qdrant + MiniLM + ONNX Runtime is a large dependency for a feature the app must work without anyway. | FTS5 keyword search stays fully functional; the UI hides semantic ranking rather than failing. |
| `diarization-model:sherpa-onnx-pyannote-segmentation-3-0` | 8 | Needs ONNX Runtime and three models; dictation and meetings must be reliable first. | Transcripts have no speaker labels. |
| `diarization-model:3dspeaker-campplus-en` | 8 | As above. | As above. |
| `platform:aec` | 4 | No Rust equivalent to WebRTC's audio processing module exists that is not itself a C++ binding, and there is no C++ toolchain on this development machine. | Meetings still record and transcribe correctly; on speakers the far-end voice is transcribed twice instead of once, and on headphones there is nothing to cancel. |
| `meeting-stream:tinfoil-realtime` | 4 | Declined on principle, not resource-blocked — see "the `ACTIVE`-status audit" above. | Every other meeting-stream provider is unaffected; this one is refused rather than approximated. |
| `transcription:tinfoil` | 3 | Same principled refusal as above, for the batch client. | The other six transcription providers are unaffected. |
| `calendar:apple` | 5 | EventKit has no Rust binding anywhere in this project and there is no macOS machine to write or verify one against. | Google and Microsoft calendar sync are unaffected; Apple reports `Unsupported` with the reason rather than an empty calendar list. |
| `developer:agent-skills` | 7 | The rewritten `docs/agent-skills/` was never written; nothing blocks it beyond someone doing it. | None — documentation only, with no backend or UI surface of its own. |

## Things worth knowing about the data

**Four providers the brief omits are in the manifest** because the repository audit is
authoritative: Berget AI, xAI, Mistral, and Liquid AI.

**Berget is normalized, not special-cased.** Upstream privileges it inconsistently —
`sizeCapBytes: null` (uncapped, while OpenAI and Groq cap at 25 MB), `allowCustomModelId: true`
bypassing model-prefix validation, and `streaming: false` on all three models. The port replaces
the magic `null` with an explicit `maxUploadBytes` that *every* provider declares, keeps
`allowCustomModelId` as a legitimate capability, and keeps `streaming: false` until a Berget
streaming model exists.

**`openwhisprCloudModels` is excluded from the baseline**, not silently dropped: it has zero
readers upstream and is recorded under `registry.droppedFromBaseline` with the reason
`removed-vendor-service`.

**The seventh meeting provider is measured, not ported.** `meeting-stream:local-streaming`
(2026-09-01) has no upstream counterpart that could be read: `parakeetWsServer.js` is not in this
checkout and sherpa-onnx's WebSocket protocol is undocumented, so the wire format came from a probe
against the real server. It is the only client here whose format was established by measurement,
and the only one where a documented fact turned out to be wrong — the reference client's `--help`
says 16-bit samples and the wire carries `f32`. See `crates/meeting-core/src/nemotron.rs`.

**Meeting-provider capabilities are honest about asymmetry.** There is no formal capability
object upstream, so `MeetingProviderCapabilities` is new work. It must encode the real
differences rather than a uniform shape: keyterms are supported by three of five providers with
three different wire formats (AssemblyAI `keyterms_prompt`, **capped at 100 terms**; Deepgram
repeated params; Corti `configuration.keyterms`) and are **silently ignored by OpenAI Realtime
and Tinfoil**. Only Deepgram rotates tokens proactively. OpenAI Realtime has a 60-minute session
cap. Reporting a capability a provider does not have would be exactly the faked integration
success the brief forbids.

## Bumping the baseline

1. `node tools/integration-audit/generate-baseline.mjs --upstream <path>`
2. Review the diff. If a count assertion fired, upstream changed shape — understand it before
   editing the `EXPECT` table.
3. Update `config/integration-manifest.json` for any new or changed integration, including a
   `reachable` claim for anything a user configures.
4. Add any new UI surface to `config/ui-surfaces.json`.
5. `node tools/integration-audit/verify-parity.mjs` must pass.

The baseline generator emits deterministic output, so a no-op regeneration produces an empty
`git diff`.
