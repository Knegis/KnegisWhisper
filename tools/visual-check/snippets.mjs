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

/**
 * `startMeetingRecording`'s other way in: sets the "Other participants" chip
 * the same way, then presses the call card's "Record this call", which the
 * Meetings page shows while a meeting app is in a call nobody is recording.
 * Returns what the chip reads when the button is pressed, and whether it was
 * enabled.
 */
export function recordThisCall(systemAudio) {
  return source(`
    return (async () => {
      const chip = [...document.querySelectorAll('button.chip')]
        .find((item) => item.textContent.trim() === 'Other participants');
      if (!chip) throw new Error('Other participants chip is missing');
      const pressed = () => chip.getAttribute('aria-pressed') === 'true';
      if (pressed() !== ${JSON.stringify(systemAudio)} && !chip.disabled) chip.click();
      await new Promise((resolve) => setTimeout(resolve, 50));
      const record = [...document.querySelectorAll('.call-card button')]
        .find((item) => item.textContent.trim() === 'Record this call');
      if (!record) throw new Error('Record this call button is missing');
      const enabled = !record.disabled;
      record.click();
      return { systemAudio: pressed(), enabled };
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

/* --------------------------------------------------------- meeting discard */

/** The names of the logged calls among `commands`, oldest first, so a check can read their order. */
export function stubSequence(commands) {
  return source(`
    const wanted = ${JSON.stringify(commands)};
    return (window.__chipStub?.log ?? [])
      .filter((entry) => wanted.includes(entry.cmd))
      .map((entry) => entry.cmd);
  `);
}

/**
 * The dev console's `kw.meetingWindowSilent`: whether the fixtures stop
 * answering for the meeting window when Stop or Discard asks it to save its
 * notes, so the page's 5 s wait runs out as it would for a window that hung.
 */
export function meetingWindowSilent(silent) {
  return source(`
    if (!globalThis.kw?.meetingWindowSilent) throw new Error('The fixture dev console has no meetingWindowSilent');
    globalThis.kw.meetingWindowSilent(${JSON.stringify(silent)});
    return true;
  `);
}

/**
 * Replaces `window.confirm` with a scripted answer, keeping every question
 * asked in `window.__confirmQuestions` and, where the invoke stub is
 * installed, logging it there as `window.confirm`, so its place among the
 * calls can be read. `mergeWhileOpen` is laid over the stub's `merge` while
 * the question is "open" -- after it is asked and before it is answered --
 * for what changes behind a modal dialog; it needs `installInvokeStub` first.
 * `invokeWhileOpen`, `{ cmd, args }`, is a command sent while the question is
 * open, not waited for -- another client changing something meanwhile, as the
 * local API can move a note to Trash while Empty trash is asking (KNEG-401).
 */
export function scriptConfirm(answer, mergeWhileOpen = null, invokeWhileOpen = null) {
  return source(`
    const merge = ${JSON.stringify(mergeWhileOpen)};
    const call = ${JSON.stringify(invokeWhileOpen)};
    window.__confirmQuestions = [];
    window.confirm = (question) => {
      window.__confirmQuestions.push(String(question));
      window.__chipStub?.log.push({ cmd: 'window.confirm', args: { question: String(question) } });
      if (merge !== null) {
        const stub = window.__chipStub;
        if (!stub) throw new Error('The invoke stub is not installed');
        for (const [cmd, value] of Object.entries(merge)) stub.merge[cmd] = value;
      }
      // Something else happening while the question is open, as another client of the app would.
      if (call !== null) void window.__TAURI_INTERNALS__.invoke(call.cmd, call.args);
      return ${JSON.stringify(answer)};
    };
    return true;
  `);
}

/** The questions the scripted `window.confirm` was asked, oldest first. */
export function confirmQuestions() {
  return source(`
    return window.__confirmQuestions ?? [];
  `);
}

/**
 * The Meetings page, live or idle: whether the live view is up, its state
 * label and its action buttons, the caveats and errors in view (the live
 * view's or the control bar's), and Start.
 */
export function meetingPageState() {
  return source(`
    const text = (item) => item.textContent.trim();
    const live = document.querySelector('.meeting-live');
    const start = document.querySelector('.control-start');
    return {
      live: Boolean(live),
      status: live?.querySelector('.live-status [role="status"]')?.textContent.trim() ?? null,
      actions: live ? [...live.querySelectorAll('.live-actions button')].map(text) : [],
      caveats: [...document.querySelectorAll('.live-caveats > .caveat, .control-body > .caveat')].map(text),
      errors: [...document.querySelectorAll('.live-caveats > .error, .control-body > .error')].map(text),
      start: start ? { text: text(start), disabled: start.disabled } : null,
    };
  `);
}

/* ------------------------------------------------------------- meeting plan */

/**
 * What the Meetings page says a meeting is saved as (KNEG-365): the name
 * field (its value, placeholder and accessible name), the live view's title,
 * and the titles of the list panel's rows under each heading -- "Now",
 * "Recent", "Interrupted".
 */
export function meetingPlanState() {
  return source(`
    const field = document.querySelector('.control-row input.meeting-name');
    const sections = {};
    for (const section of document.querySelectorAll('.list-panel .panel-section')) {
      const label = section.querySelector('.panel-label')?.textContent.trim();
      if (!label) continue;
      sections[label] = [...section.querySelectorAll('.panel-item-title')].map((item) => item.textContent.trim());
    }
    return {
      name: field ? { value: field.value, placeholder: field.placeholder, label: field.getAttribute('aria-label') } : null,
      liveTitle: document.querySelector('.meeting-live .live-title')?.textContent.trim() ?? null,
      sections,
    };
  `);
}

/* ------------------------------------------------------- meeting recordings */

/**
 * Settings > Meetings' Recordings section (KNEG-409): its heading, the
 * checkbox labelled `label` (whether it is there and ticked), and the
 * caveats and Learn more text in the section around it.
 */
export function recordingsSetting(label) {
  return source(`
    const wanted = ${JSON.stringify(label)};
    const box = [...document.querySelectorAll('label')]
      .find((item) => item.textContent.trim() === wanted)?.querySelector('input[type="checkbox"]') ?? null;
    const section = box?.closest('section') ?? null;
    const text = (item) => item.textContent.trim();
    return {
      found: Boolean(box),
      checked: box?.checked ?? null,
      disabled: box?.disabled ?? null,
      heading: section?.querySelector('h2')?.textContent.trim() ?? null,
      caveats: section ? [...section.querySelectorAll(':scope > .caveat')].map(text) : [],
      learnMore: section ? [...section.querySelectorAll('.learn-more .learn-more-body')].map(text) : [],
    };
  `);
}

/**
 * What the Meetings page says about its recordings: the control bar's notes,
 * the list panel's foot, the live view's sub-line, and the Learn more under
 * the outcome caveat.
 */
export function meetingRecordingNotes() {
  return source(`
    const text = (item) => item.textContent.trim();
    return {
      controlNotes: [...document.querySelectorAll('.control-body p.control-note')].map(text),
      panelFoot: [...document.querySelectorAll('.list-panel .panel-foot')].map(text),
      liveSub: document.querySelector('.meeting-live .live-sub')?.textContent.trim() ?? null,
      learnMore: [...document.querySelectorAll('.control-body > .learn-more .learn-more-body, .live-caveats > .learn-more .learn-more-body')].map(text),
    };
  `);
}

/**
 * The open note's recording, as Notes shows it: how many players there are,
 * how many transcript lines play when pressed (`.transcript-line-playable`,
 * which a line is only with a recording to play, since KNEG-448 took the
 * rows' own Play buttons away), its transcript rows, its recording details,
 * the "no recording attached" block, and which tab is selected.
 */
export function noteRecordingState() {
  return source(`
    const note = document.querySelector('.note-detail');
    const text = (item) => item.textContent.trim();
    return {
      title: note?.querySelector('input[aria-label="Title"]')?.value ?? null,
      tab: note?.querySelector('[role="tab"][aria-selected="true"]')?.textContent.trim() ?? null,
      audios: note ? note.querySelectorAll('audio').length : null,
      players: note ? note.querySelectorAll('.recording-player').length : null,
      playableLines: note ? note.querySelectorAll('.transcript-line-playable').length : null,
      rows: note ? [...note.querySelectorAll('.transcript-rows .transcript-text')].map(text) : [],
      details: note ? [...note.querySelectorAll('.recording-details p')].filter((item) => !item.closest('[hidden]')).map(text) : [],
      disposition: note ? [...note.querySelectorAll('.recording-disposition p')].filter(item => !item.closest('[hidden]')).map(text) : [],
      noRecording: note ? [...note.querySelectorAll('.recording-empty')].filter((item) => !item.closest('[hidden]')).map((item) => text(item.querySelector('p'))) : [],
    };
  `);
}

/**
 * Clicks the library row titled exactly `title`, as a person opens a note.
 */
export function openNotesEntry(title) {
  return source(`
    const wanted = ${JSON.stringify(title)};
    const entry = [...document.querySelectorAll('.list-panel .notes-entry')]
      .find((item) => item.querySelector('.notes-entry-title')?.textContent === wanted);
    if (!entry) throw new Error('No library row is titled ' + wanted);
    entry.click();
    return wanted;
  `);
}

/**
 * The open note's meta row under its title (KNEG-368): each item's text and
 * the path of its icon, and whether any item reaches past the row, the note
 * or the window.
 */
export function noteMetaState() {
  return source(`
    const note = document.querySelector('.note-detail');
    const row = note?.querySelector('.note-detail-header .note-meta') ?? null;
    const page = note?.querySelector('.note-page') ?? note;
    const items = row ? [...row.querySelectorAll(':scope > li')].map((item) => {
      const box = item.getBoundingClientRect();
      return {
        text: item.textContent.trim(),
        icon: item.querySelector('svg path')?.getAttribute('d') ?? null,
        left: Math.round(box.left),
        right: Math.round(box.right),
        height: Math.round(box.height),
      };
    }) : [];
    const rowBox = row?.getBoundingClientRect() ?? null;
    const pageBox = page?.getBoundingClientRect() ?? null;
    return {
      title: note?.querySelector('input[aria-label="Title"]')?.value ?? null,
      row: Boolean(row),
      items,
      innerWidth,
      scrollWidth: document.scrollingElement.scrollWidth,
      rowRight: rowBox ? Math.round(rowBox.right) : null,
      pageRight: pageBox ? Math.round(pageBox.right) : null,
      rowOverflows: row ? row.scrollWidth > row.clientWidth : null,
    };
  `);
}

/**
 * The Notes library's folders, each with the titles of the notes filed in
 * it, and the notes in none ("Not in a folder").
 */
export function notesFolders() {
  return source(`
    const titles = (root) => [...root.querySelectorAll('.notes-entry-title')].map((item) => item.textContent.trim());
    const folders = {};
    for (const folder of document.querySelectorAll('.list-panel .notes-folder')) {
      const name = folder.querySelector('.notes-folder-name')?.textContent.trim() ?? '';
      folders[name] = titles(folder);
    }
    const unfiled = [...document.querySelectorAll('.list-panel .panel-section')]
      .find((section) => section.querySelector('.panel-label')?.textContent.trim() === 'Not in a folder');
    return { folders, unfiled: unfiled ? titles(unfiled) : [] };
  `);
}

/**
 * People (KNEG-362). The open note's people item in its meta row: each
 * chip's name and its remove button's label, whether Add person's form is
 * open, the refusal shown under it, and the item's right edge. The library's
 * People list: each person's name, count and whether pressed. And the
 * panel's section headings, the notes under a "Notes with" one with its
 * actions, and how many folder groups are drawn.
 */
export function peopleState() {
  return source(`
    const text = (item) => item.textContent.trim();
    const item = document.querySelector('.note-detail .note-meta > li.note-people');
    const panel = document.querySelector('.list-panel');
    const sections = panel ? [...panel.querySelectorAll('.panel-section')] : [];
    const label = (section) => section.querySelector('.panel-label')?.textContent.trim() ?? null;
    const filtered = sections.find((section) => (label(section) ?? '').startsWith('Notes with '));
    const box = item?.getBoundingClientRect() ?? null;
    return {
      note: item ? {
        chips: [...item.querySelectorAll('.note-person')].map((chip) => ({
          name: chip.querySelector('.note-person-name')?.textContent.trim() ?? null,
          remove: chip.querySelector('button')?.getAttribute('aria-label') ?? null,
        })),
        formOpen: Boolean(item.querySelector('form')),
        email: item.querySelector('form input[type="email"]')?.value ?? null,
        error: item.querySelector('[role="alert"]')?.textContent.trim() ?? null,
        right: box ? Math.round(box.right) : null,
      } : null,
      people: [...(panel?.querySelectorAll('button.notes-person') ?? [])].map((row) => ({
        name: row.querySelector('.notes-person-name')?.textContent.trim() ?? null,
        count: Number(row.querySelector('.notes-folder-count [aria-hidden]')?.textContent ?? 'NaN'),
        pressed: row.getAttribute('aria-pressed') === 'true',
      })),
      sections: sections.map(label),
      filtered: filtered ? {
        label: label(filtered),
        titles: [...filtered.querySelectorAll('.notes-entry-title')].map(text),
        actions: [...filtered.querySelectorAll('.panel-action')].map(text),
      } : null,
      folders: panel ? panel.querySelectorAll('.notes-folder').length : 0,
      innerWidth,
      scrollWidth: document.scrollingElement.scrollWidth,
    };
  `);
}

/**
 * Trash (KNEG-401). The panel's Trash row (its count and whether pressed), the
 * panel's section headings, the library's note titles, the Trash list's rows,
 * the "Moved to Trash" line and its Undo, Trash's page in the main area, the
 * read-only note in Trash, and whether a workspace or an editor is open.
 */
export function trashState() {
  return source(`
    const text = (item) => item.textContent.trim();
    const panel = document.querySelector('.list-panel');
    const entry = panel?.querySelector('button.notes-trash') ?? null;
    const moved = document.querySelector('.notes-notices .notes-moved');
    const space = document.querySelector('.notes-main .trash-space');
    const view = document.querySelector('.notes-main .trashed-note');
    return {
      entry: entry ? {
        count: Number(entry.querySelector('.notes-folder-count [aria-hidden]')?.textContent ?? 'NaN'),
        pressed: entry.getAttribute('aria-pressed') === 'true',
      } : null,
      sections: panel ? [...panel.querySelectorAll('.panel-section .panel-label')].map(text) : [],
      library: panel ? [...panel.querySelectorAll('.notes-entry .notes-entry-title')].map(text) : [],
      trashed: panel ? [...panel.querySelectorAll('.notes-trashed')].map((row) => ({
        title: row.querySelector('.notes-entry-title')?.textContent.trim() ?? null,
        meta: row.querySelector('.panel-item-meta')?.textContent.trim() ?? null,
        current: row.getAttribute('aria-current') === 'page',
      })) : [],
      panelNotes: panel ? [...panel.querySelectorAll('.panel-note')].map(text) : [],
      moved: moved ? {
        text: moved.firstChild?.textContent.trim() ?? null,
        undo: [...moved.querySelectorAll('button')].map(text),
      } : null,
      space: space ? {
        lines: [...space.querySelectorAll('p')].map(text),
        emptyTrash: space.querySelector('button') ? { text: text(space.querySelector('button')), disabled: space.querySelector('button').disabled } : null,
      } : null,
      view: view ? {
        banner: view.querySelector('.trashed-note-banner p')?.textContent.trim() ?? null,
        buttons: [...view.querySelectorAll('.trashed-note-banner button')].map(text),
        title: view.querySelector('.trashed-note-title')?.textContent.trim() ?? null,
        meta: view.querySelector('.trashed-note-header .note-meta')?.textContent.trim() ?? null,
        inputs: view.querySelectorAll('input, textarea, [contenteditable="true"]').length,
      } : null,
      workspace: Boolean(document.querySelector('.notes-main .note-detail')),
      title: document.querySelector('.note-detail input[aria-label="Title"]')?.value ?? null,
      error: document.querySelector('.notes-notices .error')?.textContent.trim() ?? null,
      notices: (() => {
        const region = document.querySelector('.notes-main > .notes-notices');
        return region ? { role: region.getAttribute('role'), text: region.textContent.trim() } : null;
      })(),
      focus: (() => {
        const active = document.activeElement;
        if (!active || active === document.body) return null;
        return {
          tag: active.tagName.toLowerCase(),
          className: typeof active.className === 'string' ? active.className : '',
          text: active.textContent.trim().slice(0, 80),
          label: active.getAttribute('aria-label'),
          value: 'value' in active ? active.value : null,
        };
      })(),
      innerWidth,
      scrollWidth: document.scrollingElement.scrollWidth,
    };
  `);
}

/**
 * Opens a library row's menu as a right-click does: a `contextmenu` event on
 * the row titled exactly `title`, at its centre.
 */
export function openRowMenu(title) {
  return source(`
    const wanted = ${JSON.stringify(title)};
    const row = [...document.querySelectorAll('.list-panel .notes-entry')]
      .find((item) => item.querySelector('.notes-entry-title')?.textContent === wanted);
    if (!row) throw new Error('No library row is titled ' + wanted);
    const rect = row.getBoundingClientRect();
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 10, clientY: rect.top + 10, button: 2 }));
    return wanted;
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

