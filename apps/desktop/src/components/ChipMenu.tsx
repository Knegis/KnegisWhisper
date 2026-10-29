/**
 * A chip that changes its value in place: the chip, and a menu it opens.
 *
 * The chip is drawn as `Chip` draws one -- icon, label, chevron, a pill --
 * so a strip of links, toggles and menus reads as one row. It is
 * named "<name>: <label>", the pattern every named chip follows, so the value
 * has something to be the value of and the words on screen are still inside
 * the name a speech user says (WCAG 2.5.3). `title` defaults to the label,
 * for when the chip's 260px cap has cut it short.
 *
 * Two kinds of menu:
 *
 *   - **Options** (`options` and `onChoose`): a `listbox` of `MenuOption`s
 *     (`lib/chipMenu.ts`), under group headings in the pixel voice where the
 *     options name groups. The value in effect is ticked and
 *     `aria-selected`. A greyed row takes focus and says `aria-disabled`, with
 *     its reason as its description, but cannot be chosen.
 *   - **Panel** (`children`): a non-modal `dialog` around whatever the caller
 *     puts in it, for a value that is more than one choice.
 *
 * Either can end in a `footer` row, for what the menu cannot do here -- "More
 * in Settings…".
 *
 * The keyboard is the select-only listbox's (WAI-ARIA APG): the chip opens on
 * Enter or Space, and a listbox's on an arrow too; the menu opens on the
 * value in effect, or a panel on its first control; the arrows and Home and
 * End move (`nextIndex`); Enter or Space chooses and closes; and Escape
 * closes. Both put focus back on the chip. Tab closes the menu as it leaves
 * -- a panel's, once Tab passes its last control -- and goes on from the
 * chip, so it lands where it would have without the menu.
 *
 * A press outside, the window losing focus, a resize, or a scroll anywhere
 * but in the menu closes it too, leaving focus where it went. A scroll and a
 * resize close it because it is placed against the chip as it opens, and
 * would otherwise be left floating where the chip was.
 *
 * The menu is portalled to `body`, because the control bar clips its own
 * corners, and placed by `placeMenu` (`lib/menuPlacement.ts`): above the
 * chip wherever it fits there, so it does not cover the bar. Its position is
 * a `style` prop, which the `style-src 'self'` CSP allows; it is not a
 * stylesheet. While it is open, `Tooltips` draws nothing over it.
 *
 * `busy` is what the chip says while a choice is being applied ("Starting
 * large-v3…"). The chip cannot be opened then. It is `aria-disabled` rather
 * than `disabled`, because a disabled button drops the focus a keyboard user
 * just brought back to it by choosing.
 *
 * `held` is why the value cannot be changed right now ("Can't change during a
 * dictation"). The chip keeps saying the value in effect -- which is the point
 * while it is in effect -- and cannot be opened, as when busy; the reason is
 * its `title` and its `aria-description`.
 *
 * `open()` (the ref's handle) opens the menu from elsewhere -- a setup button
 * in a page's hero that is asking for this very value. Focus goes into the
 * menu, and returns to the chip, as it would had the chip been pressed.
 */
