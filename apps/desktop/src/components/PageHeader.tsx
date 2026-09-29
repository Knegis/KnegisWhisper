import type { ReactNode } from "react";

import type { Page } from "../lib/pages";

/**
 * A screen's title and one-sentence intro, sitting outside `.view` so the
 * `.view > section:first-of-type` rule that sets a screen's first section
 * directly under this header still applies to the content beneath it.
 *
 * `actions` are controls that belong beside the title, right-aligned, and
 * wrapping under it when the column is too narrow for both. Only Providers has
 * any -- its search field and Test connections -- and because what they hold
 * is that page's own state, Providers draws this header itself, inside its
 * `.view`, and `App.tsx` leaves its own out (the first-section rule above is
 * unaffected: it matches the first `section`, which the header is not).
 */
export function PageHeader({ page, actions }: { page: Page; actions?: ReactNode }) {
  if (actions === undefined) {
    return (
      <header className="page-header">
        <h1>{page.title}</h1>
        <p className="page-intro">{page.intro}</p>
      </header>
    );
  }
  return (
    <header className="page-header page-header-actions">
      <div className="page-heading">
        <h1>{page.title}</h1>
        <p className="page-intro">{page.intro}</p>
      </div>
      <div className="page-actions">{actions}</div>
    </header>
  );
}
