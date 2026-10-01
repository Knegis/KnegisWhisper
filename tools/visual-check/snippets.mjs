/**
 * Renderer-side expressions used by the visual checks.
 *
 * Keeping generated snippets here makes selectors reviewable and lets syntax
 * tests catch quoting errors before a real measurement needs an application.
 */
const source = (body) => `(() => {
  ${body}
})()`;

export function removeFixtureBanner() {
  return source(`
    const banner = document.querySelector('[data-dev-fixtures="banner"]');
    const existed = Boolean(banner);
    banner?.remove();
    document.body.style.paddingTop = '';
    return { existed, paddingTop: document.body.style.paddingTop };
  `);
}

export function clickText(selector, text) {
  return source(`
    const wanted = ${JSON.stringify(text)}.toLowerCase();
    const button = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find((item) => item.textContent.trim().toLowerCase() === wanted);
    if (!button) throw new Error('Could not find ' + wanted);
    button.click();
    return { text: button.textContent.trim() };
  `);
}

/** Clicks the first element matching `selector`, whatever its text. */
export function clickSelector(selector) {
  return source(`
    const element = document.querySelector(${JSON.stringify(selector)});
    if (!element) throw new Error('Could not find ' + ${JSON.stringify(selector)});
    element.click();
    return { clicked: ${JSON.stringify(selector)} };
  `);
}

/** The rail's box, its entries, and which of them read as current. */
export function railState() {
  return source(`
    const rail = document.querySelector('nav.rail');
    if (!rail) throw new Error('The rail is missing');
    const rect = rail.getBoundingClientRect();
    return {
      rect: {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      },
      items: [...rail.querySelectorAll('.rail-item')]
        .map((item) => item.textContent.trim()),
      current: [...rail.querySelectorAll('.rail-item[aria-current="page"]')]
        .map((item) => item.textContent.trim()),
      rem: getComputedStyle(document.documentElement).fontSize,
    };
  `);
}

/**
 * Whether the list panel is on screen, and what it lists. `visible` is the
 * rendered answer -- computed display and width -- not the `hidden`
 * attribute alone, because a stylesheet can outrank that attribute.
 */
export function panelState() {
  return source(`
    const panel = document.querySelector('.list-panel');
    if (!panel) {
      return { present: false, visible: false, hidden: null, labels: [], items: [], current: [] };
    }
    const style = getComputedStyle(panel);
    const rect = panel.getBoundingClientRect();
    return {
      present: true,
      hidden: panel.hidden,
      display: style.display,
      width: rect.width,
      visible: style.display !== 'none' && rect.width > 0,
      title: panel.querySelector('.panel-title')?.textContent.trim() ?? null,
      labels: [...panel.querySelectorAll('.panel-label')]
        .map((item) => item.textContent.trim()),
      items: [...panel.querySelectorAll('.panel-item')]
        .map((item) => item.textContent.trim()),
      current: [...panel.querySelectorAll('.panel-item[aria-current]')]
        .map((item) => item.textContent.trim()),
    };
  `);
}

/** The title bar's list-panel toggle: its name and what it says it controls. */
export function panelToggle() {
  return source(`
    const toggle = document.querySelector('.app-titlebar button[aria-controls="list-panel"]');
    if (!toggle) throw new Error('The list-panel toggle is missing');
    return {
      label: toggle.getAttribute('aria-label'),
      expanded: toggle.getAttribute('aria-expanded'),
    };
  `);
}

/** The heading of the page in the card, which says which screen is open. */
export function pageHeading() {
  return source(`
    return document.querySelector('.card .page-header h1')?.textContent.trim() ?? null;
  `);
}

/** Horizontal overflow of the document and of the card's scrolling main area. */
export function overflowState() {
  return source(`
    const main = document.querySelector('.card .split-main');
    return {
      innerWidth: window.innerWidth,
      scrollWidth: document.scrollingElement.scrollWidth,
      mainClientWidth: main?.clientWidth ?? null,
      mainScrollWidth: main?.scrollWidth ?? null,
    };
  `);
}

/** The title bar search field: whether it has focus, its value, its popup. */
export function searchState() {
  return source(`
    const input = document.querySelector('input[role="combobox"]');
    if (!input) throw new Error('The search field is missing');
    const rect = input.getBoundingClientRect();
    return {
      focused: document.activeElement === input,
      value: input.value,
      placeholder: input.placeholder,
      width: rect.width,
      expanded: input.getAttribute('aria-expanded'),
      popup: Boolean(document.querySelector('.titlebar-search-popup')),
    };
  `);
}

/** Moves focus to the document body, so the next Tab starts from the top. */
export function blurActive() {
  return source(`
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    return document.activeElement === document.body;
  `);
}

