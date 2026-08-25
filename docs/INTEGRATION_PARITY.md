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
the split: a key can now be stored for all fifteen slots, and four of the seven transcription
providers are still unreachable because their route resolves to `Transport::ProviderClient` and
the executor refuses it. Built, configurable, and honestly unusable — three separate facts.

## The backlog

On success the gate prints what is declared unreachable, grouped by surface. That list is the
frontend backlog, sized in integrations rather than in adjectives:

| Surface | Entries | Surface built |
| --- | --- | --- |
| `Settings > Intelligence` | 7 | no |
| `Settings > Intelligence > Local` | 6 | no |
| `Settings > Meetings` | 6 | no |
| `Settings > Transcription` | 4 | yes — these four need a provider client that does not exist |
| `Settings > Transcription > Local` | 4 | yes — whisper only; Parakeet and Nemotron have no sidecar |
| `Settings > Intelligence > Enterprise` | 3 | no |
| `Integrations > Calendar` | 3 | no |
| `Notes > Upload` | 3 | no |
| `Integrations > CLI` | 1 | no |
| `Integrations > MCP` | 1 | no |

**26 integrations are reachable today**, up from 8: the fifteen credential slots, three
transcription providers, the six whisper models, the local REST API, and transcript export.

The jump came from one screen and no new subsystem. Every one of those fifteen slots had working
Rust behind it and nowhere to type a key.

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

99 entries against baseline `c4000db9`:

| Status | Count |
| --- | --- |
| `ported-with-new-rust-implementation` | 52 |
| `rewired-to-direct-byok` | 20 |
| `removed-vendor-service` | 18 |
| `ported` | 4 |
| `temporarily-deferred` | 3 |
| `platform-specific` | 2 |

## Deferred integrations

Three, each with dependencies and a named interface in the manifest.

| Integration | Phase | Why | Degradation |
| --- | --- | --- | --- |
| `search:semantic` | 8 | Qdrant + MiniLM + ONNX Runtime is a large dependency for a feature the app must work without anyway. | FTS5 keyword search stays fully functional; the UI hides semantic ranking rather than failing. |
| `diarization-model:sherpa-onnx-pyannote-segmentation-3-0` | 8 | Needs ONNX Runtime and three models; dictation and meetings must be reliable first. | Transcripts have no speaker labels. |
| `diarization-model:3dspeaker-campplus-en` | 8 | As above. | As above. |

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