import {
  forwardRef,
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { chosenOption, menuGroups, nextIndex, openIndex, type MenuOption } from "../lib/chipMenu";
import type { IconName } from "../lib/icons";
import { placeMenu, type MenuPlacement } from "../lib/menuPlacement";
import { Icon } from "./Icon";

export type ChipMenuHandle = { open: () => void };

export type MenuFooter = { label: string; onClick: () => void };

type ChipMenuCommon = {
  icon: IconName;
  /** What the value is the value of: "Microphone". Also the menu's own name. */
  name: string;
  /** The value in effect, on the chip. */
  label: string;
  title?: string;
  /** While set, the chip says this instead of the label and cannot be opened. */
  busy?: string | null;
  /** While set, the chip keeps its label but cannot be opened, and this says why. */
  held?: string | null;
  footer?: MenuFooter;
};

export type ChipMenuProps = ChipMenuCommon &
  (
    | { options: readonly MenuOption[]; onChoose: (id: string) => void; children?: never }
    | { children: ReactNode; options?: never; onChoose?: never }
  );

/** The rows the arrows walk: every option, then the footer. */
const ROW = "[data-menu-row]";
/** What Tab stops at inside a panel. */
const TABBABLE =
  'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])';

export const ChipMenu = forwardRef<ChipMenuHandle, ChipMenuProps>(function ChipMenu(props, ref) {
  const { icon, name, label, title, busy = null, held = null, footer } = props;
  const options = props.options;
  const panel = options === undefined;
  const id = useId();
  const popupId = `${id}menu`;
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  /** What scrolls when the menu is taller than its room -- the list, or a panel's body -- while the footer stays. */
  const scroller = useRef<HTMLDivElement>(null);
  /** Whether the opening focus has been placed, so a re-render does not move it back. */
  const focused = useRef(false);
  const [open, setOpen] = useState(false);
  const [placement, setPlacement] = useState<MenuPlacement | null>(null);
  const shown = busy ?? label;
  /** Busy or held: either way it cannot be opened. */
  const blocked = busy !== null || held !== null;

  const show = useCallback(() => {
    if (!blocked) setOpen(true);
  }, [blocked]);
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    setPlacement(null);
    focused.current = false;
    if (refocus) trigger.current?.focus();
  }, []);

  useImperativeHandle(ref, () => ({ open: show }), [show]);

  // A choice being applied closes the menu it was made in, if something else has not;
  // and a value that stops being changeable closes its menu too.
  useLayoutEffect(() => {
    if (blocked) close(false);
  }, [blocked, close]);

  // Placed against the chip once it has been laid out hidden at its own size, and again
  // whenever what it holds changes while it is open. The natural height is read past any
  // `maxHeight` already applied, so a list that was cut short can grow back.
  useLayoutEffect(() => {
    const box = popup.current;
    const chip = trigger.current;
    const scroll = scroller.current;
    if (!open || !box || !chip || !scroll) return;
    const size = {
      width: box.getBoundingClientRect().width,
      height: box.offsetHeight + scroll.scrollHeight - scroll.clientHeight,
    };
    const placed = placeMenu(chip.getBoundingClientRect(), size, { width: window.innerWidth, height: window.innerHeight });
    // Snapped to device pixels, as the tooltip is: a hairline border on a fraction of a pixel blurs.
    const snap = (value: number) => Math.round(value * window.devicePixelRatio) / window.devicePixelRatio;
    const next = { ...placed, left: snap(placed.left), top: snap(placed.top), maxHeight: Math.floor(placed.maxHeight) };
    setPlacement((prev) =>
      prev !== null && prev.left === next.left && prev.top === next.top && prev.side === next.side && prev.maxHeight === next.maxHeight
        ? prev
        : next,
    );
  }, [open, options, props.children, footer]);

  // Focus goes in once the menu is visible: a `visibility: hidden` element cannot take it.
  useLayoutEffect(() => {
    const box = popup.current;
    if (!open || placement === null || !box || focused.current) return;
    focused.current = true;
    const first = panel ? box.querySelector<HTMLElement>(TABBABLE) : box.querySelectorAll<HTMLElement>(ROW)[openIndex(options)];
    (first ?? box).focus();
  }, [open, placement, panel, options]);

  useEffect(() => {
    if (!open) return;
    const inside = (target: EventTarget | null) =>
      target instanceof Node && (popup.current?.contains(target) === true || trigger.current?.contains(target) === true);
    // The chip is left to its own click, which closes what it opened.
    const pressed = (event: PointerEvent) => {
      if (!inside(event.target)) close(false);
    };
    const scrolled = (event: Event) => {
      if (!(event.target instanceof Node && popup.current?.contains(event.target))) close(false);
    };
    const dismiss = () => close(false);
    document.addEventListener("pointerdown", pressed, true);
    document.addEventListener("scroll", scrolled, true);
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    return () => {
      document.removeEventListener("pointerdown", pressed, true);
      document.removeEventListener("scroll", scrolled, true);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
    };
  }, [open, close]);

  const choose = (index: number) => {
    if (options === undefined || props.onChoose === undefined) return;
    const option = chosenOption(options, index);
    if (option === null) return;
    close(true);
    props.onChoose(option.id);
  };

  const onPopupKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const box = popup.current;
    if (!box) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    if (event.key === "Tab") {
      if (panel) {
        const stops = [...box.querySelectorAll<HTMLElement>(TABBABLE)];
        const active = document.activeElement;
        const leaving =
          stops.length === 0 ||
          (event.shiftKey ? active === stops[0] || active === box : active === stops[stops.length - 1]);
        if (!leaving) return;
      }
      // Not prevented: with focus back on the chip, Tab goes on from there.
      close(true);
      return;
    }
    if (panel) return;
    const rows = [...box.querySelectorAll<HTMLElement>(ROW)];
    const from = rows.findIndex((row) => row === document.activeElement);
    const to = nextIndex(options, from, event.key, footer !== undefined);
    if (to !== null) {
      event.preventDefault();
      rows[to]?.focus();
      return;
    }
    // The footer is a button, and Enter and Space press it as they would any other.
    if ((event.key === "Enter" || event.key === " ") && from >= 0 && from < options.length) {
      event.preventDefault();
      choose(from);
    }
  };

  const optionRow = ({ option, index }: { option: MenuOption; index: number }) => {
    const nameId = `${id}o${index}`;
    const described = [
      option.detail !== undefined ? `${nameId}d` : null,
      option.reason !== null ? `${nameId}r` : null,
    ].filter((part) => part !== null);
    return (
      <div
        key={option.id}
        role="option"
        className="chip-menu-option"
        tabIndex={-1}
        data-menu-row=""
        aria-selected={option.current}
        aria-disabled={option.disabled || undefined}
        aria-labelledby={nameId}
        aria-describedby={described.length > 0 ? described.join(" ") : undefined}
        onClick={() => choose(index)}
      >
        <span className="chip-menu-check">{option.current ? <Icon name="check" size={16} /> : null}</span>
        <span className="chip-menu-text">
          <span id={nameId}>{option.label}</span>
          {option.detail !== undefined ? (
            <span id={`${nameId}d`} className="chip-menu-detail">
              {option.detail}
            </span>
          ) : null}
        </span>
        {option.reason !== null ? (
          <span id={`${nameId}r`} className="chip-menu-reason">
            {option.reason}
          </span>
        ) : null}
      </div>
    );
  };

  const footerRow =
    footer === undefined ? null : (
      <button
        type="button"
        className="chip-menu-footer"
        data-menu-row=""
        tabIndex={panel ? undefined : -1}
        onClick={() => {
          close(true);
          footer.onClick();
        }}
      >
        {footer.label}
      </button>
    );

  const position = placement
    ? { left: placement.left, top: placement.top, maxHeight: placement.maxHeight }
    : { left: 0, top: 0, visibility: "hidden" as const };

  const menu = !open ? null : options === undefined ? (
    <div
      ref={popup}
      id={popupId}
      role="dialog"
      aria-label={name}
      tabIndex={-1}
      className="chip-menu"
      data-side={placement?.side}
      style={position}
      onKeyDown={onPopupKey}
    >
      <div ref={scroller} className="chip-menu-body">
        {props.children}
      </div>
      {footerRow}
    </div>
  ) : (
    <div ref={popup} className="chip-menu" tabIndex={-1} data-side={placement?.side} style={position} onKeyDown={onPopupKey}>
      <div ref={scroller} id={popupId} role="listbox" aria-label={name} className="chip-menu-list">
        {menuGroups(options).map((run, r) =>
          run.group === null ? (
            run.items.map(optionRow)
          ) : (
            <div key={`group${r}`} role="group" aria-labelledby={`${id}g${r}`}>
              <div id={`${id}g${r}`} role="presentation" className="chip-menu-label">
                {run.group}
              </div>
              {run.items.map(optionRow)}
            </div>
          ),
        )}
      </div>
      {footerRow}
    </div>
  );

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className="chip"
        aria-haspopup={panel ? "dialog" : "listbox"}
        aria-expanded={open}
        aria-controls={open ? popupId : undefined}
        aria-label={`${name}: ${shown}`}
        aria-disabled={blocked || undefined}
        aria-description={held ?? undefined}
        title={held ?? title ?? shown}
        onClick={() => (open ? close(true) : show())}
        onKeyDown={(event) => {
          if (!panel && !open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
            event.preventDefault();
            show();
          }
        }}
      >
        <Icon name={icon} size={16} />
        <span className="chip-label">{shown}</span>
        <span className="chip-chevron">
          <Icon name="chevronDown" size={14} />
        </span>
      </button>
      {menu === null ? null : createPortal(menu, document.body)}
    </>
  );
});
