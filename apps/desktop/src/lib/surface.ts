/**
 * Which of the four windows the shared frontend bundle is rendering.
 *
 * This has two readers: `main.tsx` stamps it on the root element before the
 * first frame, and `App.tsx` routes the bundle to its surface. A second copy
 * of this mapping would let those readers drift, which is why it lives here.
 * The three panel names match the `LABEL` constants in `overlay.rs`,
 * `meeting_window.rs`, and `update_window.rs`.
 */
export type Surface = "main" | "overlay" | "meeting" | "update";

/** The fragment selects a panel; every other URL is the settings shell. */
export function currentSurface(): Surface {
  switch (window.location.hash) {
    case "#overlay":
      return "overlay";
    case "#meeting":
      return "meeting";
    case "#update":
      return "update";
    default:
      return "main";
  }
}
