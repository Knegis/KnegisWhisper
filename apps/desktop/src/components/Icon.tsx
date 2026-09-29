import { FILLED, ICONS, ICON_STROKE, type IconName } from "../lib/icons";

/**
 * A Kit icon, drawn from `lib/icons.ts`'s `d` strings rather than from a
 * separate SVG asset per icon -- one component, one stylesheet rule
 * (`.icon` in `styles.css`), and every icon the same square-ended stroke.
 *
 * Sized with the `width`/`height` React props rather than a CSS variable, and
 * `strokeWidth` with a `style` prop rather than an injected `<style>` tag,
 * because the production CSP is `style-src 'self'` and both are ordinary DOM
 * attributes React sets directly -- neither is a stylesheet.
 */
export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  const strokeWidth = ICON_STROKE[name];
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

/** A filled mark (`FILLED`): a state, drawn as a solid shape rather than a stroke. */
export function IconFilled({ name, size = 18 }: { name: keyof typeof FILLED; size?: number }) {
  return (
    <svg className="icon-filled" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <path d={FILLED[name]} />
    </svg>
  );
}
