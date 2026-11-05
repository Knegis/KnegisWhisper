/** Run the existing repository contracts before producing a release. */
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
const checks = [
  ["--test", "tools/ci/changes.test.mjs", "tools/ci/entry.test.mjs"],
  ["tools/integration-audit/verify-parity.mjs"],
  ["tools/settings-audit/scan.mjs"],
  ["tools/integration-audit/verify-binaries.mjs"],
  ["tools/command-audit/scan.mjs"],
  ["tools/domain-scanner/scan.mjs"],
  ["tools/integration-audit/generate-provider-registry.mjs", "--check"],
  ["tools/integration-audit/generate-ts-types.mjs", "--check"],
  ["tools/feature-audit/scan.mjs"],
  ["tools/capability-audit/scan.mjs"],
  ["--test", "tools/domain-scanner/scan.test.mjs", "tools/integration-audit/verify-parity.test.mjs",
    "tools/settings-audit/scan.test.mjs", "tools/model-pin/pin.test.mjs",
    "tools/model-pin/retry.test.mjs", "tools/sidecar-pin/pin.test.mjs",
    "tools/feature-audit/scan.test.mjs", "tools/capability-audit/scan.test.mjs",
    "tools/command-audit/scan.test.mjs", "tools/dev-bundle-audit/scan.test.mjs"],
];
for (const args of checks) {
  const result = spawnSync(process.execPath, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
const manifest = JSON.parse(readFileSync("config/integration-manifest.json", "utf8"));
const baseline = JSON.parse(readFileSync("config/upstream-integration-baseline.json", "utf8"));
if (manifest.baseline.commit !== baseline.provenance.commit) throw new Error("Baseline provenance mismatch");