/**
 * The open note's layout: the grid tracks of its article (`.note-detail` --
 * one, or two with the transcript column beside it), which of its tabs are
 * rendered, and the transcript column's width when there is one. A tab
 * counts as rendered by its computed display, not its `hidden` attribute.
 */
export function notesGrid() {
  return source(`
    const detail = document.querySelector('.note-detail');
    if (!detail) throw new Error('No note is open');
    const columns = getComputedStyle(detail).gridTemplateColumns;
    const side = detail.querySelector('.note-side');
    return {
      columns,
      tracks: columns === 'none'
        ? 0
        : columns.split(/\\s+(?![^()]*\\))/).filter(Boolean).length,
      tabs: [...detail.querySelectorAll('[role="tab"]')]
        .filter((tab) => getComputedStyle(tab).display !== 'none')
        .map((tab) => tab.textContent.trim()),
      sideWidth: side ? side.getBoundingClientRect().width : null,
    };
  `);
}

/**
 * The open note's Hide transcript / Show transcript button, or null where it
 * draws none (below 1280px): its name, whether it reads as expanded, the id it
 * names in `aria-controls` and whether that element exists, whether it has
 * focus, and which tab is selected.
 */
export function transcriptToggle() {
  return source(`
    const toggle = [...document.querySelectorAll('.note-topbar button')]
      .find((button) => /^(Hide|Show) transcript$/.test(button.getAttribute('aria-label') ?? ''));
    const selected = document.querySelector('.note-detail [role="tab"][aria-selected="true"]');
    if (!toggle) return { exists: false, selectedTab: selected?.textContent.trim() ?? null };
    const controls = toggle.getAttribute('aria-controls');
    return {
      exists: true,
      label: toggle.getAttribute('aria-label'),
      expanded: toggle.getAttribute('aria-expanded'),
      controls,
      controlsExists: controls === null ? null : Boolean(document.getElementById(controls)),
      focused: document.activeElement === toggle,
      selectedTab: selected?.textContent.trim() ?? null,
    };
  `);
}

/**
 * Focuses and presses the open note's transcript column button, as a keyboard
 * user would: `click()` alone leaves focus where it was.
 */
export function pressTranscriptToggle() {
  return source(`
    const toggle = [...document.querySelectorAll('.note-topbar button')]
      .find((button) => /^(Hide|Show) transcript$/.test(button.getAttribute('aria-label') ?? ''));
    if (!toggle) throw new Error('The transcript column button is missing');
    toggle.focus();
    toggle.click();
    return { pressed: toggle.getAttribute('aria-label') };
  `);
}

/**
 * Whether every tab sits inside `.notes-main`, horizontally: the same
 * comparison `notes-smoke.mjs` makes at 720x520 in the native window. Also
 * `.notes-main`'s own widths, since it is the note's scroller and would be
 * what scrolled sideways if the note did not fit.
 */
export function notesTabsContained() {
  return source(`
    const main = document.querySelector('.notes-main');
    if (!main) throw new Error('.notes-main is missing');
    const bounds = main.getBoundingClientRect();
    const tabs = [...document.querySelectorAll('[role="tab"]')];
    return {
      mainClientWidth: main.clientWidth,
      mainScrollWidth: main.scrollWidth,
      count: tabs.length,
      contained: tabs.every((tab) => {
        const rect = tab.getBoundingClientRect();
        return rect.left >= bounds.left && rect.right <= bounds.right;
      }),
    };
  `);
}

/**
 * A note's row in the library, found by its exact title as the smoke scripts
 * find it (`.notes-entry` holding `.notes-entry-title`), and whether that
 * title is cut off with an ellipsis rather than wrapping or overflowing.
 */
export function notesEntry(title) {
  return source(`
    const wanted = ${JSON.stringify(title)};
    const entry = [...document.querySelectorAll('.list-panel .notes-entry')]
      .find((item) => item.querySelector('.notes-entry-title')?.textContent === wanted);
    if (!entry) throw new Error('No library row is titled ' + wanted);
    const text = entry.querySelector('.notes-entry-title');
    const style = getComputedStyle(text);
    return {
      clientWidth: text.clientWidth,
      scrollWidth: text.scrollWidth,
      textOverflow: style.textOverflow,
      whiteSpace: style.whiteSpace,
      ellipsised: text.scrollWidth > text.clientWidth
        && style.textOverflow === 'ellipsis'
        && style.whiteSpace === 'nowrap',
    };
  `);
}

/**
 * Where keyboard focus is, and the outline it draws. `kind` says which part of
 * the shell holds it, so a Tab walk can be read back as "reached the rail".
 */
