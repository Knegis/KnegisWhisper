import type { Page } from "../lib/pages";

/**
 * A screen's title, with the part of the application it belongs to over it, and
 * one-sentence intro, sitting outside `.view` so the
 * `.view > section:first-of-type` rule that sets a screen's first section
 * directly under this header still applies to the content beneath it.
 */
export function PageHeader({ page, eyebrow = null }: { page: Page; eyebrow?: string | null }) {
  return (
    <header className="page-header">
      {eyebrow === null ? null : <span className="section-label">{eyebrow}</span>}
      <h1>{page.title}</h1>
      <p className="page-intro">{page.intro}</p>
    </header>
  );
}
