import type { DiscoveryPermissions } from "../generated/ipc-types";
import { discoveryPermissionsLabel } from "../lib/labels";
import { LearnMore } from "./LearnMore";
import { Row } from "./Row";

type Props = {
  discoveryPath: string;
  permissions: DiscoveryPermissions | null;
};

/**
 * Whether a discovery-file permission state is the one intended, so its
 * evidence -- a mode, an access list -- can go under Details. Every other
 * state is an exposure, and an exposure stays in view with what it means
 * (KNEG-343).
 */
export function permissionsAsIntended(p: DiscoveryPermissions): boolean {
  return p.state === "owner-only" || p.state === "owner-and-administrators";
}

/**
 * The discovery-file permission rows, as Developer › Local API, CLI and MCP
 * show them: the state, then either its evidence under Details (when it is
 * the intended one) or what it exposes, in view.
 */
export function DiscoveryPermissionRows({ permissions }: { permissions: DiscoveryPermissions }) {
  const stated = discoveryPermissionsLabel(permissions);
  if (permissionsAsIntended(permissions)) {
    return (
      <>
        <Row label="Discovery file permissions" value={stated.text} />
        <LearnMore summary="How this was checked">
          <p>{stated.detail}.</p>
          {stated.caveat ? <p>{stated.caveat}</p> : null}
        </LearnMore>
      </>
    );
  }
  return (
    <>
      <Row label="Discovery file permissions" value={stated.text} detail={stated.detail} />
      {stated.caveat ? <p className="caveat">{stated.caveat}</p> : null}
    </>
  );
}

/**
 * The discovery-file facts shared by every local-API client.
 *
 * Keeping the complete block here matters: the CLI and MCP screens read the
 * same token file, so a wording or caveat that appears on only one of them is
 * a product contradiction, not a harmless presentation difference.
 *
 * The path stays in view: on these developer screens it is what a person
 * needs, not detail (KNEG-343).
 */
export function DiscoveryFileRows({ discoveryPath, permissions }: Props) {
  return (
    <>
      <Row label="Discovery file" value={discoveryPath} />
      {permissions ? (
        <DiscoveryPermissionRows permissions={permissions} />
      ) : (
        <p className="muted">
          Nothing to report about the discovery file's permissions: it does not
          exist while the local API is not running.
        </p>
      )}
    </>
  );
}
