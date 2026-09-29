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

export function contentMeasure() {
  return source(`
    const content = document.querySelector('.content');
    if (!content) throw new Error('Content is missing');
    const rect = content.getBoundingClientRect();
    const style = getComputedStyle(content);
    return {
      width: rect.width,
      maxWidth: style.maxWidth,
      rem: getComputedStyle(document.documentElement).fontSize,
    };
  `);
}

export function notesGrid() {
  return source(`
    const workspace = document.querySelector('.note-workspace');
    if (!workspace) throw new Error('Note workspace did not open');
    const columns = getComputedStyle(workspace).gridTemplateColumns;
    return {
      columns,
      tracks: columns === 'none'
        ? 0
        : columns.split(/\\s+(?![^()]*\\))/).filter(Boolean).length,
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
 * Ticks or unticks the participants checkbox and presses Start recording, on
 * the Meetings task page -- the other half of what `selectMeetingProvider`
 * used to do in one place before the provider select moved to Settings.
 */
export function startMeetingRecording(systemAudio) {
  return source(`
    const checkbox = [...document.querySelectorAll('input[type="checkbox"]')]
      .find((item) =>
        item.closest('label')?.textContent.includes('other participants')
          || item.parentElement?.textContent.includes('other participants'),
      );
    if (!checkbox) throw new Error('System-audio checkbox is missing');
    if (checkbox.checked !== ${JSON.stringify(systemAudio)}) checkbox.click();
    const start = [...document.querySelectorAll('button')]
      .find((item) => item.textContent.trim() === 'Start recording');
    if (!start) throw new Error('Start recording button is missing');
    start.click();
    return { systemAudio: checkbox.checked };
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
