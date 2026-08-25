/**
 * Tests for the forbidden-domain scanner.
 *
 * The important ones are the negative tests: a scanner that only ever passes
 * proves nothing. Each builds a throwaway tree, plants a violation, and asserts
 * it is caught — in particular the CI-workflow case, which is the leak found in
 * the upstream audit and the reason workflows are in scope at all.
 *
 * Run: node --test tools/domain-scanner/
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";

import { scan, globToRegExp } from "./scan.mjs";

/** Minimal config mirroring the real one's shape. */
const CONFIG = {
  forbidden: [
    { pattern: "auth\\.openwhispr\\.com", reason: "hosted auth" },
    { pattern: "\\bpluely\\.com\\b", reason: "vendor backend" },
    { pattern: "OpenWhispr/whisper\\.cpp", reason: "vendor-controlled fork" },
    { pattern: "\\bisPaid\\b", reason: "paid gating" },
  ],
  scanPaths: [
    "crates/**/*.rs",
    "apps/**/*.ts",
    ".github/workflows/*.yml",
    "sidecars/**/*.mjs",
  ],
  builtAssetPaths: ["apps/desktop/dist/**/*.js"],
  allowlist: [
    { path: "docs/**", reason: "documentation" },
    { path: "**/test/fixtures/**", reason: "fixtures" },
  ],
};

function makeTree(files) {
  const root = mkdtempSync(join(tmpdir(), "kw-scan-"));
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(root, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  return root;
}

function withTree(files, fn) {
  const root = makeTree(files);
  try {
    return fn(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("a clean tree passes", () => {
  withTree(
    {
      "crates/foo/src/lib.rs": "pub fn ok() {}\n",
      "apps/desktop/src/App.ts": "export const x = 1;\n",
    },
    (root) => {
      const { violations, scanned } = scan({ root, config: CONFIG });
      assert.deepEqual(violations, []);
      assert.equal(scanned, 2);
    }
  );
});

test("catches a vendor host in Rust source", () => {
  withTree(
    { "crates/foo/src/lib.rs": 'const URL: &str = "https://auth.openwhispr.com";\n' },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1);
      assert.equal(violations[0].file, "crates/foo/src/lib.rs");
      assert.equal(violations[0].line, 1);
    }
  );
});

test("catches the upstream VITE_AUTH_URL leak in a CI workflow", () => {
  // This is the exact shape found in OpenWhispr's build-and-notarize.yml. A
  // src/-only scanner sees nothing, yet the value reaches the shipped binary.
  withTree(
    {
      ".github/workflows/release.yml":
        "jobs:\n  build:\n    env:\n" +
        "      VITE_AUTH_URL: ${{ vars.VITE_AUTH_URL || 'https://auth.openwhispr.com' }}\n",
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1, "workflow leak must be caught");
      assert.match(violations[0].file, /\.github\/workflows\//);
      assert.equal(violations[0].pattern, "auth\\.openwhispr\\.com");
    }
  );
});

test("catches a vendor host that survives only in a built asset", () => {
  withTree(
    {
      "apps/desktop/dist/assets/index-abc123.js":
        'var e="https://api.pluely.com/v1";export{e};\n',
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1, "built assets must be scanned");
    }
  );
});

test("catches a build-time dependency on the vendor's GitHub org", () => {
  withTree(
    {
      "sidecars/whisper/download.mjs":
        'const REPO = "OpenWhispr/whisper.cpp";\nconst TAG = "0.0.8";\n',
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1);
      assert.equal(violations[0].pattern, "OpenWhispr/whisper\\.cpp");
    }
  );
});

test("catches paid-plan gating", () => {
  withTree(
    { "apps/desktop/src/Settings.ts": "if (isPaid) { showPro(); }\n" },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1);
    }
  );
});

test("reports every occurrence, with line numbers", () => {
  withTree(
    {
      "crates/foo/src/lib.rs":
        "// line 1\n" +
        'let a = "auth.openwhispr.com";\n' +
        "// line 3\n" +
        'let b = "pluely.com";\n',
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 2);
      assert.deepEqual(
        violations.map((v) => v.line),
        [2, 4]
      );
    }
  );
});

test("allowlisted documentation may name what was removed", () => {
  withTree(
    {
      "docs/UPSTREAM_AUDIT.md": "We removed auth.openwhispr.com and pluely.com.\n",
      // Prove the allowlist is path-scoped, not global.
      "crates/foo/src/lib.rs": "// clean\n",
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.deepEqual(violations, []);
    }
  );
});

test("allowlisted fixtures may embed forbidden strings", () => {
  withTree(
    { "crates/foo/test/fixtures/leak.rs": '"auth.openwhispr.com"\n' },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.deepEqual(violations, []);
    }
  );
});

