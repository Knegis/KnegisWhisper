/**
 * Typed wrappers over the Tauri command boundary.
 *
 * Every call into Rust goes through this file. React components never call
 * `invoke` directly, so the set of commands the frontend can reach is greppable
 * in one place, and the payload types live next to the call rather than being
 * re-declared at each use site.
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * How we know a capture-exclusion claim is true. Mirrors `overlay::Evidence`.
 */
export type Evidence = {
  kind: "read-back";
  api: string;
  value: string;
};

/**
 * Mirrors `overlay::CaptureExclusion`. Four states, not a boolean — see
 * `docs/PLATFORM_SUPPORT.md` for why.
 */
export type CaptureExclusion =
  | { state: "active"; verified_by: Evidence }
  | { state: "unsupported"; reason: string }
  | { state: "failed"; error: string }
  | { state: "unknown"; reason: string };

/** Only `active` means the overlay is genuinely hidden from screen capture. */
export function isCaptureExclusionConfirmed(c: CaptureExclusion): boolean {
  return c.state === "active";
}

export type CredentialStoreStatus =
  | { state: "available"; backend: string }
  | { state: "unavailable"; reason: string };

export type Diagnostics = {
  appVersion: string;
  os: string;
  arch: string;
  /** Present so we can prove the claim in the README, not as a feature. */
  electronPresent: boolean;
  credentialStore: CredentialStoreStatus;
  captureExclusion: CaptureExclusion;
  /** Egress purposes the network policy layer will accept. */
  allowedEgressPurposes: string[];
};

export async function getDiagnostics(): Promise<Diagnostics> {
  return invoke<Diagnostics>("diagnostics");
}
