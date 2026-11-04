import type { ReactNode } from "react";

/**
 * A label/value line, with an optional explanation underneath.
 *
 * Extracted from `App.tsx` unchanged. The `detail` slot exists because most of
 * the facts on these screens need a caveat next to them — "applied, but cannot
 * be verified" is only honest if it says why — and a component that has nowhere
 * to put the caveat encourages leaving it out.
 */
export type RowTone = "ok" | "bad" | "off";

/** Mint for what is fine, pink for what is not, grey for what is simply off. */
export function StatusDot({ tone }: { tone: RowTone }) {
  return <span className="status-dot" data-tone={tone} aria-hidden="true" />;
}

export function Row({
  label,
  value,
  detail,
  tone,
  code = false,
  children,
}: {
  label: string;
  value: ReactNode;
  // `| undefined` explicitly, because exactOptionalPropertyTypes is on: a
  // caller passing a computed `string | undefined` is the normal case here.
  detail?: ReactNode;
  /** A dot beside the value. Only for a state something actually read. */
  tone?: RowTone | undefined;
  /** The value is a path or an identifier, in the code face. */
  code?: boolean;
  /** What sits under the value in its column: a disclosure, usually. */
  children?: ReactNode;
}) {
  const shown = code ? <span className="row-code">{value}</span> : value;
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <div className="row-value">
        {tone ? <span className="status-ok"><StatusDot tone={tone} />{shown}</span> : shown}
        {detail ? <em className="row-detail">{detail}</em> : null}
        {children}
      </div>
    </div>
  );
}
