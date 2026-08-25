# Integration Parity

Integration parity with the OpenWhispr baseline is a **hard acceptance criterion**, not an
aspiration. No upstream integration may be silently omitted.

This document explains the mechanism. The data itself lives in two files, and those files — not
this prose — are the contract:

| File | Role |
| --- | --- |
| `config/upstream-integration-baseline.json` | Generated, frozen record of the upstream surface at `c4000db9`. Never hand-edited. |
| `config/integration-manifest.json` | Hand-maintained declaration of intent: one entry per integration, each with a status. |

## The gate

```sh
node tools/integration-audit/verify-parity.mjs
```

Reads only the two committed JSON files, never the upstream checkout, so it runs anywhere with
no dependencies. It fails the build when:

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
| `baseline-drift` | The manifest's pinned baseline commit no longer matches the baseline file. |

Each rule has a negative test: the gate has been verified to actually fail for every class
above, because a gate that only ever passes proves nothing.

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
3. Update `config/integration-manifest.json` for any new or changed integration.
4. `node tools/integration-audit/verify-parity.mjs` must pass.

The baseline generator emits deterministic output, so a no-op regeneration produces an empty
`git diff`.
