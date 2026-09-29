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

/**
 * Opens a rail group -- `Settings`, `Developer` -- only if it is closed.
 *
 * A group header is a toggle, so a bare `clickText` on it closes a group that
 * something earlier left open, and the child click after it finds nothing.
 * This is the guard `shell-rail.mjs` writes out by hand before its own
 * header clicks, as one snippet.
 */
export function openGroup(label) {
  return source(`
    const header = [...document.querySelectorAll('nav.sidebar button.nav-group')]
      .find((item) => item.textContent.trim() === ${JSON.stringify(label)});
    if (!header) throw new Error('Could not find group ' + ${JSON.stringify(label)});
    const wasOpen = header.getAttribute('aria-expanded') === 'true';
    if (!wasOpen) header.click();
    return { label: ${JSON.stringify(label)}, wasOpen };
  `);
}

export function railGeometry() {
  return source(`
    const rail = document.querySelector('nav.sidebar');
    const shell = document.querySelector('.shell');
    if (!rail || !shell) throw new Error('Shell rail is missing');
    const rect = rail.getBoundingClientRect();
    const style = getComputedStyle(rail);
    return {
      position: style.position,
      rect: {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      },
      scrollX: window.scrollX,
      scrollY: window.scrollY,
      shellPaddingLeft: getComputedStyle(shell).paddingLeft,
      rem: getComputedStyle(document.documentElement).fontSize,
    };
  `);
}

export function groupState() {
  return source(`
    const read = (label) => {
      const button = [...document.querySelectorAll('button.nav-group')]
        .find((item) => item.textContent.trim() === label);
      if (!button) throw new Error('Missing group ' + label);
      const style = getComputedStyle(button);
      return {
        label,
        content: getComputedStyle(button, '::after').content,
        expanded: button.getAttribute('aria-expanded'),
        display: style.display,
        justifyContent: style.justifyContent,
        fontFamily: style.fontFamily,
        color: style.color,
        background: style.backgroundColor,
      };
    };
    return {
      settings: read('Settings'),
      developer: read('Developer'),
      fontLoaded: document.fonts.check('13px ProFontWindows'),
    };
  `);
}

export function railCurrent() {
  return source(`
    const resolveColor = (token) => {
      const probe = document.createElement('span');
      probe.style.color = 'var(' + token + ')';
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const rail = document.querySelector('nav.sidebar');
    const sidebarBackground = getComputedStyle(rail).backgroundColor;
    const buttons = [...rail.querySelectorAll('button')];
    const filled = buttons.filter((button) => {
      const background = getComputedStyle(button).backgroundColor;
      return background !== 'rgba(0, 0, 0, 0)'
        && background !== 'transparent'
        && background !== sidebarBackground;
    });
    const current = rail.querySelector('[aria-current="page"]');
    const settings = [...rail.querySelectorAll('.nav-group')]
      .find((item) => item.textContent.trim() === 'Settings');
    const child = rail.querySelector('.nav-child');
    return {
      childPaddingLeft: child ? getComputedStyle(child).paddingLeft : null,
      topPaddingLeft: getComputedStyle(buttons[0]).paddingLeft,
      filled: filled.map((item) => item.textContent.trim()),
      current: current
        ? { text: current.textContent.trim(), className: current.className }
        : null,
      settings: settings
        ? {
            color: getComputedStyle(settings).color,
            background: getComputedStyle(settings).backgroundColor,
            text: resolveColor('--text'),
          }
        : null,
    };
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

export function sidewaysState() {
  return source(`
    const current = document.querySelector('nav.sidebar [aria-current="page"]');
    const settings = [...document.querySelectorAll('button.nav-group')]
      .find((item) => item.textContent.trim() === 'Settings');
    const heading = document.querySelector('.content h1, .content h2');
    return {
      current: current ? current.textContent.trim() : null,
      expanded: settings?.getAttribute('aria-expanded') ?? null,
      heading: heading?.textContent.trim() ?? null,
    };
  `);
}

export function focusState() {
  return source(`
    const active = document.activeElement;
    if (!(active instanceof HTMLButtonElement) || !active.closest('nav.sidebar')) {
      return null;
    }
    const style = getComputedStyle(active);
    return {
      text: active.textContent.trim(),
      classList: [...active.classList],
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
