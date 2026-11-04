// The same list as the `:is(...)` rules in `styles.css`: the list panel, Notes' scroller, the
// note and its transcript column, which scroll on their own in a wide window, and a page.
const surfaces = ".list-panel, .notes-main, .note-body, .note-side-body, .split-main";

/** Reveal native scrollbars during scrolling or while pointing at their track. */
export function installIdleScrollbars(): () => void {
  const timers = new Map<HTMLElement, ReturnType<typeof setTimeout>>();
  let hovered: HTMLElement | null = null;
  const reveal = (element: HTMLElement) => {
    clearTimeout(timers.get(element));
    element.classList.add("scrollbar-active");
    timers.set(element, setTimeout(() => {
      timers.delete(element);
      if (hovered !== element) element.classList.remove("scrollbar-active");
    }, 900));
  };
  const scroll = (event: Event) => {
    if (event.target instanceof HTMLElement && event.target.matches(surfaces)) reveal(event.target);
  };
  const move = (event: PointerEvent) => {
    let next: HTMLElement | null = null;
    if (event.pointerType !== "touch") {
      for (const element of document.querySelectorAll<HTMLElement>(surfaces)) {
        const rect = element.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) continue;
        const inside = event.clientX >= rect.left && event.clientX < rect.right
          && event.clientY >= rect.top && event.clientY < rect.bottom;
        if (inside && ((element.scrollHeight > element.clientHeight && event.clientX >= rect.right - 14)
          || (element.scrollWidth > element.clientWidth && event.clientY >= rect.bottom - 14))) next = element;
      }
    }
    const previous = hovered;
    hovered = next;
    if (previous && previous !== next) reveal(previous);
    if (next) reveal(next);
  };
  const leave = () => {
    const previous = hovered;
    hovered = null;
    if (previous) reveal(previous);
  };
  document.addEventListener("scroll", scroll, true);
  document.addEventListener("pointermove", move, { passive: true });
  document.documentElement.addEventListener("pointerleave", leave);
  window.addEventListener("blur", leave);
  return () => {
    document.removeEventListener("scroll", scroll, true);
    document.removeEventListener("pointermove", move);
    document.documentElement.removeEventListener("pointerleave", leave);
    window.removeEventListener("blur", leave);
    for (const timer of timers.values()) clearTimeout(timer);
    document.querySelectorAll(surfaces).forEach(element => element.classList.remove("scrollbar-active"));
  };
}
