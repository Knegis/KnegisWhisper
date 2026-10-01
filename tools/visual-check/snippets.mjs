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
    const toggle = [...document.querySelectorAll('.note-actions button')]
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
    const toggle = [...document.querySelectorAll('.note-actions button')]
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

/* ---------------------------------------------------------------- chip menus */

/**
 * Wraps the fixture `invoke`, once per document, so a check can reach states
 * the fixture defaults do not: a slow write, a refused one, a different read.
 * Nothing is stubbed until `stubInvoke` says so. Every call is logged, so a
 * check can say a write did or did not happen. Re-install after a reload.
 */
export function installInvokeStub() {
  return source(`
    if (window.__chipStub) return { installed: false };
    const stub = window.__chipStub = { delay: {}, reject: {}, merge: {}, log: [] };
    const original = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
    window.__TAURI_INTERNALS__.invoke = async (cmd, args, options) => {
      stub.log.push({ cmd, args: args ?? null });
      if (stub.delay[cmd]) await new Promise((resolve) => setTimeout(resolve, stub.delay[cmd]));
      if (stub.reject[cmd]) throw new Error(stub.reject[cmd]);
      const result = await original(cmd, args, options);
      return stub.merge[cmd] ? { ...result, ...stub.merge[cmd] } : result;
    };
    return { installed: true };
  `);
}

/**
 * Sets what the stub does per command: `delay` (ms before the call), `reject`
 * (an error message instead of the call) and `merge` (fields laid over an
 * object the fixture returns). `null` for a command clears it.
 */
export function stubInvoke({ delay = {}, reject = {}, merge = {} } = {}) {
  return source(`
    const stub = window.__chipStub;
    if (!stub) throw new Error('The invoke stub is not installed');
    const apply = (into, from) => {
      for (const [cmd, value] of Object.entries(from)) {
        if (value === null) delete into[cmd];
        else into[cmd] = value;
      }
    };
    apply(stub.delay, ${JSON.stringify(delay)});
    apply(stub.reject, ${JSON.stringify(reject)});
    apply(stub.merge, ${JSON.stringify(merge)});
    return true;
  `);
}

/** The arguments of every logged call to `cmd`, oldest first. */
export function stubCalls(cmd) {
  return source(`
    return (window.__chipStub?.log ?? [])
      .filter((entry) => entry.cmd === ${JSON.stringify(cmd)})
      .map((entry) => entry.args);
  `);
}

/** Empties the call log, leaving the stub's behaviour as it is. */
export function clearStubCalls() {
  return source(`
    if (window.__chipStub) window.__chipStub.log.length = 0;
    return true;
  `);
}

/** Runs a fixture command through the wrapped `invoke`, for state the page would otherwise need a click to reach. */
export function invokeFixture(cmd, args = {}) {
  return source(`
    return window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(() => true);
  `);
}

/** The dev console's dictation state hand-hold (`kw.dictationState`), as the session announces it. */
export function emitDictationState(state) {
  return source(`
    if (!globalThis.kw?.dictationState) throw new Error('The fixture dev console is missing');
    globalThis.kw.dictationState(${JSON.stringify(state)});
    return true;
  `);
}

/**
 * The first chip whose name -- its aria-label, or its text where it has none --
 * starts with `prefix`, as a DOM expression for the other snippets to build on.
 */
const chipExpression = (prefix) =>
  `[...document.querySelectorAll('button.chip')].find((item) => (item.getAttribute('aria-label') ?? item.textContent.trim()).startsWith(${JSON.stringify(prefix)}))`;

/** A chip's state: name, label, the ARIA a menu or toggle chip carries, and whether it is held. */
export function chipState(prefix) {
  return source(`
    const chip = ${chipExpression(prefix)};
    if (!chip) return null;
    const label = chip.querySelector('.chip-label');
    return {
      name: chip.getAttribute('aria-label'),
      text: label ? label.textContent : chip.textContent.trim(),
      expanded: chip.getAttribute('aria-expanded'),
      controls: chip.getAttribute('aria-controls'),
      haspopup: chip.getAttribute('aria-haspopup'),
      pressed: chip.getAttribute('aria-pressed'),
      ariaDisabled: chip.getAttribute('aria-disabled'),
      disabled: chip.disabled,
      title: chip.getAttribute('title'),
      description: chip.getAttribute('aria-description'),
      chevron: Boolean(chip.querySelector('.chip-chevron')),
      focused: document.activeElement === chip,
    };
  `);
}

/** The centre of the chip, to press with a real pointer. */
export function chipCenter(prefix) {
  return source(`
    const chip = ${chipExpression(prefix)};
    if (!chip) throw new Error('No chip is named ' + ${JSON.stringify(prefix)});
    const rect = chip.getBoundingClientRect();
    return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
  `);
}

/** The centre of the first element matching `selector` whose text is `text` (any, when `text` is null). */
export function centerOfText(selector, text) {
  return source(`
    const wanted = ${JSON.stringify(text)};
    const element = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find((item) => wanted === null || item.textContent.trim() === wanted);
    if (!element) throw new Error('No ' + ${JSON.stringify(selector)} + ' reads ' + wanted);
    const rect = element.getBoundingClientRect();
    return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
  `);
}

