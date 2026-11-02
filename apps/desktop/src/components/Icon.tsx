import { FILLED, ICONS, iconStrokeWidth, type FilledIconName, type IconName } from "../lib/icons";

function isFilled(name: IconName): name is FilledIconName {
  return name in FILLED;
}

/**
 * An icon, drawn from `lib/icons.ts`'s `d` strings rather than from a separate
 * SVG asset per icon -- one component and two stylesheet rules: `.icon`, a soft
 * line with round ends, and `.icon-filled`, a solid shape. The name decides
 * which, so a caller holding a name, as `Chip` does, needs no second prop.
 *
 * Sized with the `width`/`height` React props rather than a CSS variable, and
 * `strokeWidth` with a `style` prop rather than an injected `<style>` tag,
 * because the production CSP is `style-src 'self'` and both are ordinary DOM
 * attributes React sets directly -- neither is a stylesheet.
 */
export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  if (isFilled(name)) {
    return (
      <svg className="icon-filled" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
        <path d={FILLED[name]} />
      </svg>
    );
  }
  const strokeWidth = iconStrokeWidth(name, size);
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      aria-hidden="true"
      {...(strokeWidth === undefined ? {} : { style: { strokeWidth } })}
    >
      <path d={ICONS[name]} />
    </svg>
  );
}
