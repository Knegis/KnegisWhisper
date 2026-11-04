import type { BinaryLocation, DiscoveryPermissions } from "../generated/ipc-types";
import { binaryLocationLabel, discoveryPermissionsLabel } from "../lib/labels";
import { LearnMore } from "./LearnMore";
import { Row, StatusDot } from "./Row";

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
      <Row label="Discovery file permissions" tone="ok" value={stated.text}>
        <LearnMore summary="How this was checked">
          <p>{stated.detail}.</p>
          {stated.caveat ? <p>{stated.caveat}</p> : null}
        </LearnMore>
      </Row>
    );
  }
  return (
    <>
      <Row label="Discovery file permissions" tone="bad" value={stated.text} detail={stated.detail} />
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
      <Row label="Discovery file" code value={discoveryPath} />
      {permissions ? (
        <DiscoveryPermissionRows permissions={permissions} />
      ) : (
        <Row
          label="Discovery file permissions"
          value="Nothing to report"
          detail="it does not exist while the local API is not running"
        />
      )}
    </>
  );
}

/** Where the client's binary is. Both found states lead with its path. */
export function BinaryRow({ location }: { location: BinaryLocation }) {
  const stated = binaryLocationLabel(location);
  return location.state === "not-found" ? (
    <Row label="Binary" tone="bad" value={stated.text} detail={stated.detail} />
  ) : (
    <Row
      label="Binary"
      tone="ok"
      value={stated.text}
      detail={
        <>
          <code>{location.path}</code>
          {stated.detail.slice(location.path.length)}
        </>
      }
    />
  );
}

/** Whether the Local API is up, and the way to the screen that changes it. */
export function LocalApiRow({
  running,
  detail,
  onOpen,
}: {
  running: boolean;
  detail: string;
  onOpen: () => void;
}) {
  return (
    <Row
      label="Local API"
      value={
        <button type="button" className="row-link" onClick={onOpen}>
          <StatusDot tone={running ? "ok" : "off"} />
          {running ? "Running" : "Not running"}
        </button>
      }
      detail={running ? undefined : detail}
    />
  );
}