/** The centre of the open menu's row named `name`. */
export function optionCenter(name) {
  return source(`
    const row = [...document.querySelectorAll('.chip-menu [role="option"]')]
      .find((item) => document.getElementById(item.getAttribute('aria-labelledby'))?.textContent === ${JSON.stringify(name)});
    if (!row) throw new Error('The open menu has no row named ' + ${JSON.stringify(name)});
    const rect = row.getBoundingClientRect();
    return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
  `);
}

/** Whether a menu is open and placed (not still `visibility: hidden`). */
export function menuVisible() {
  return source(`
    const menu = document.querySelector('.chip-menu');
    return Boolean(menu) && getComputedStyle(menu).visibility === 'visible';
  `);
}

/**
 * The open menu: its box against the window and the control bar, its kind,
 * rows (with group, ticked, greyed and the reason a screen reader is given),
 * footer, and what has focus.
 */
export function menuState() {
  return source(`
    const menu = document.querySelector('.chip-menu');
    const bar = document.querySelector('.control-bar')?.getBoundingClientRect() ?? null;
    const active = document.activeElement;
    const describe = (element) => {
      if (!element || element === document.body) return 'body';
      if (element.getAttribute('role') === 'option') {
        return 'option:' + document.getElementById(element.getAttribute('aria-labelledby'))?.textContent;
      }
      if (element.classList.contains('chip-menu-footer')) return 'footer';
      if (element.getAttribute('role') === 'dialog') return 'dialog';
      if (element.classList.contains('chip')) return 'chip:' + (element.getAttribute('aria-label') ?? element.textContent.trim());
      return element.tagName.toLowerCase() + ':' + (element.textContent ?? '').trim().slice(0, 30);
    };
    const list = menu?.querySelector('[role="listbox"]') ?? null;
    const box = menu?.getBoundingClientRect() ?? null;
    return {
      open: Boolean(menu),
      viewport: { width: innerWidth, height: innerHeight },
      bar: bar && { top: bar.top, bottom: bar.bottom },
      box: box && { left: box.left, top: box.top, right: box.right, bottom: box.bottom },
      side: menu?.dataset.side ?? null,
      kind: !menu ? null : list ? 'listbox' : menu.getAttribute('role'),
      id: menu ? (list ? list.id : menu.id) : null,
      name: menu ? (list ? list.getAttribute('aria-label') : menu.getAttribute('aria-label')) : null,
      groups: menu ? [...menu.querySelectorAll('.chip-menu-label')].map((item) => item.textContent) : [],
      options: menu ? [...menu.querySelectorAll('[role="option"]')].map((row) => ({
        name: document.getElementById(row.getAttribute('aria-labelledby'))?.textContent ?? null,
        selected: row.getAttribute('aria-selected') === 'true',
        greyed: row.getAttribute('aria-disabled') === 'true',
        reason: row.querySelector('.chip-menu-reason')?.textContent ?? null,
      })) : [],
      footer: menu?.querySelector('.chip-menu-footer')?.textContent ?? null,
      active: describe(active),
    };
  `);
}

/**
 * The page under a control bar: the primary button, the privacy lines, the
 * page's error and notes, and how far the page scrolls sideways.
 */
export function taskPageState() {
  return source(`
    const start = document.querySelector('.control-start');
    const stop = [...document.querySelectorAll('.control-row button')]
      .find((item) => item.textContent.trim().startsWith('Stop'));
    const main = document.querySelector('.card .split-main');
    return {
      start: start ? { text: start.textContent.trim(), disabled: start.disabled } : null,
      stop: stop ? { text: stop.textContent.trim(), disabled: stop.disabled } : null,
      privacy: [...document.querySelectorAll('.control-privacy li')].map((item) => item.textContent.trim()),
      errors: [...document.querySelectorAll('.control-body > .error, .task > .error')].map((item) => item.textContent.trim()),
      notes: [...document.querySelectorAll('.control-body > p.control-note[role="status"]')].map((item) => ({
        text: item.textContent.trim(),
        link: item.querySelector('button.provider-link')?.textContent ?? null,
      })),
      hero: [...document.querySelectorAll('.task-hero button')].map((item) => item.textContent.trim()),
      scroll: {
        innerWidth: innerWidth,
        scrollWidth: document.scrollingElement.scrollWidth,
        mainClientWidth: main?.clientWidth ?? null,
        mainScrollWidth: main?.scrollWidth ?? null,
        barClientWidth: document.querySelector('.control-bar')?.clientWidth ?? null,
        barScrollWidth: document.querySelector('.control-bar')?.scrollWidth ?? null,
      },
    };
  `);
}

/** Puts keyboard focus on the chip, as Tab would have. */
export function focusChip(prefix) {
  return source(`
    const chip = ${chipExpression(prefix)};
    if (!chip) throw new Error('No chip is named ' + ${JSON.stringify(prefix)});
    chip.focus();
    return document.activeElement === chip;
  `);
}

/** Which colour scheme the page is drawing, and the open menu's own background. */
export function themeState() {
  return source(`
    const menu = document.querySelector('.chip-menu');
    return {
      dark: matchMedia('(prefers-color-scheme: dark)').matches,
      menuBackground: menu ? getComputedStyle(menu).backgroundColor : null,
      pageBackground: getComputedStyle(document.body).backgroundColor,
    };
  `);
}
