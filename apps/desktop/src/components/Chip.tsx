import type { IconName } from "../lib/icons";
import { Icon } from "./Icon";

/**
 * One control in a task page's control bar: an icon, a label, and an optional
 * chevron.
 *
 * A chip is a real control only where the page itself writes the value it
 * shows -- Meetings' "Other participants" toggle, which takes `pressed` and so
 * gets `aria-pressed`. Every other chip is a way to the settings screen that
 * owns the value, and says so by going there on click (the redesign's
 * decision 4). Dictation's microphone is the one real select, and it uses
 * `className="chip"` on a `<select>` rather than this component, because a
 * select already is the control.
 *
 * The label ellipsises inside the chip's 260px cap, so `title` defaults to the
 * whole label: "Microphone Array (Realtek(R) Audio)" still reads in full under
 * the pointer at the 720px minimum window. A caller with something better to
 * say there -- Meetings' reason the toggle is off -- passes its own.
 */
export function Chip({
  icon,
  label,
  onClick,
  pressed,
  chevron = false,
  disabled = false,
  title,
}: {
  icon: IconName;
  label: string;
  onClick?: () => void;
  pressed?: boolean;
  chevron?: boolean;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      className="chip"
      onClick={onClick}
      aria-pressed={pressed}
      disabled={disabled}
      title={title ?? label}
    >
      <Icon name={icon} size={16} />
      <span className="chip-label">{label}</span>
      {chevron ? <span className="chip-chevron"><Icon name="chevronDown" size={14} /></span> : null}
    </button>
  );
}
