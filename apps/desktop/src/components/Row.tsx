/**
 * A label/value line, with an optional explanation underneath.
 *
 * Extracted from `App.tsx` unchanged. The `detail` slot exists because most of
 * the facts on these screens need a caveat next to them — "applied, but cannot
 * be verified" is only honest if it says why — and a component that has nowhere
 * to put the caveat encourages leaving it out.
 */
export function Row({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  // `| undefined` explicitly, because exactOptionalPropertyTypes is on: a
  // caller passing a computed `string | undefined` is the normal case here.
  detail?: string | undefined;
}) {
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <span className="row-value">
        {value}
        {detail ? <em className="row-detail">{detail}</em> : null}
      </span>
    </div>
  );
}