test("files outside scanPaths are ignored", () => {
  withTree(
    { "scratch/notes.txt": "auth.openwhispr.com\n" },
    (root) => {
      const { violations, scanned } = scan({ root, config: CONFIG });
      assert.deepEqual(violations, []);
      assert.equal(scanned, 0);
    }
  );
});

test("node_modules and target are never walked", () => {
  withTree(
    {
      "node_modules/pkg/apps/x.ts": '"auth.openwhispr.com"\n',
      "target/debug/build/apps/x.ts": '"pluely.com"\n',
    },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.deepEqual(violations, []);
    }
  );
});

test("matching is case-insensitive", () => {
  withTree(
    { "crates/foo/src/lib.rs": '"AUTH.OpenWhispr.COM"\n' },
    (root) => {
      const { violations } = scan({ root, config: CONFIG });
      assert.equal(violations.length, 1);
    }
  );
});

test("globToRegExp handles the patterns the config actually uses", () => {
  assert.ok(globToRegExp("crates/**/*.rs").test("crates/a/src/lib.rs"));
  assert.ok(globToRegExp("crates/**/*.rs").test("crates/a/b/c/d.rs"));
  assert.ok(!globToRegExp("crates/**/*.rs").test("apps/a/src/lib.rs"));

  // `**/` must also match zero directories.
  assert.ok(globToRegExp("**/tauri.conf.json").test("tauri.conf.json"));
  assert.ok(globToRegExp("**/tauri.conf.json").test("apps/desktop/src-tauri/tauri.conf.json"));

  assert.ok(globToRegExp("*.{yml,yaml}").test("ci.yaml"));
  assert.ok(!globToRegExp("*.{yml,yaml}").test("ci.json"));

  // `*` must not cross a segment boundary.
  assert.ok(!globToRegExp(".github/workflows/*.yml").test(".github/workflows/a/b.yml"));

  // A dot in the glob is literal, not "any character".
  assert.ok(!globToRegExp("*.rs").test("axrs"));
});

test("the shipped patterns match their vendors and not ordinary English", () => {
  // Every pattern here has been, or could be, written loosely enough to match
  // a word rather than a vendor. `amplitude` is the live example: it is
  // ordinary audio terminology (peak amplitude, signal amplitude), the bare
  // `\bamplitude\b` pattern matched crates/audio-core's own DSP code, and the
  // Phase 0 audit found the same false positive upstream where every apparent
  // hit was `peakAmplitude`.
  //
  // Tightening a pattern is the right fix for that, but it is also exactly how
  // a pattern quietly stops matching the thing it exists to block -- the first
  // attempt here double-escaped the dot and stopped matching
  // api.amplitude.com. So the shipped patterns are asserted against real
  // vendor strings, not only against the clean tree.
  const shipped = JSON.parse(
    readFileSync(new URL("../../config/forbidden-domains.json", import.meta.url), "utf8")
  );
  const patterns = shipped.forbidden.map((f) => new RegExp(f.pattern, "i"));
  const matchesSomething = (text) => patterns.some((p) => p.test(text));

  // Real vendor strings that must still be caught.
  for (const vendor of [
    "https://api.amplitude.com/2/httpapi",
    "@amplitude/analytics-browser",
    "https://app.posthog.com/capture",
    "tauri-plugin-posthog",
    "https://o0.ingest.sentry.io/api",
    "https://api.mixpanel.com/track",
    "https://http-intake.logs.datadoghq.com",
    "https://auth.openwhispr.com",
    "https://api.openwhispr.com/api/v1",
    "https://pluely.com/api/update",
    "OpenWhispr/whisper.cpp",
  ]) {
    assert.ok(matchesSomething(vendor), `no pattern catches ${JSON.stringify(vendor)}`);
  }

  // Ordinary technical English that must not be caught.
  for (const innocent of [
    "let peak_amplitude = 0.0;",
    "fn tone(amplitude: f32, len: usize)",
    "the signal amplitude rises",
    "audio segmentation and segment boundaries",
    "this is a mixed panel layout",
    "a sentry pattern guards the loop",
  ]) {
    assert.ok(
      !matchesSomething(innocent),
      `a pattern falsely matches ${JSON.stringify(innocent)}`
    );
  }
});

test("the repository itself is clean", () => {
  // Runs against the real tree and the real config: this is the check CI cares
  // about, and it must pass without any special-casing.
  const { violations } = scan({});
  assert.deepEqual(
    violations.map((v) => `${v.file}:${v.line} ${v.pattern}`),
    []
  );
});
