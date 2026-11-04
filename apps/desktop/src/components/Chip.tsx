import type { IconName } from "../lib/icons";
import { Icon } from "./Icon";

/**
 * One control in a task page's control bar: an icon, a label, and an optional
 * chevron.
 *
 * A chip is one of three things:
 *
 *   - **A link**, with `onClick`: a way to the settings screen that owns a
 *     value the page does not write, which says so by going there on click
 *     (the redesign's decision 4). It takes the `chevron`.
 *   - **A toggle**, with `pressed`, where the page writes an on-or-off value
 *     itself -- Meetings' "Other participants" (kept on the page only),
 *     "Summary" and "Speaker labels", Dictation's "Tidy up" (all written once
 *     they have been set up in Settings; until then each is a link) -- and so
 *     gets `aria-pressed`.
 *   - **Static**, with neither: not a control at all but a statement of what
 *     is in effect -- every chip in Meetings' live view, where nothing can be
 *     changed until the meeting ends, the microphone among them: the one the
 *     meeting opened (KNEG-364). That one is a `span.chip.chip-static`: drawn
 *     like the others so the strip reads as one row, but not a button, so it
 *     is not in the tab order and does not offer a press that does nothing.
 *
 * A chip that chooses among values in place is not this component but
 * `ChipMenu`, which draws the same chip and opens a menu from it -- since
 * KNEG-385, Dictation's microphone, which was a native select styled as a
 * chip before that, and since KNEG-364 Meetings' microphone before a meeting
 * starts.
 *
 * The label ellipsises inside the chip's 260px cap, so `title` defaults to the
 * whole label: "Microphone Array (Realtek(R) Audio)" still reads in full under
 * the pointer at the 720px minimum window. A caller with something better to
 * say there -- Meetings' reason the toggle is off -- passes its own, on a
 * static chip as on a button.
 *
 * A `title` is not something everyone can reach -- a static chip is not
 * focusable, and no screen reader is obliged to read it -- so two props say
 * the rest in the chip itself:
 *
 *   - `name` is a button chip's accessible name, where the label alone is a
 *     value with nothing to say what it is the value of: "OpenAI" is
 *     "Transcription: OpenAI". The name keeps the label whole inside it, so
 *     the words on screen are still words a speech user can say to press it
 *     (WCAG 2.5.3). A static chip takes none, because a `span` cannot be
 *     named.
 *   - `held` is why a button chip cannot be pressed right now ("Can't change
 *     during a dictation"). It is `aria-disabled` rather than `disabled`, as
 *     `ChipMenu`'s is, so a keyboard user on it keeps focus when the hold
 *     starts; the reason is its `aria-description` and its `title`, and a
 *     press does nothing. `disabled` stays for a chip that is simply
 *     unavailable.
 *   - `srText` is visually hidden text after the label, for what `title`
 *     explains -- the live meeting's microphone, that it is the one the
 *     meeting opened and a choice made now waits for the next recording.
 */
export function Chip({
  icon,
  label,
  onClick,
  pressed,
  chevron = false,
  disabled = false,
  held = null,
  title,
  name,
  srText,
}: {
  icon: IconName;
  label: string;
  onClick?: () => void;
  pressed?: boolean;
  chevron?: boolean;
  disabled?: boolean;
  held?: string | null;
  title?: string;
  name?: string;
  srText?: string;
}) {
  const content = (
    <>
      <Icon name={icon} size={18} />
      <span className="chip-label">{label}</span>
      {srText === undefined ? null : <span className="sr-only">{`. ${srText}`}</span>}
      {chevron ? <span className="chip-chevron"><Icon name="chevronDown" size={24} /></span> : null}
    </>
  );
  if (onClick === undefined && pressed === undefined) {
    return <span className="chip chip-static" title={title ?? label}>{content}</span>;
  }
  return (
    <button
      type="button"
      className="chip"
      onClick={held === null ? onClick : undefined}
      aria-pressed={pressed}
      aria-label={name}
      aria-disabled={held !== null ? true : undefined}
      aria-description={held ?? undefined}
      disabled={disabled}
      title={held ?? title ?? label}
    >
      {content}
    </button>
  );
}
