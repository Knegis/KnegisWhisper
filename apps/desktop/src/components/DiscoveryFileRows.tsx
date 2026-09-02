import type { DiscoveryPermissions } from "../generated/ipc-types";
import { discoveryPermissionsLabel } from "../lib/labels";
import { Row } from "./Row";

type Props = {
  discoveryPath: string;
  permissions: DiscoveryPermissions | null;
};

/**
 * The discovery-file facts shared by every local-API client.
 *
 * Keeping the complete block here matters: the CLI and MCP screens read the
 * same token file, so a wording or caveat that appears on only one of them is
 * a product contradiction, not a harmless presentation difference.
 */
export function DiscoveryFileRows({ discoveryPath, permissions }: Props) {
  const stated = permissions ? discoveryPermissionsLabel(permissions) : null;

  return (
    <>
      <Row label="Discovery file" value={discoveryPath} />
      {stated ? (
        <Row
          label="Discovery file permissions"
          value={stated.text}
          detail={stated.detail}
        />
      ) : (
        <p className="muted">
          Nothing to report about the discovery file's permissions: it does not
          exist while the local API is not running.
        </p>
      )}
      {stated?.caveat ? <p className="caveat">{stated.caveat}</p> : null}
    </>
  );
}