/**
 * Scrolls the first element matching `selector` whose text is `text` into
 * view, as a person scrolls a panel to reach a row below its fold, and says
 * whether there was one.
 */
export function scrollToText(selector, text) {
  return source(`
    const wanted = ${JSON.stringify(text)};
    const element = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find((item) => item.textContent.trim() === wanted);
    element?.scrollIntoView({ block: 'nearest' });
    return Boolean(element);
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
      notes: [...document.querySelectorAll('.control-body > [role="status"] > p.control-note')].map((item) => ({
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

/**
 * The open language panel (`LocalLanguagePicker` in a chip's menu): the line
 * under the choices, the languages ticked, whether Detect automatically is
 * marked, and whether the picker is off.
 */
export function languagePanel() {
  return source(`
    const picker = document.querySelector('.chip-menu .local-language-picker');
    if (!picker) return null;
    return {
      note: picker.querySelector('p.muted')?.textContent ?? null,
      ticked: [...picker.querySelectorAll('.local-language-options label')]
        .filter((label) => label.querySelector('input')?.checked)
        .map((label) => label.textContent.trim()),
      detect: picker.querySelector(':scope > button')?.getAttribute('aria-pressed') ?? null,
      disabled: picker.disabled,
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

/* ------------------------------------------------------------- note playback */

/**
 * Which of a note's three player slots an `audio` element is in, as an
 * expression over `audio`: the transcript column's foot (`side`), Replay's
 * full player (`full`) or the mini player at the foot of the note (`mini`).
 */
const slotExpression = `(audio.closest('.note-side-player') ? 'side'
  : audio.closest('.note-full-player') ? 'full'
  : audio.closest('.note-mini-player') ? 'mini' : null)`;

/**
 * Every `audio` on the page -- its slot, whether it is paused, its time,
 * speed and ready state, whether it plays the fixtures' blob, and the
 * `data-probe` a check may tag it with -- and what the players say: their
 * error lines and their clocks.
 */
export function playerState() {
  return source(`
    return {
      audios: [...document.querySelectorAll('audio')].map((audio) => ({
        slot: ${slotExpression},
        hidden: Boolean(audio.closest('[hidden]')),
        paused: audio.paused,
        currentTime: audio.currentTime,
        duration: Number.isFinite(audio.duration) ? audio.duration : null,
        playbackRate: audio.playbackRate,
        readyState: audio.readyState,
        blob: audio.src.startsWith('blob:'),
        probe: audio.dataset.probe ?? null,
      })),
      errors: [...document.querySelectorAll('.recording-player .error')]
        .map((item) => item.textContent.trim()),
      clocks: [...document.querySelectorAll('.recording-clock')]
        .map((item) => item.textContent.trim()),
    };
  `);
}

/**
 * Samples every `audio` every 50ms into `window.__playbackSamples`, from now
 * on, replacing any sampler already running. A timer rather than animation
 * frames, which a headless page may not draw. `t` is `performance.now()`.
 */
export function startPlaybackSampler() {
  return source(`
    if (window.__playbackSampler) clearInterval(window.__playbackSampler);
    const samples = window.__playbackSamples = [];
    const take = () => samples.push({
      t: performance.now(),
      audios: [...document.querySelectorAll('audio')].map((audio) => ({
        slot: ${slotExpression},
        paused: audio.paused,
        currentTime: audio.currentTime,
        playbackRate: audio.playbackRate,
        readyState: audio.readyState,
        probe: audio.dataset.probe ?? null,
      })),
    });
    take();
    window.__playbackSampler = setInterval(take, 50);
    return performance.now();
  `);
}

/** The sampler's samples so far, oldest first. */
export function playbackSamples() {
  return source(`
    return window.__playbackSamples ?? [];
  `);
}

/** Chooses a speed on the player inside `container`, as its select would be changed by hand. */
export function choosePlayerSpeed(container, rate) {
  return source(`
    const select = document.querySelector(${JSON.stringify(`${container} .recording-player select`)});
    if (!select) throw new Error('No player speed in ' + ${JSON.stringify(container)});
    select.value = ${JSON.stringify(rate)};
    if (select.value !== ${JSON.stringify(rate)}) throw new Error('The player has no speed ' + ${JSON.stringify(rate)});
    select.dispatchEvent(new Event('change', { bubbles: true }));
    return select.value;
  `);
}

/* ------------------------------------------------------------ word timings */

/** Note `id`'s transcript rows, words and all, as the page's own `note_transcript_rows` answers. */
export function noteTranscriptRows(id) {
  return source(`
    return window.__TAURI_INTERNALS__.invoke('note_transcript_rows', { id: ${JSON.stringify(id)} });
  `);
}

/**
 * One read, in one task, of the recording's time and the open note's
 * transcript rows (KNEG-408): for each row whether it is the playing one
 * (`aria-current`) and selected, its text as drawn, how many word spans it
 * draws, which of them carry `transcript-word-current`, and the first span's
 * computed transition. Read together so the lit word and the time are of the
 * same moment.
 */
export function wordHighlightState() {
  return source(`
    const audio = document.querySelector('audio');
    const rows = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    return {
      time: audio ? audio.currentTime : null,
      paused: audio ? audio.paused : null,
      rows: rows.map((row) => {
        const words = [...row.querySelectorAll('.transcript-text .transcript-word')];
        return {
          current: row.getAttribute('aria-current') === 'true',
          selected: row.getAttribute('aria-selected') === 'true',
          text: row.querySelector('.transcript-text')?.textContent ?? null,
          words: words.length,
          lit: words.flatMap((word, at) => word.classList.contains('transcript-word-current') ? [at] : []),
          transition: words[0] ? getComputedStyle(words[0]).transitionDuration : null,
        };
      }),
    };
  `);
}

/** Records where the one recording was when its next seek began and when it ended, into `window.__seek`. */
export function watchSeek() {
  return source(`
    const audio = document.querySelector('audio');
    if (!audio) throw new Error('No recording on the page');
    const seek = window.__seek = { seeking: null, seeked: null };
    audio.addEventListener('seeking', () => { seek.seeking = audio.currentTime; }, { once: true });
    audio.addEventListener('seeked', () => { seek.seeked = audio.currentTime; }, { once: true });
    return true;
  `);
}

/** What `watchSeek` recorded so far. */
export function seekRecord() {
  return source(`
    return window.__seek ?? null;
  `);
}

/* ------------------------------------------------------- transcript column */

/**
 * Marks the root with `data-theme` for `scheme`, beside the emulated
 * `prefers-color-scheme` a check sets over CDP. The stylesheet reads only the
 * media query (`styles.css`, `@media (prefers-color-scheme: light)`); the
 * attribute is set so a rule keyed on it later would see the same scheme.
 * Returns which scheme the page is now drawing.
 */
export function markColorScheme(scheme) {
  return source(`
    document.documentElement.dataset.theme = ${JSON.stringify(scheme)};
    return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  `);
}

/**
 * The transcript column's player card (KNEG-448). Its box against the
 * column's padding box -- inside the column's left rule, which is the
 * column's own edge, not the card's to clear -- its border, corners and
 * background beside the resolved `--rule` and `--bg` (a token's value is
 * resolved by giving it to an element's `color`, so `#2e2e33` and
 * `rgb(46, 46, 51)` compare equal), the controls it holds, and where each
 * section mark's centre sits beside where the range's thumb would at that
 * fraction: `5px + (width - 10px) * fraction` of the range's box, the
 * mapping `styles.css` draws the thumb and the played part with.
 */
export function playerCardState() {
  return source(`
    const side = document.querySelector('.note-side');
    const card = side?.querySelector('.note-side-player');
    if (!side || !card) throw new Error('No player in the transcript column');
    const resolve = (value) => {
      const probe = document.createElement('i');
      probe.style.color = value;
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const root = getComputedStyle(document.documentElement);
    const raw = (name) => root.getPropertyValue(name).trim();
    const box = (element) => {
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    };
    const outer = side.getBoundingClientRect();
    const left = outer.left + side.clientLeft;
    const inner = { left, right: left + side.clientWidth, bottom: outer.top + side.clientTop + side.clientHeight };
    const rect = card.getBoundingClientRect();
    const style = getComputedStyle(card);
    const sides = ['Top', 'Right', 'Bottom', 'Left'];
    const corners = ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'];
    const play = card.querySelector('button[aria-label="Play"]');
    const row = card.querySelector('.recording-controls');
    const range = card.querySelector('input[type="range"]');
    const audio = card.querySelector('audio');
    return {
      scheme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
      dataTheme: document.documentElement.dataset.theme ?? null,
      inset: { left: rect.left - inner.left, right: inner.right - rect.right, bottom: inner.bottom - rect.bottom },
      card: box(card),
      position: style.position,
      borderWidths: sides.map((name) => style['border' + name + 'Width']),
      borderStyles: sides.map((name) => style['border' + name + 'Style']),
      borderColors: sides.map((name) => style['border' + name + 'Color']),
      radii: corners.map((name) => style['border' + name + 'Radius']),
      background: style.backgroundColor,
      tokens: { rule: raw('--rule'), bg: raw('--bg') },
      resolved: { rule: resolve(raw('--rule')), bg: resolve(raw('--bg')) },
      play: box(play),
      playMarginTop: play ? getComputedStyle(play).marginTop : null,
      controls: box(row),
      back: Boolean(card.querySelector('button[aria-label="Back 15 seconds"]')),
      forward: Boolean(card.querySelector('button[aria-label="Forward 15 seconds"]')),
      clock: card.querySelector('.recording-clock')?.textContent ?? null,
      selects: card.querySelectorAll('select').length,
      range: box(range),
      marks: [...card.querySelectorAll('.recording-mark')].map((mark) => {
        const at = mark.getBoundingClientRect();
        return { centre: (at.left + at.right) / 2, width: at.width, height: at.height, color: getComputedStyle(mark).backgroundColor };
      }),
      duration: audio && Number.isFinite(audio.duration) ? audio.duration : null,
    };
  `);
}

/**
 * Pauses the recording in `container` and puts it at `seconds`, as a check's
 * starting point; the seek's end is for the check to wait on
 * (`recordingAt`).
 */
export function pauseRecordingAt(container, seconds) {
  return source(`
    const audio = document.querySelector(${JSON.stringify(`${container} audio`)});
    if (!audio) throw new Error('No recording in ' + ${JSON.stringify(container)});
    audio.pause();
    audio.currentTime = ${JSON.stringify(seconds)};
    return true;
  `);
}

/** The recording in `container`: its time, whether it is paused, and whether a seek is under way. */
export function recordingAt(container) {
  return source(`
    const audio = document.querySelector(${JSON.stringify(`${container} audio`)});
    if (!audio) throw new Error('No recording in ' + ${JSON.stringify(container)});
    return { time: audio.currentTime, paused: audio.paused, seeking: audio.seeking };
  `);
}

/** Scrolls the transcript column to its end at once, not smoothly, and says how far that was. */
export function scrollSideToEnd() {
  return source(`
    const body = document.querySelector('.note-side .note-side-body');
    if (!body) throw new Error('No transcript column');
    body.scrollTo({ top: body.scrollHeight, behavior: 'instant' });
    return { scrollTop: body.scrollTop, maxScroll: body.scrollHeight - body.clientHeight };
  `);
}

/**
 * The transcript column at the end of its scroll: the last transcript line's
 * bottom, the card's top, the scroller's own box, whether the column is at
 * the end of its scroll, and the `--player-h` the note carries beside the
 * card's own height.
 */
export function lastLineState() {
  return source(`
    const body = document.querySelector('.note-side .note-side-body');
    const card = document.querySelector('.note-side .note-side-player');
    if (!body || !card) throw new Error('No transcript column with a player');
    const lines = body.querySelectorAll('.transcript-rows li');
    const last = lines[lines.length - 1];
    if (!last) throw new Error('The transcript column has no lines');
    const scroller = body.getBoundingClientRect();
    return {
      lines: lines.length,
      lastText: last.querySelector('.transcript-text')?.textContent ?? last.textContent,
      lastBottom: last.getBoundingClientRect().bottom,
      cardTop: card.getBoundingClientRect().top,
      cardHeight: card.offsetHeight,
      scroller: { top: scroller.top, bottom: scroller.bottom },
      scrollTop: body.scrollTop,
      maxScroll: body.scrollHeight - body.clientHeight,
      paddingBottom: getComputedStyle(body).paddingBottom,
      playerH: document.querySelector('.note-detail')?.style.getPropertyValue('--player-h') || null,
    };
  `);
}

/**
 * The line playing in the transcript column (`aria-current`), as the column
 * scrolled it into view: its box beside the card's top and the bottom of the
 * column's sticky head (the scroller's top when it has none), and how far the
 * column had to scroll to show it.
 */
export function playingLineState() {
  return source(`
    const body = document.querySelector('.note-side .note-side-body');
    const card = document.querySelector('.note-side .note-side-player');
    if (!body || !card) throw new Error('No transcript column with a player');
    const line = body.querySelector('.transcript-rows li[aria-current="true"]');
    const head = body.querySelector('.transcript-head');
    const scroller = body.getBoundingClientRect();
    const rect = line?.getBoundingClientRect() ?? null;
    return {
      index: line ? [...body.querySelectorAll('.transcript-rows li')].indexOf(line) : null,
      text: line?.querySelector('.transcript-text')?.textContent ?? null,
      top: rect?.top ?? null,
      bottom: rect?.bottom ?? null,
      clearTop: head ? head.getBoundingClientRect().bottom : scroller.top + body.clientTop,
      cardTop: card.getBoundingClientRect().top,
      scrollTop: body.scrollTop,
      playerH: document.querySelector('.note-detail')?.style.getPropertyValue('--player-h') || null,
    };
  `);
}

/** An element as a short selector-like string for a report: its tag, its classes, and its text when short. */
const DESCRIBE = `const describe = (element) => {
      const classes = typeof element.className === 'string' && element.className.trim() ? '.' + element.className.trim().split(/\\s+/).join('.') : '';
      const text = (element.textContent ?? '').trim();
      return element.tagName.toLowerCase() + classes + (text && text.length <= 24 ? ' "' + text + '"' : '');
    };`;

/**
 * The transcript beside a note, grouped as the M6 redesign draws it
 * (KNEG-448): its sticky head (the headings and buttons in it, its position),
 * each section toggle's state, title and count, the 6px marker of each turn,
 * how many per-line Play buttons the column (`.note-side`) holds,
 * and -- over the panel's `aside` and everything in it -- any element with a
 * rounded corner other than a control (a button, an input, a select, a
 * textarea, a mark or a word), and any computed weight above 400.
 */
export function transcriptColumnState() {
  return source(`
    ${DESCRIBE}
    const aside = document.querySelector('.note-side .transcript');
    if (!aside) throw new Error('No transcript in the column');
    const head = aside.querySelector('.transcript-head');
    const headStyle = head ? getComputedStyle(head) : null;
    const corners = ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'];
    const rounded = [];
    const heavy = [];
    for (const element of [aside, ...aside.querySelectorAll('*')]) {
      const style = getComputedStyle(element);
      if (!element.matches('button, input, select, textarea, mark, .transcript-word')) {
        const radii = corners.map((corner) => style['border' + corner + 'Radius']);
        if (radii.some((radius) => radius !== '0px')) rounded.push(describe(element) + ' ' + radii.join(' '));
      }
      if (Number(style.fontWeight) > 400) heavy.push(describe(element) + ' ' + style.fontWeight);
    }
    return {
      head: head ? {
        position: headStyle.position,
        top: headStyle.top,
        headings: [...head.querySelectorAll('h1, h2, h3, h4, h5, h6, [role="heading"]')].map((heading) => ({
          text: heading.textContent.trim(),
          drawn: getComputedStyle(heading).textTransform,
        })),
        buttons: [...head.querySelectorAll('button')].map((button) => ({
          label: button.getAttribute('aria-label'),
          disabled: button.disabled,
        })),
      } : null,
      toggles: [...aside.querySelectorAll('button.transcript-section-toggle')].map((button) => ({
        expanded: button.getAttribute('aria-expanded'),
        title: button.querySelector('.transcript-section-title')?.textContent ?? null,
        count: button.querySelector('.transcript-section-count')?.textContent ?? null,
        height: button.getBoundingClientRect().height,
      })),
      markers: [...aside.querySelectorAll('.transcript-turn-head')].map((turnHead) => {
        const marker = turnHead.querySelector('.transcript-turn-marker');
        if (!marker) return null;
        const rect = marker.getBoundingClientRect();
        return { width: rect.width, height: rect.height };
      }),
      lines: aside.querySelectorAll('.transcript-rows li').length,
      // Per-line Play buttons anywhere in the column, the card included (gone since KNEG-448's click model).
      rowPlays: document.querySelector('.note-side')?.querySelectorAll('.transcript-play').length ?? null,
      rounded,
      heavy,
    };
  `);
}

/**
 * Scrolls `scroller` down by `by` pixels at once, then reads where the open
 * note's transcript head stands against the scroller's top edge (inside its
 * border, where a sticky head sticks).
 */
export function scrollAndReadHead(scroller, by) {
  return source(`
    const element = document.querySelector(${JSON.stringify(scroller)});
    if (!element) throw new Error('No ' + ${JSON.stringify(scroller)});
    const before = element.scrollTop;
    element.scrollBy({ top: ${JSON.stringify(by)}, behavior: 'instant' });
    const heads = [...document.querySelectorAll('.note-detail .transcript .transcript-head')].filter((head) => head.getClientRects().length > 0);
    const head = heads[0] ?? null;
    return {
      before,
      after: element.scrollTop,
      scrollerTop: element.getBoundingClientRect().top + element.clientTop,
      headTop: head ? head.getBoundingClientRect().top : null,
      heads: heads.length,
      position: head ? getComputedStyle(head).position : null,
    };
  `);
}

/**
 * The transcript column's label and status line, and how the label is drawn,
 * for the column before and after its rows arrive.
 */
export function columnLabelState() {
  return source(`
    const side = document.querySelector('.note-side');
    if (!side) throw new Error('No transcript column');
    const label = side.querySelector('.transcript-head .transcript-label');
    const head = label?.closest('.transcript-head') ?? null;
    const style = label ? getComputedStyle(label) : null;
    return {
      label: label?.textContent ?? null,
      drawn: style ? { textTransform: style.textTransform, fontFamily: style.fontFamily, fontSize: style.fontSize, color: style.color } : null,
      head: head ? { height: head.getBoundingClientRect().height, left: label.getBoundingClientRect().left - side.getBoundingClientRect().left } : null,
      status: side.querySelector('[role="status"]')?.textContent ?? null,
      lines: side.querySelectorAll('.transcript-rows li').length,
      headings: [...side.querySelectorAll('h1, h2, h3, h4, h5, h6')].map((heading) => heading.textContent.trim()),
    };
  `);
}

/** Every transcript line's `aria-selected` in the open note, in stored order. */
export function lineSelection() {
  return source(`
    return [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')].map((line) => line.getAttribute('aria-selected'));
  `);
}

/**
 * Marks the first `button.transcript-speaker` in the column reading `name`
 * with `data-probe`, so the turn it belongs to can be found again after it is
 * pressed, and returns its centre.
 */
export function markSpeakerButton(name) {
  return source(`
    const button = [...document.querySelectorAll('.note-side button.transcript-speaker')]
      .find((item) => item.textContent.trim() === ${JSON.stringify(name)});
    if (!button) throw new Error('No speaker button reads ' + ${JSON.stringify(name)});
    document.querySelectorAll('[data-probe="rename"]').forEach((item) => item.removeAttribute('data-probe'));
    button.dataset.probe = 'rename';
    const rect = button.getBoundingClientRect();
    return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2, label: button.getAttribute('aria-label') };
  `);
}

/**
 * The open rename form beside the speaker button `markSpeakerButton` marked:
 * whether it sits in that button's `.transcript-turn`, what it is a child of,
 * and whether focus is in its text field.
 */
export function renameFormState() {
  return source(`
    const button = document.querySelector('[data-probe="rename"]');
    const forms = [...document.querySelectorAll('.note-detail form.transcript-rename')];
    const form = forms[0] ?? null;
    const turn = button?.closest('.transcript-turn') ?? null;
    const input = form?.querySelector('input[type="text"]') ?? null;
    const active = document.activeElement;
    return {
      forms: forms.length,
      label: form?.getAttribute('aria-label') ?? null,
      inPressedTurn: Boolean(form && turn && turn.contains(form)),
      parent: form ? [form.parentElement.tagName.toLowerCase(), ...form.parentElement.classList].join('.') : null,
      focusInInput: Boolean(input) && active === input,
      focused: active ? [active.tagName.toLowerCase(), ...active.classList].join('.') : null,
    };
  `);
}

/**
 * Line `index` of the column: a point inside its left padding (on the line,
 * off its text, words and buttons), whether it is the playing line, and its
 * computed background beside `--panel-hover` and `--band-wash` resolved from
 * the root (by giving each to an element's `color`).
 */
export function lineHoverState(index) {
  return source(`
    const line = document.querySelectorAll('.note-side .transcript .transcript-rows li')[${JSON.stringify(index)}];
    if (!line) throw new Error('No line ' + ${JSON.stringify(index)});
    const resolve = (name) => {
      const probe = document.createElement('i');
      probe.style.color = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const rect = line.getBoundingClientRect();
    return {
      point: { x: rect.left + 8, y: rect.top + rect.height / 2 },
      text: line.querySelector('.transcript-text')?.textContent ?? null,
      current: line.getAttribute('aria-current') === 'true',
      hovered: line.matches(':hover'),
      background: getComputedStyle(line).backgroundColor,
      panelHover: resolve('--panel-hover'),
      bandWash: resolve('--band-wash'),
    };
  `);
}

/**
 * A point to press on transcript line `index` of the open note (KNEG-448): inside the line's
 * left padding, so a press there lands on the line itself and not on a word or its text. The
 * line is first brought into view the way the column's own scrolls bring one (`nearest`, at once,
 * clear of the sticky head and the player card by the lines' scroll margins). `onLine` says
 * whether the element under the point is the line, so a press that would land on anything else
 * -- the card floating over it, a word -- is caught before it is made.
 */
export function linePadPoint(index) {
  return source(`
    const line = document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${JSON.stringify(index)}];
    if (!line) throw new Error('No transcript line ' + ${JSON.stringify(index)});
    line.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const rect = line.getBoundingClientRect();
    const pad = parseFloat(getComputedStyle(line).paddingLeft) || 0;
    const x = rect.left + Math.max(1, Math.min(8, pad / 2));
    const y = rect.top + Math.min(rect.height / 2, 11);
    return {
      x,
      y,
      pad,
      onLine: document.elementFromPoint(x, y) === line,
      text: line.querySelector('.transcript-text')?.textContent ?? null,
    };
  `);
}

/**
 * The open note's transcript lines as the click model leaves them (KNEG-448): which are selected
 * (`aria-selected="true"`), which is playing (`aria-current`), which hold a tab stop
 * (`tabindex="0"`), which has focus, and each one's classes for the selected and playing look;
 * plus what has focus when it is not a line.
 */
export function lineModelState() {
  return source(`
    const lines = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    const active = document.activeElement;
    const indexes = (test) => lines.flatMap((line, at) => test(line) ? [at] : []);
    return {
      lines: lines.length,
      selected: indexes((line) => line.getAttribute('aria-selected') === 'true'),
      playing: indexes((line) => line.getAttribute('aria-current') === 'true'),
      stops: indexes((line) => line.getAttribute('tabindex') === '0'),
      playable: indexes((line) => line.classList.contains('transcript-line-playable')),
      selectedLook: indexes((line) => line.classList.contains('transcript-line-selected')),
      playingLook: indexes((line) => line.classList.contains('transcript-line-playing')),
      focused: lines.indexOf(active),
      focusedWithin: lines.findIndex((line) => line.contains(active)),
      active: active ? [active.tagName.toLowerCase(), ...active.classList].join('.') + (active.getAttribute('aria-label') ? '[' + active.getAttribute('aria-label') + ']' : '') : null,
    };
  `);
}

/**
 * Where focus is after a Tab, for a walk through the transcript: whether it is inside the open
 * note's `.transcript` at all, the transcript line it is on (its index among every line, or -1),
 * and the element, described.
 */
export function focusPlace() {
  return source(`
    ${DESCRIBE}
    const active = document.activeElement;
    const lines = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    return {
      inTranscript: Boolean(active?.closest?.('.note-detail .transcript')),
      line: lines.indexOf(active),
      element: active && active !== document.body ? describe(active).slice(0, 80) : 'body',
    };
  `);
}

/**
 * The header line of the player card in `container` (KNEG-448's `PlayerHeader`): its height,
 * the speaker it names and its 6px marker's fill beside the resolved `--band` and `--muted`, or
 * its "Play selection" button and the button that clears the selection.
 */
export function cardHeaderState(container) {
  return source(`
    const slot = document.querySelector(${JSON.stringify(container)});
    if (!slot) throw new Error('No ' + ${JSON.stringify(container)});
    const resolve = (name) => {
      const probe = document.createElement('i');
      probe.style.color = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const header = slot.querySelector('.player-header');
    const marker = header?.querySelector('.player-header-marker') ?? null;
    const play = header?.querySelector('button.player-header-play') ?? null;
    const clear = header?.querySelector('button.player-header-clear') ?? null;
    return {
      present: Boolean(header),
      height: header ? header.getBoundingClientRect().height : null,
      speaker: header?.querySelector('.player-header-speaker')?.textContent ?? null,
      marker: marker ? { width: marker.getBoundingClientRect().width, height: marker.getBoundingClientRect().height, fill: getComputedStyle(marker).backgroundColor } : null,
      playSelection: play?.textContent ?? null,
      clear: clear ? clear.getAttribute('aria-label') : null,
      band: resolve('--band'),
      muted: resolve('--muted'),
    };
  `);
}

/**
 * Records, into `window.__landing`, where the recording in `container` lands after its next seek:
 * the time when that seek ended (`seeked`), and the time on the first animation frame after it
 * with the recording playing -- "where it was once playing", read in the page rather than after a
 * round trip to the check.
 */
export function watchLanding(container) {
  return source(`
    const audio = document.querySelector(${JSON.stringify(`${container} audio`)});
    if (!audio) throw new Error('No recording in ' + ${JSON.stringify(container)});
    const landing = window.__landing = { seeked: null, playingAt: null };
    audio.addEventListener('seeked', () => { landing.seeked = audio.currentTime; }, { once: true });
    const step = () => {
      if (window.__landing !== landing) return;
      if (landing.seeked !== null && !audio.paused && audio.readyState >= 3) { landing.playingAt = audio.currentTime; return; }
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
    return true;
  `);
}

/** What `watchLanding` recorded so far. */
export function landingRecord() {
  return source(`
    return window.__landing ?? null;
  `);
}

/**
 * The narrow layout's mini player: whether it is there, `hidden`, drawn, and whether it carries the
 * card's header line and its Follow chip (KNEG-448).
 */
export function miniPlayerState() {
  return source(`
    const mini = document.querySelector('.note-detail .note-mini-player');
    return {
      present: Boolean(mini),
      hidden: mini ? mini.hidden : null,
      drawn: mini ? mini.getClientRects().length > 0 : false,
      header: Boolean(mini?.querySelector('.player-header')),
      follow: Boolean(mini?.querySelector('button.follow-chip')),
      tab: document.querySelector('.note-detail [role="tab"][aria-selected="true"]')?.textContent.trim() ?? null,
    };
  `);
}

/**
 * The Follow chip in the player card in `container` (KNEG-448): whether it is there, its
 * `aria-pressed`, its text and how it is drawn, beside `--band`, `--band-ink` and `--muted`
 * resolved from the root (by giving each to an element's `color`), and the centre to press it at.
 */
export function followChipState(container) {
  return source(`
    const slot = document.querySelector(${JSON.stringify(container)});
    if (!slot) throw new Error('No ' + ${JSON.stringify(container)});
    const resolve = (name) => {
      const probe = document.createElement('i');
      probe.style.color = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const chip = slot.querySelector('button.follow-chip');
    const style = chip ? getComputedStyle(chip) : null;
    const rect = chip ? chip.getBoundingClientRect() : null;
    return {
      present: Boolean(chip),
      pressed: chip ? chip.getAttribute('aria-pressed') : null,
      text: chip ? chip.textContent.trim() : null,
      drawn: style ? {
        background: style.backgroundColor,
        color: style.color,
        fontFamily: style.fontFamily,
        fontSize: style.fontSize,
        textTransform: style.textTransform,
        radius: style.borderTopLeftRadius,
        height: rect.height,
      } : null,
      point: rect ? { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 } : null,
      band: resolve('--band'),
      bandInk: resolve('--band-ink'),
      muted: resolve('--muted'),
    };
  `);
}

/**
 * Starts recording, into `window.__playingLines`, every transcript line in `scroller` that becomes
 * the playing one (`aria-current`), read each animation frame: its index among the lines, the
 * scroller's `scrollTop` at that frame, and -- `settle` ms later, once a scroll it set off has had
 * time to end -- where the line stands beside the bottom of the transcript's sticky head and the
 * top of the card in `container`, the scroller's `scrollTop` then, whether it is still playing,
 * and the Follow chip's `aria-pressed`. A line already playing is recorded first.
 */
export function watchPlayingLines(scroller, container, settle) {
  return source(`
    const body = document.querySelector(${JSON.stringify(scroller)});
    const card = document.querySelector(${JSON.stringify(container)});
    if (!body || !card) throw new Error('No ' + ${JSON.stringify(scroller)} + ' with a card in ' + ${JSON.stringify(container)});
    const record = window.__playingLines = { changes: [] };
    const measure = (change, line) => {
      const rect = line.getBoundingClientRect();
      const head = body.querySelector('.transcript-head');
      change.settled = {
        current: line.getAttribute('aria-current') === 'true',
        drawn: line.getClientRects().length > 0,
        top: rect.top,
        bottom: rect.bottom,
        headBottom: head ? head.getBoundingClientRect().bottom : null,
        cardTop: card.getBoundingClientRect().top,
        scrollTop: body.scrollTop,
        follow: card.querySelector('button.follow-chip')?.getAttribute('aria-pressed') ?? null,
      };
    };
    let current = null;
    const step = () => {
      if (window.__playingLines !== record) return;
      const line = body.querySelector('.transcript-rows li[aria-current="true"]');
      if (line !== current) {
        current = line;
        if (line) {
          const change = { index: [...body.querySelectorAll('.transcript-rows li')].indexOf(line), scrollTop: body.scrollTop, settled: null };
          record.changes.push(change);
          setTimeout(() => measure(change, line), ${JSON.stringify(settle)});
        }
      }
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
    return true;
  `);
}

/** What `watchPlayingLines` recorded so far; stops it with `stop`. */
export function playingLinesRecord(stop = false) {
  return source(`
    const record = window.__playingLines ?? null;
    if (${JSON.stringify(stop)}) window.__playingLines = null;
    return record;
  `);
}

/**
 * The playing line in `scroller` beside the band Follow keeps it in (KNEG-448): the scroller's
 * box inside its border, less the transcript's sticky head at the top and the card -- the note's
 * `--player-h` plus 24px -- at the bottom. Both centres, and how far apart they are.
 */
export function followBandState(scroller) {
  return source(`
    const body = document.querySelector(${JSON.stringify(scroller)});
    if (!body) throw new Error('No ' + ${JSON.stringify(scroller)});
    const lines = [...body.querySelectorAll('.transcript-rows li')];
    const line = body.querySelector('.transcript-rows li[aria-current="true"]');
    const heads = [...body.querySelectorAll('.transcript-head')].filter((head) => head.getClientRects().length > 0);
    const head = heads[0] ?? null;
    const detail = document.querySelector('.note-detail');
    const playerH = detail ? parseFloat(getComputedStyle(detail).getPropertyValue('--player-h')) || 0 : 0;
    const top = body.getBoundingClientRect().top + body.clientTop;
    const headHeight = head ? head.getBoundingClientRect().height : 0;
    const band = { top: top + headHeight, bottom: top + body.clientHeight - playerH - 24 };
    band.centre = (band.top + band.bottom) / 2;
    const rect = line ? line.getBoundingClientRect() : null;
    const centre = rect ? (rect.top + rect.bottom) / 2 : null;
    return {
      index: line ? lines.indexOf(line) : null,
      line: rect ? { top: rect.top, bottom: rect.bottom, centre } : null,
      band,
      offset: centre === null ? null : centre - band.centre,
      scrollTop: body.scrollTop,
      headHeight,
      playerH,
    };
  `);
}

/**
 * Transcript line `index` of the open note beside the band Follow keeps lines in, inside
 * `scroller` (`followBandState`'s band): whether it is drawn and wholly inside it (±1px), how far
 * its centre is from the band's, and the scroller's `scrollTop`.
 */
export function lineBandState(scroller, index) {
  return source(`
    const body = document.querySelector(${JSON.stringify(scroller)});
    const line = document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${JSON.stringify(index)}];
    if (!body || !line) throw new Error('No ' + ${JSON.stringify(scroller)} + ' or no line ' + ${JSON.stringify(index)});
    const head = [...body.querySelectorAll('.transcript-head')].find((item) => item.getClientRects().length > 0) ?? null;
    const detail = document.querySelector('.note-detail');
    const playerH = detail ? parseFloat(getComputedStyle(detail).getPropertyValue('--player-h')) || 0 : 0;
    const top = body.getBoundingClientRect().top + body.clientTop;
    const band = { top: top + (head ? head.getBoundingClientRect().height : 0), bottom: top + body.clientHeight - playerH - 24 };
    const rect = line.getBoundingClientRect();
    const drawn = line.getClientRects().length > 0;
    return {
      drawn,
      top: rect.top,
      bottom: rect.bottom,
      band,
      inBand: drawn && rect.top >= band.top - 1 && rect.bottom <= band.bottom + 1,
      offset: (rect.top + rect.bottom) / 2 - (band.top + band.bottom) / 2,
      scrollTop: body.scrollTop,
    };
  `);
}

/**
 * A point inside `selector` that lands on nothing a person would operate -- no button, field,
 * link, editable text or focusable element -- found down its left edge, 6px in: for a press that
 * only says where the person last pressed. `plain` is false when there is none.
 */
export function blankPoint(selector) {
  return source(`
    const element = document.querySelector(${JSON.stringify(selector)});
    if (!element) throw new Error('No ' + ${JSON.stringify(selector)});
    ${DESCRIBE}
    const rect = element.getBoundingClientRect();
    for (let y = rect.top + 40; y < Math.min(rect.bottom, innerHeight) - 40; y += 20) {
      const x = rect.left + 6;
      const hit = document.elementFromPoint(x, y);
      if (!hit || !element.contains(hit)) continue;
      if (hit.closest('button, input, textarea, select, a, [contenteditable], [tabindex], [role="option"]')) continue;
      return { x, y, plain: true, hit: describe(hit).slice(0, 60) };
    }
    return { x: rect.left + 6, y: rect.top + 40, plain: false, hit: null };
  `);
}

/**
 * A point over `selector`, a scroller, for a real wheel: across its middle and 40% down, and
 * whether the element there is inside it (so not the card floating over its foot), with its
 * `scrollTop` now.
 */
export function scrollerPoint(selector) {
  return source(`
    const element = document.querySelector(${JSON.stringify(selector)});
    if (!element) throw new Error('No ' + ${JSON.stringify(selector)});
    const rect = element.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height * 0.4;
    const hit = document.elementFromPoint(x, y);
    return { x, y, inside: Boolean(hit) && element.contains(hit), scrollTop: element.scrollTop };
  `);
}

/**
 * The section toggle titled `title` in the open note's transcript, scrolled to the middle of its
 * scroller at once -- clear of the sticky head and of the card floating over the scroller's foot,
 * which the toggle has no scroll margins to keep out from under -- unless it is already clear of
 * both: its centre to press, its `aria-expanded`, and whether the toggle is what is at that point.
 */
export function sectionToggleState(title) {
  return source(`
    const toggle = [...document.querySelectorAll('.note-detail .transcript button.transcript-section-toggle')]
      .find((button) => button.querySelector('.transcript-section-title')?.textContent === ${JSON.stringify(title)});
    if (!toggle) throw new Error('No section toggle titled ' + ${JSON.stringify(title)});
    const before = toggle.getBoundingClientRect();
    if (!toggle.contains(document.elementFromPoint((before.left + before.right) / 2, (before.top + before.bottom) / 2))) {
      toggle.scrollIntoView({ block: 'center', behavior: 'instant' });
    }
    const rect = toggle.getBoundingClientRect();
    return {
      x: (rect.left + rect.right) / 2,
      y: (rect.top + rect.bottom) / 2,
      expanded: toggle.getAttribute('aria-expanded'),
      onToggle: toggle.contains(document.elementFromPoint((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2)),
    };
  `);
}

/** The `aria-expanded` of the open note's section toggle titled `title`, without moving anything. */
export function sectionExpanded(title) {
  return source(`
    const toggle = [...document.querySelectorAll('.note-detail .transcript button.transcript-section-toggle')]
      .find((button) => button.querySelector('.transcript-section-title')?.textContent === ${JSON.stringify(title)});
    return toggle ? toggle.getAttribute('aria-expanded') : null;
  `);
}

/**
 * Two points across the first line box of transcript line `index`'s text, for a real drag that
 * selects some of it: 2px in from the text's start, and up to 160px along the same line box. The
 * line is first brought into view as `linePadPoint` brings it. `onText` says whether the press
 * point is on the line's text.
 */
export function lineTextDrag(index) {
  return source(`
    const line = document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${JSON.stringify(index)}];
    if (!line) throw new Error('No transcript line ' + ${JSON.stringify(index)});
    line.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const text = line.querySelector('.transcript-text');
    if (!text) throw new Error('Line ' + ${JSON.stringify(index)} + ' has no text');
    const range = document.createRange();
    range.selectNodeContents(text);
    const first = range.getClientRects()[0];
    if (!first) throw new Error('Line ' + ${JSON.stringify(index)} + ' text is not drawn');
    const y = first.top + first.height / 2;
    const from = { x: first.left + 2, y };
    const to = { x: first.left + Math.min(first.width - 2, 160), y };
    const hit = document.elementFromPoint(from.x, from.y);
    return { from, to, onText: Boolean(hit) && text.contains(hit), text: text.textContent };
  `);
}

/** The page's text selection: what it reads, whether it is collapsed, and which transcript lines it reaches. */
export function textSelectionState() {
  return source(`
    const selection = getSelection();
    const lines = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    const range = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;
    return {
      text: selection ? selection.toString() : '',
      collapsed: selection ? selection.isCollapsed : true,
      lines: range ? lines.flatMap((line, at) => range.intersectsNode(line) ? [at] : []) : [],
    };
  `);
}

/**
 * The open note's transcript head while Find may be open (KNEG-448): whether it holds the find bar
 * (`[role="search"]`) or the label, the bar's field -- its `aria-label`, its value and whether it
 * has focus -- the count it shows (and whether that is hidden from assistive technology), what its
 * `role="status"` says, the labels of the bar's buttons, and what has focus, described: the Find
 * button, when the head shows it, is told apart. Also how many `mark.transcript-match` the
 * transcript holds.
 */
export function findBarState() {
  return source(`
    ${DESCRIBE}
    const heads = [...document.querySelectorAll('.note-detail .transcript > .transcript-head')].filter((head) => head.getClientRects().length > 0);
    const head = heads[0] ?? null;
    if (!head) throw new Error('No transcript head on screen');
    const bar = head.querySelector('[role="search"]');
    const input = bar ? bar.querySelector('input') : null;
    const count = bar ? bar.querySelector('.transcript-find-count') : null;
    const status = bar ? bar.querySelector('[role="status"]') : null;
    const findButton = head.querySelector('button.transcript-find');
    const active = document.activeElement;
    return {
      bar: Boolean(bar),
      height: head.getBoundingClientRect().height,
      label: head.querySelector('.transcript-label')?.textContent.trim() ?? null,
      input: input ? { label: input.getAttribute('aria-label'), value: input.value, focused: active === input } : null,
      shown: count ? count.textContent : null,
      shownHidden: count ? count.closest('[aria-hidden="true"]') !== null : null,
      spoken: status ? status.textContent : null,
      buttons: bar ? [...bar.querySelectorAll('button')].map((button) => button.getAttribute('aria-label')) : [],
      findButton: findButton ? { label: findButton.getAttribute('aria-label'), disabled: findButton.disabled, focused: active === findButton } : null,
      focus: active && active !== document.body ? describe(active).slice(0, 80) : 'body',
      marks: document.querySelectorAll('.note-detail .transcript mark.transcript-match').length,
    };
  `);
}

/**
 * Every Find mark in the open note's transcript (KNEG-448): its text, whether it is the current
 * one (`transcript-match-current`), the line it is in (its index among every line, and whether
 * that line is the playing one), its computed background, colour, text decoration and box shadow
 * -- a shadow would paint over the letters either side of a match inside a word -- and its box,
 * beside `--band`, `--band-ink`, `--band-wash` and `--band-text` resolved from the root (a token
 * given to a probe's `color`, so equal colours compare equal however they were written).
 */
export function findMarksState() {
  return source(`
    const resolve = (name) => {
      const probe = document.createElement('i');
      probe.style.color = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const lines = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    const marks = [...document.querySelectorAll('.note-detail .transcript mark.transcript-match')];
    return {
      marks: marks.map((mark) => {
        const style = getComputedStyle(mark);
        const line = mark.closest('li');
        return {
          text: mark.textContent,
          current: mark.classList.contains('transcript-match-current'),
          line: lines.indexOf(line),
          playing: line?.getAttribute('aria-current') === 'true',
          background: style.backgroundColor,
          color: style.color,
          decoration: style.textDecorationLine,
          radius: style.borderTopLeftRadius,
          shadow: style.boxShadow,
          box: (() => { const rect = mark.getBoundingClientRect(); return { left: rect.left, top: rect.top, width: rect.width, height: rect.height }; })(),
        };
      }),
      band: resolve('--band'),
      bandInk: resolve('--band-ink'),
      bandWash: resolve('--band-wash'),
      bandText: resolve('--band-text'),
    };
  `);
}

/**
 * Transcript line `index` of the open note as Find and the word highlight leave it (KNEG-448),
 * with the recording's time, in one read: whether it is the playing line, how many Find marks and
 * word spans it draws, which of its words are lit, and how many words are lit anywhere in the
 * transcript.
 */
export function findLineState(index) {
  return source(`
    const line = document.querySelectorAll('.note-detail .transcript .transcript-rows li')[${JSON.stringify(index)}];
    if (!line) throw new Error('No transcript line ' + ${JSON.stringify(index)});
    const words = [...line.querySelectorAll('.transcript-word')];
    return {
      time: document.querySelector('.note-detail audio')?.currentTime ?? null,
      playing: line.getAttribute('aria-current') === 'true',
      text: line.querySelector('.transcript-text')?.textContent ?? null,
      marks: line.querySelectorAll('mark.transcript-match').length,
      words: words.length,
      lit: words.flatMap((word, at) => word.classList.contains('transcript-word-current') ? [at] : []),
      litAnywhere: document.querySelectorAll('.note-detail .transcript .transcript-word-current').length,
    };
  `);
}

/**
 * Note `id` with a long meeting's transcript in place of its own (KNEG-448): `invoke` is wrapped,
 * as `follow-ups.mjs` wraps it, so `note_transcript_rows` for `id` answers `count` rows made here,
 * and every other call goes on to the fixtures. The rows start 5 s apart from 0 s and each lasts
 * its 5 s; three diarised speakers ("Maja", "Jonas" and the unnamed "Speaker 3", as note 106 names
 * them) take turns of 1, 2, 3 and 4 lines in rotation; and every tenth row, from the first on
 * (0 s, 50 s, … 300 s, …), is timed word by word as local Whisper times it, its words spelling its
 * text. Run before the
 * note opens. `window.__longTranscript` records, with `performance.now()`, when the rows are
 * handed back (`resolvedAt`), when the first of them is in the column (`shownAt`, a
 * MutationObserver's callback after React's commit) and the next animation frame (`frameAt`, the
 * frame that paints them) and the task after it (`paintedAt`), and every long task from now until
 * `longTasksRecord` stops it (`loadTasks`).
 */
export function injectLongTranscript(id, count) {
  return source(`
    if (window.__longTranscript) return { installed: false };
    const id = ${JSON.stringify(id)};
    const count = ${JSON.stringify(count)};
    const speakers = [['speaker_0', 'Maja'], ['speaker_1', 'Jonas'], ['speaker_2', 'Speaker 3']];
    const sayings = [
      'Vi tittar på siffrorna för förra kvartalet innan vi går vidare.',
      'Det där behöver vi stämma av med kunden först.',
      'Jag kan ta fram ett förslag till nästa vecka.',
      'Bygget har varit grönt hela veckan, så det ser bra ut.',
      'Har någon hunnit läsa igenom underlaget som skickades ut?',
      'Vi lägger det på listan och tar det i nästa sprint.',
      'Det låter rimligt, men vi behöver mer tid för testerna.',
      'Kan vi flytta den punkten till slutet av mötet?',
    ];
    const rows = [];
    let speaker = 0;
    let left = 1;
    let turn = 0;
    for (let index = 0; index < count; index += 1) {
      if (left === 0) {
        turn += 1;
        speaker = turn % speakers.length;
        left = (turn % 4) + 1;
      }
      left -= 1;
      const start = index * 5;
      const text = sayings[index % sayings.length];
      const words = index % 10 === 0
        ? text.split(' ').map((word, at) => ({ text: word, start: Math.round((start + 0.2 + at * 0.4) * 100) / 100, end: Math.round((start + 0.2 + at * 0.4 + 0.32) * 100) / 100 }))
        : [];
      rows.push({ index, startSeconds: start, endSeconds: start + 5, speaker: speakers[speaker][1], speakerId: speakers[speaker][0], text, words });
    }
    // Long tasks are reported only where the browser has the entry type; without it, nothing is observed rather than 0.
    const observing = typeof PerformanceObserver === 'function' && (PerformanceObserver.supportedEntryTypes ?? []).includes('longtask');
    const record = window.__longTranscript = { id, count, observing, resolvedAt: null, shownAt: null, frameAt: null, paintedAt: null, loadTasks: [] };
    const loadObserver = observing ? new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) record.loadTasks.push({ start: entry.startTime, duration: entry.duration });
    }) : null;
    loadObserver?.observe({ type: 'longtask', buffered: false });
    record.stop = () => loadObserver?.disconnect();
    const shown = new MutationObserver(() => {
      if (record.shownAt !== null || !document.querySelector('.note-side .transcript-rows li')) return;
      record.shownAt = performance.now();
      shown.disconnect();
      requestAnimationFrame(() => {
        record.frameAt = performance.now();
        setTimeout(() => { record.paintedAt = performance.now(); }, 0);
      });
    });
    shown.observe(document.body, { childList: true, subtree: true });
    const original = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
    window.__TAURI_INTERNALS__.invoke = async (cmd, args, options) => {
      if (cmd === 'note_transcript_rows' && args?.id === id) {
        record.resolvedAt = performance.now();
        return rows.map((row) => ({ ...row, words: row.words.map((word) => ({ ...word })) }));
      }
      return original(cmd, args, options);
    };
    return { installed: true, rows: rows.length, worded: rows.filter((row) => row.words.length > 0).length };
  `);
}

/** What `injectLongTranscript` recorded of the column's first paint, and the long tasks while it loaded; stops that watch. */
export function longTranscriptLoad() {
  return source(`
    const record = window.__longTranscript;
    if (!record) throw new Error('No long transcript was injected');
    record.stop();
    const { id, count, observing, resolvedAt, shownAt, frameAt, paintedAt, loadTasks } = record;
    return { id, count, observing, resolvedAt, shownAt, frameAt, paintedAt, loadTasks };
  `);
}

/**
 * Starts recording, into `window.__longTasks`, every long task from now (a `PerformanceObserver`
 * for `longtask`, not buffered, so nothing from before counts), and at each `timeupdate` of the
 * recording in `container` its time, its rate, the playing line (`aria-current`) -- read as the
 * event fires, so it is the line of the render before -- the Follow chip's `aria-pressed`, and
 * `scroller`'s `scrollTop`. Resolves `{ supported: false }`, observing nothing, where the browser
 * has no `longtask` entries (`PerformanceObserver.supportedEntryTypes`), so a count of none is
 * never read from an observer that was never registered.
 */
export function watchLongTasks(container, scroller) {
  return source(`
    const audio = document.querySelector(${JSON.stringify(`${container} audio`)});
    const body = document.querySelector(${JSON.stringify(scroller)});
    if (!audio || !body) throw new Error('No recording in ' + ${JSON.stringify(container)} + ' or no ' + ${JSON.stringify(scroller)});
    const supported = typeof PerformanceObserver === 'function' && (PerformanceObserver.supportedEntryTypes ?? []).includes('longtask');
    if (!supported) return { supported: false };
    const lines = [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')];
    const record = window.__longTasks = { startedAt: performance.now(), tasks: [], updates: [] };
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) record.tasks.push({ start: entry.startTime, duration: entry.duration });
    });
    observer.observe({ type: 'longtask', buffered: false });
    const update = () => {
      const line = document.querySelector('.note-detail .transcript .transcript-rows li[aria-current="true"]');
      record.updates.push({
        at: performance.now(),
        time: audio.currentTime,
        rate: audio.playbackRate,
        line: line ? lines.indexOf(line) : null,
        follow: document.querySelector(${JSON.stringify(`${container} button.follow-chip`)})?.getAttribute('aria-pressed') ?? null,
        scrollTop: body.scrollTop,
      });
    };
    audio.addEventListener('timeupdate', update);
    record.observer = observer;
    record.stopUpdates = () => audio.removeEventListener('timeupdate', update);
    return { supported: true };
  `);
}

/**
 * What `watchLongTasks` recorded, then a control, then stopped. The watched window ends now
 * (`endedAt`); one task in the page then runs `controlMs` ms on purpose, and the observer must
 * report it -- tasks from then on are the control's, not the window's -- so a window with no long
 * task was watched by an observer that does see one.
 */
export function longTasksRecord(controlMs = 120) {
  return source(`
    const record = window.__longTasks;
    if (!record) throw new Error('No long-task watch was started');
    record.stopUpdates();
    const endedAt = performance.now();
    return (async () => {
      await new Promise((done) => setTimeout(() => {
        const from = performance.now();
        while (performance.now() - from < ${JSON.stringify(controlMs)}) { /* the control's long task */ }
        done();
      }, 0));
      await new Promise((done) => setTimeout(done, 300));
      for (const entry of record.observer.takeRecords()) record.tasks.push({ start: entry.startTime, duration: entry.duration });
      record.observer.disconnect();
      window.__longTasks = null;
      return {
        startedAt: record.startedAt,
        endedAt,
        tasks: record.tasks.filter((task) => task.start < endedAt),
        control: record.tasks.filter((task) => task.start >= endedAt),
        updates: record.updates,
      };
    })();
  `);
}

/**
 * How many of the note's components React rendered for one `timeupdate` of the recording in
 * `container`, read from React's own fibers (no counter in the app): after a `timeupdate` and
 * two frames, by when React has committed it, each component's props object and first hook are
 * taken -- from both copies of its fiber, the one on screen and the one worked on last -- for the
 * workspace, the card's player and header, the panel, the sections' wrapper, and every section,
 * turn and line; then the same after the next `timeupdate`. React gives a component it renders
 * again a new props object when its parent drew it afresh, and new hooks whenever it runs, so a
 * render shows as an object it did not have before; one a memo comparison kept shows none.
 * Resolves to the count per component, how many it found, the `timeupdate`s between the two
 * reads (one, or the count is of more), and the playing line at each read (the same, or the count
 * includes a line change).
 */
export function rendersForOneTimeupdate(container) {
  return source(`
    const audio = document.querySelector(${JSON.stringify(`${container} audio`)});
    if (!audio) throw new Error('No recording in ' + ${JSON.stringify(container)});
    const fiberOf = (element) => {
      const key = Object.keys(element).find((name) => name.startsWith('__reactFiber$'));
      return key ? element[key] : null;
    };
    // A function's name as the bundler left it: Vite's dev build renames a forwardRef's render
    // function that shares its component's name ("RecordingPlayer2").
    const nameOf = (fiber) => (typeof fiber.type === 'function' ? fiber.type.name : fiber.type?.render?.name ?? '').replace(/\\d+$/, '');
    // The nearest component above \`element\` called \`name\`: for a memo with a comparison, the
    // function inside it, which is worked on only when it renders.
    const owner = (element, name) => {
      for (let fiber = fiberOf(element); fiber; fiber = fiber.return) if (nameOf(fiber) === name) return fiber;
      return null;
    };
    const NAMES = ['NoteWorkspace', 'RecordingPlayer', 'PlayerHeader', 'TranscriptPanel', 'TranscriptSections', 'Section', 'Turn', 'Line'];
    const instances = (name) => {
      const detail = document.querySelector('.note-detail');
      const first = (selector) => [...document.querySelectorAll(selector)].slice(0, 1);
      switch (name) {
        case 'NoteWorkspace': return first('.note-detail');
        case 'RecordingPlayer': return first(${JSON.stringify(`${container} .recording-player`)});
        case 'PlayerHeader': return first(${JSON.stringify(`${container} .player-header`)});
        case 'TranscriptPanel': return first('.note-detail .transcript');
        case 'TranscriptSections': return first('.note-detail .transcript .transcript-turn');
        case 'Section': return [...detail.querySelectorAll('.transcript .transcript-section')];
        case 'Turn': return [...detail.querySelectorAll('.transcript .transcript-turn')];
        default: return [...detail.querySelectorAll('.transcript .transcript-rows li')];
      }
    };
    const read = () => {
      const byName = new Map();
      for (const name of NAMES) {
        const seen = new Map();
        for (const element of instances(name)) {
          const fiber = owner(element, name);
          seen.set(element, fiber === null ? null
            : new Set([fiber.memoizedProps, fiber.memoizedState, fiber.alternate?.memoizedProps, fiber.alternate?.memoizedState].filter((value) => value !== null && typeof value === 'object')));
        }
        byName.set(name, seen);
      }
      const playing = document.querySelector('.note-detail .transcript .transcript-rows li[aria-current="true"]');
      return { byName, line: playing ? [...document.querySelectorAll('.note-detail .transcript .transcript-rows li')].indexOf(playing) : null, time: audio.currentTime };
    };
    const frames = () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const nextUpdate = () => new Promise((done) => audio.addEventListener('timeupdate', done, { once: true }));
    return (async () => {
      if (audio.paused) throw new Error('The recording is not playing');
      await nextUpdate();
      await frames();
      const before = read();
      let updates = 0;
      const count = () => { updates += 1; };
      audio.addEventListener('timeupdate', count);
      await nextUpdate();
      await frames();
      audio.removeEventListener('timeupdate', count);
      const after = read();
      const rendered = {};
      const found = {};
      for (const name of NAMES) {
        const then = before.byName.get(name);
        rendered[name] = 0;
        found[name] = 0;
        for (const [element, now] of after.byName.get(name)) {
          if (now === null) continue;
          found[name] += 1;
          const was = then.get(element);
          if (was === undefined || was === null || [...now].some((value) => !was.has(value))) rendered[name] += 1;
        }
      }
      return { rendered, found, updates, lineBefore: before.line, lineAfter: after.line, timeBefore: before.time, timeAfter: after.time };
    })();
  `);
}

/* ------------------------------------------------------------ level meters */

/**
 * Every live sound-level meter on the page (KNEG-371): where it sits, its
 * label on the Meetings page, what it shows (`data-level`, `data-lit`, and
 * the lit segments actually drawn), whether it is hidden from assistive
 * technology and has no `role`, its segments' transition, the lit and unlit
 * colours, and its box against the window and the button it sits in, if any.
 */
export function levelMeters() {
  return source(`
    const box = (element) => {
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height };
    };
    return [...document.querySelectorAll('.level-meter')].map((meter) => {
      const segment = meter.querySelector('.level-segment:not(.level-segment-lit)');
      const lit = meter.querySelector('.level-segment-lit');
      const host = meter.closest('button');
      return {
        className: meter.className,
        label: meter.closest('.live-level')?.querySelector('.live-level-label')?.textContent.trim() ?? null,
        level: Number(meter.dataset.level),
        lit: Number(meter.dataset.lit),
        litDrawn: meter.querySelectorAll('.level-segment-lit').length,
        segments: meter.querySelectorAll('.level-segment').length,
        ariaHidden: meter.getAttribute('aria-hidden'),
        hiddenFromAssistiveTechnology: meter.closest('[aria-hidden="true"]') !== null,
        role: meter.getAttribute('role'),
        transition: getComputedStyle(meter.querySelector('.level-segment')).transitionDuration,
        colours: {
          unlit: segment ? getComputedStyle(segment).backgroundColor : null,
          lit: lit ? getComputedStyle(lit).backgroundColor : null,
        },
        box: box(meter),
        host: host ? { className: host.className, box: box(host) } : null,
        viewport: { width: innerWidth, height: innerHeight },
      };
    });
  `);
}

/** One reading through the dev console's `kw.level`, as a capture's publisher sends one. */
export function emitLevel(level, session = "dictation", sourceName = "microphone") {
  return source(`
    if (!globalThis.kw?.level) throw new Error('The fixture dev console has no level');
    globalThis.kw.level(${JSON.stringify(level)}, ${JSON.stringify(session)}, ${JSON.stringify(sourceName)});
    return true;
  `);
}

/** The dev console's fake levels for `session` (`kw.levels`): ten a second, or stopped with a last 0. */
export function fakeLevels(session, running) {
  return source(`
    if (!globalThis.kw?.levels) throw new Error('The fixture dev console has no levels');
    globalThis.kw.levels(${JSON.stringify(session)}, ${JSON.stringify(running)});
    return true;
  `);
}

/**
 * Timestamps every change of what the first meter matching `selector` shows
 * (its `data-level`), from now until `levelChanges` reads them.
 */
export function watchLevelChanges(selector) {
  return source(`
    window.__levelWatch?.disconnect();
    const meter = document.querySelector(${JSON.stringify(selector)});
    if (!meter) throw new Error('No meter matches ' + ${JSON.stringify(selector)});
    const changes = window.__levelChanges = [];
    window.__levelWatch = new MutationObserver(() => changes.push({ t: performance.now(), level: meter.dataset.level }));
    window.__levelWatch.observe(meter, { attributes: true, attributeFilter: ['data-level'] });
    return true;
  `);
}

/** The changes `watchLevelChanges` saw, oldest first, and stops watching. */
export function levelChanges() {
  return source(`
    window.__levelWatch?.disconnect();
    return window.__levelChanges ?? [];
  `);
}

/* -------------------------------------------------------- meeting microphone */

/**
 * The Meetings microphone check (KNEG-364): its live region and the lines in
 * it (text and class), the meter beside them (hidden from assistive
 * technology, its level), every failure line in the control bar with what
 * its Details hold, and whether Start and Record this call are held.
 */
export function microphoneCheckState() {
  return source(`
    const text = (item) => item.textContent.trim();
    const region = document.querySelector('.control-body .microphone-check');
    const meter = region?.querySelector('.level-meter') ?? null;
    const start = document.querySelector('.control-start');
    const record = [...document.querySelectorAll('.call-card button')].find((item) => text(item) === 'Record this call');
    return {
      role: region?.getAttribute('role') ?? null,
      lines: region ? [...region.querySelectorAll('p')].map((item) => ({ text: text(item), className: item.className })) : [],
      meter: meter && {
        hiddenFromAssistiveTechnology: meter.closest('[aria-hidden="true"]') !== null,
        role: meter.getAttribute('role'),
        level: Number(meter.dataset.level),
        lit: Number(meter.dataset.lit),
        segments: meter.querySelectorAll('.level-segment').length,
      },
      failures: [...document.querySelectorAll('.control-body > .failure-line')].map((line) => ({
        role: line.getAttribute('role'),
        text: line.querySelector('p.error') ? text(line.querySelector('p.error')) : null,
        summary: line.querySelector('summary') ? text(line.querySelector('summary')) : null,
        details: [...line.querySelectorAll('.learn-more-body')].map(text),
      })),
      start: start ? { disabled: start.disabled } : null,
      record: record ? { disabled: record.disabled } : null,
    };
  `);
}

/** The live view's chips -- statements, not buttons -- with each one's label, hover and hidden text. */
export function liveChips() {
  return source(`
    return [...document.querySelectorAll('.meeting-live .live-chips .chip')].map((chip) => ({
      tag: chip.tagName.toLowerCase(),
      label: chip.querySelector('.chip-label')?.textContent ?? null,
      title: chip.getAttribute('title'),
      srText: chip.querySelector('.sr-only')?.textContent ?? null,
    }));
  `);
}

/* ------------------------------------------------------------ local models */

const LOCAL_SECTION = `document.querySelector('[data-surface="settings.transcription.local"]')`;

/**
 * Settings > Transcription's local models (KNEG-330): the section's headings
 * and lists, each row (its own line, detail lines, button and Details), the
 * language filter, the caveats and Learn more summaries, which of `ids` show
 * outside a closed Details, and whether every Details is closed. "Shown" is
 * every text node not inside a closed `details`' body.
 */
export function localModelsState(ids = []) {
  return source(`
    const section = ${LOCAL_SECTION};
    if (!section) return null;
    const text = (item) => item.textContent.trim();
    const own = (item) => [...item.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent).join('').trim();
    const shown = [];
    const walker = document.createTreeWalker(section, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const closed = node.parentElement?.closest('details:not([open])');
      if (closed && !node.parentElement.closest('summary')) continue;
      shown.push(node.textContent);
    }
    const shownText = shown.join(' ');
    const select = section.querySelector('select[aria-label="Show models for"]');
    const row = (label) => [...section.querySelectorAll(':scope > .row')]
      .find((item) => item.querySelector('.row-label')?.textContent.trim() === label);
    return {
      headings: [...section.querySelectorAll('h2, h3, h4')].map((item) => item.tagName.toLowerCase() + ':' + text(item)),
      lists: section.querySelectorAll('ul').length,
      rows: [...section.querySelectorAll('ul.list > li')].map((li) => ({
        line: own(li),
        details: [...li.querySelectorAll(':scope > em.row-detail')].map((item) => ({ text: text(item), warn: item.classList.contains('warn') })),
        button: li.querySelector(':scope > button') ? {
          text: text(li.querySelector(':scope > button')),
          pressed: li.querySelector(':scope > button').getAttribute('aria-pressed'),
          marked: li.querySelector(':scope > button').classList.contains('marked'),
        } : null,
        more: [...li.querySelectorAll('.learn-more-body p')].map(text),
      })),
      filter: select ? {
        value: select.value,
        shown: select.selectedOptions[0]?.textContent.trim() ?? null,
        options: [...select.options].map((option) => option.textContent.trim()),
      } : null,
      empty: [...section.querySelectorAll(':scope > p.muted')].map(text).filter((line) => line.startsWith('No model here')),
      caveats: [...section.querySelectorAll('.caveat')].map(text),
      summaries: [...section.querySelectorAll('details > summary')].map(text),
      openDetails: section.querySelectorAll('details[open]').length,
      idsShown: ${JSON.stringify(ids)}.filter((id) => shownText.includes(id)),
      idsAnywhere: ${JSON.stringify(ids)}.filter((id) => section.textContent.includes(id)),
      engines: ['Speech engine', 'Parakeet engine'].map((label) => row(label) ? label + ': ' + text(row(label).querySelector('.row-value')) : null),
    };
  `);
}

/** The value of Settings > Transcription's "Dictation goes to" row. */
export function dictationGoesTo() {
  return source(`
    const row = [...document.querySelectorAll('.row')]
      .find((item) => item.querySelector('.row-label')?.textContent.trim() === 'Dictation goes to');
    return row ? row.querySelector('.row-value').firstChild?.textContent.trim() ?? null : null;
  `);
}

/**
 * Scrolls the button of the local model row whose line starts with `name` into
 * the middle of the window, and gives its centre to press with a real pointer.
 */
export function localModelButton(name) {
  return source(`
    const wanted = ${JSON.stringify(name)};
    const li = [...${LOCAL_SECTION}.querySelectorAll('ul.list > li')].find((item) => {
      const line = [...item.childNodes].filter((node) => node.nodeType === 3).map((node) => node.textContent).join('').trim();
      return line === wanted || line.startsWith(wanted + ' · ');
    });
    const button = li?.querySelector(':scope > button');
    if (!button) throw new Error('No local model row has a button for ' + wanted);
    button.scrollIntoView({ block: 'center' });
    const rect = button.getBoundingClientRect();
    return { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2, text: button.textContent.trim() };
  `);
}

/**
 * Chooses `value` in the select named `label` as a person's choice reaches
 * React: through the element's own value setter, then a bubbling change.
 */
export function chooseSelect(label, value) {
  return source(`
    const select = document.querySelector('select[aria-label=' + JSON.stringify(${JSON.stringify(label)}) + ']');
    if (!select) throw new Error('No select is named ' + ${JSON.stringify(label)});
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, ${JSON.stringify(value)});
    select.dispatchEvent(new Event('change', { bubbles: true }));
    return select.value;
  `);
}

/** Opens (or closes) every Details in the local models section, for a measure that includes what they hold. */
export function openLocalDetails(open) {
  return source(`
    const all = [...${LOCAL_SECTION}.querySelectorAll('details')];
    for (const details of all) details.open = ${JSON.stringify(open)};
    return all.length;
  `);
}

/** How far the document, the card's `.split-main` and the local models section scroll sideways. */
export function localModelsOverflow() {
  return source(`
    const main = document.querySelector('.card .split-main');
    const section = ${LOCAL_SECTION};
    const wide = section ? [...section.querySelectorAll('*')]
      .filter((item) => item.getBoundingClientRect().right > (main?.getBoundingClientRect().right ?? innerWidth) + 0.5)
      .map((item) => item.tagName.toLowerCase() + '.' + item.className + ':' + item.textContent.trim().slice(0, 40)) : [];
    return {
      innerWidth,
      scrollWidth: document.scrollingElement.scrollWidth,
      mainClientWidth: main?.clientWidth ?? null,
      mainScrollWidth: main?.scrollWidth ?? null,
      sectionClientWidth: section?.clientWidth ?? null,
      sectionScrollWidth: section?.scrollWidth ?? null,
      pastTheCard: wide.slice(0, 5),
    };
  `);
}