export function focusState() {
  return source(`
    const active = document.activeElement;
    if (!(active instanceof HTMLElement) || active === document.body) return null;
    const style = getComputedStyle(active);
    const kind = active.matches('nav.rail .rail-item') ? 'rail'
      : active.matches('input[role="combobox"]') ? 'search'
      : active.closest('.list-panel') ? 'panel'
      : active.closest('.app-titlebar') ? 'titlebar'
      : 'page';
    return {
      kind,
      text: active.getAttribute('aria-label') ?? active.textContent.trim(),
      outlineStyle: style.outlineStyle,
      outlineWidth: style.outlineWidth,
      outlineColor: style.outlineColor,
    };
  `);
}

export function meetingSnapshot() {
  return source(`
    const notify = document.querySelector('.notify');
    if (!notify) {
      return {
        notifyClass: null,
        strong: null,
        detail: null,
        detailIsAlert: false,
        live: null,
        liveNodeCount: 0,
        hasStop: false,
      };
    }
    const detail = notify.querySelector('.notify-detail');
    const live = notify.querySelector('.notify-live');
    return {
      notifyClass: notify.className,
      strong: notify.querySelector('strong')?.textContent.trim() ?? null,
      detail: detail?.textContent.trim() ?? null,
      detailIsAlert: Boolean(detail?.classList.contains('notify-alert')),
      live: live?.textContent ?? null,
      liveNodeCount: notify.querySelectorAll('.notify-live').length,
      hasStop: [...notify.querySelectorAll('button')]
        .some((button) => button.textContent.trim() === 'Stop'),
    };
  `);
}

export function invoke(command, args = undefined) {
  return source(`
    return window.__TAURI_INTERNALS__.invoke(
      ${JSON.stringify(command)},
      ${JSON.stringify(args)},
    );
  `);
}

/**
 * Sets the meeting provider. Since KNEG-334 this select lives on
 * Settings > Meetings, not on the Meetings task page, so it no longer also
 * handles the participants checkbox or the Start button -- those are a
 * separate screen now, and `startMeetingRecording` below drives them.
 */
export function selectMeetingProvider(label) {
  return source(`
    const wanted = ${JSON.stringify(label)}.toLowerCase();
    const select = [...document.querySelectorAll('select')].find((item) =>
      [...item.options].some((option) =>
        option.textContent.toLowerCase().includes(wanted),
      ),
    );
    if (!select) throw new Error('No usable provider select matches ' + wanted);
    const option = [...select.options].find((item) =>
      item.textContent.toLowerCase().includes(wanted),
    );
    select.value = option.value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    return { provider: option.textContent.trim() };
  `);
}

/**
 * Presses or releases the "Other participants" chip and presses Start
 * recording, on the Meetings task page -- the other half of what
 * `selectMeetingProvider` used to do in one place before the provider select
 * moved to Settings. The chip replaced a checkbox (KNEG-363); it is a toggle
 * button, read by `aria-pressed`, and disabled where system audio cannot be
 * captured. 50ms pass between the two presses so Start runs with the chip's
 * new state rather than the one React had before it re-rendered -- a timer
 * rather than a frame, because a hidden window may never draw one.
 */
export function startMeetingRecording(systemAudio) {
  return source(`
    return (async () => {
      const chip = [...document.querySelectorAll('button.chip')]
        .find((item) => item.textContent.trim() === 'Other participants');
      if (!chip) throw new Error('Other participants chip is missing');
      const pressed = () => chip.getAttribute('aria-pressed') === 'true';
      if (pressed() !== ${JSON.stringify(systemAudio)} && !chip.disabled) chip.click();
      await new Promise((resolve) => setTimeout(resolve, 50));
      const start = [...document.querySelectorAll('button')]
        .find((item) => item.textContent.trim() === 'Start recording');
      if (!start) throw new Error('Start recording button is missing');
      start.click();
      return { systemAudio: pressed() };
    })();
  `);
}

export function meetingProviderValue(label) {
  return source(`
    const wanted = ${JSON.stringify(label)}.toLowerCase();
    const option = [...document.querySelectorAll('select option')].find((item) =>
      item.textContent.toLowerCase().includes(wanted),
    );
    if (!option) throw new Error('No provider option matches ' + wanted);
    return option.value;
  `);
}

export function clickStop() {
  return source(`
    const stop = [...document.querySelectorAll('button')].find((item) =>
      item.textContent.trim() === 'Stop'
        || item.textContent.trim() === 'Stop and transcribe',
    );
    if (!stop) throw new Error('No stop button is present');
    stop.click();
    return stop.textContent.trim();
  `);
}
