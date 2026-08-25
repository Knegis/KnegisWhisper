# integration-audit

Generates `config/upstream-integration-baseline.json` — the frozen record of the OpenWhispr
integration surface that KnegisWhisper claims parity with.

## Why this exists

The brief's hard acceptance criterion is that **no upstream integration may be silently
omitted**. That is only enforceable if the upstream surface is captured as machine-readable
data, once, from a recorded commit. This tool does that capture; CI then diffs
`config/integration-manifest.json` against the result and never needs the upstream checkout.

## Usage

```sh
# Regenerate the baseline (run once per deliberate baseline bump)
node generate-baseline.mjs --upstream "/path/to/openwhispr"

# Verify the committed baseline still matches the upstream checkout
node generate-baseline.mjs --check
```

Output is deterministic — no timestamps, sorted keys — so a regeneration that changes nothing
produces a byte-identical file and an empty `git diff`.

## Design notes

**Upstream files are read as text and parsed with targeted regexes, never imported.** Several
of the modules we extract from (`meetingStreamingProviders.js` and everything it requires) pull
in Electron, so importing them would mean running upstream code in our build. Regex extraction
keeps this tool dependency-free and side-effect-free.

**Every extractor asserts an expected count** (the `EXPECT` table, sourced from the Phase 0
audit of `c4000db9`). A regex that silently stops matching after an upstream refactor would
otherwise emit a quietly-truncated baseline — which is exactly the failure mode the parity gate
exists to prevent. Drift fails loudly instead, and updating `EXPECT` is a deliberate act.

## Baseline provenance

The baseline is generated from the local fork `c4000db9` (`feat/byok-only-dyve`), **not** the
brief's `858e824`, which is absent from this clone. The ~3-commit delta between them touches no
provider or registry code. `c4000db9^` is `4d1a6eaf` — pristine upstream `main` — and is
recorded in the baseline as `provenance.upstreamForkPoint` so an upstream cross-check is always
one `git diff` away.

`openwhisprCloudModels` is deliberately excluded from the baseline. It has zero readers
upstream and is a hosted-tiering artifact; it is recorded under
`registry.droppedFromBaseline` with the reason `removed-vendor-service` rather than being
dropped without a trace.
