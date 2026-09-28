import type { Page } from "../lib/pages";

/**
 * A screen's title and one-sentence intro, sitting outside `.view` so the
 * `.view > section:first-of-type` rule that levels a screen's first section
 * with the sidebar heading still applies to the content beneath it.
 */
export function PageHeader({ page }: { page: Page }) {
  return (
    <header className="page-header">
      <h1>{page.title}</h1>
      <p className="page-intro">{page.intro}</p>
    </header>
  );
}
