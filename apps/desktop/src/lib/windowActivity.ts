/**
 * Marks the root while the main window has focus, so endless motion can stop
 * when it does not (`:root[data-window-active="false"]` in `styles.css`).
 *
 * The page cannot learn this any other way. Closing the window hides it to the
 * tray, and in WebView2 `document.visibilityState` still reads "visible" while it
 * is hidden, so the browser keeps animating for nobody. Measured on the debug app
 * on 2026-11-04: Dictation's breathing tile cost 17% of one core with the window
 * hidden, against 0.3% with it paused. Focus is the signal that does change on
 * hide and on minimise, and a window beside the one being dictated into is not
 * being looked at closely enough to miss a slowly breathing picture.
 */
export function installWindowActivity(): () => void {
  const root = document.documentElement;
  const update = () => {
    root.dataset.windowActive = String(document.hasFocus());
  };
  window.addEventListener("focus", update);
  window.addEventListener("blur", update);
  update();
  return () => {
    window.removeEventListener("focus", update);
    window.removeEventListener("blur", update);
    delete root.dataset.windowActive;
  };
}
