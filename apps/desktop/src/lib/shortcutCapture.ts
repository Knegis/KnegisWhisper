const modifiers: Record<string, string> = {
  ControlLeft: "LeftControl", ControlRight: "RightControl",
  ShiftLeft: "LeftShift", ShiftRight: "RightShift",
  AltLeft: "LeftAlt", AltRight: "RightAlt",
  MetaLeft: "LeftSuper", MetaRight: "RightSuper",
};
const ordinary = new Set([
  "Space", "Tab", "Enter", "Backspace", "Delete", "Insert", "Home", "End",
  "PageUp", "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
  "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon",
  "Quote", "Backquote", "Comma", "Period", "Slash", "CapsLock", "NumLock",
  "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal", "NumpadEnter",
]);

/** Record one simultaneous chord, preserving its fullest form until release. */
export class ShortcutCapture {
  private held = new Set<string>();
  private candidate = "";
  private invalid = false;
  constructor(private readonly sided: boolean) {}
  down(code: string): { preview: string; error?: string } {
    if (this.held.has(code)) return { preview: this.candidate };
    this.held.add(code);
    const tokens: string[] = [];
    let keys = 0;
    for (const key of this.held) {
      if (modifiers[key]) {
        const name = this.sided ? modifiers[key] : modifiers[key].replace(/^(Left|Right)/, "");
        if (!tokens.includes(name)) tokens.push(name);
      } else {
        keys++;
        if (/^Key[A-Z]$/.test(key)) tokens.push(key.slice(3));
        else if (/^Digit[0-9]$/.test(key)) tokens.push(key.slice(5));
        else if (/^(F([1-9]|1[0-9]|2[0-4])|Numpad[0-9]|Mouse[45])$/.test(key) || ordinary.has(key)) tokens.push(key);
        else this.invalid = true;
      }
    }
    if (keys > 1) this.invalid = true;
    const order = (name: string) => /Control/.test(name) ? 0 : /Alt/.test(name) ? 1 : /Shift/.test(name) ? 2 : /Super/.test(name) ? 3 : 4;
    tokens.sort((a, b) => order(a) - order(b));
    this.candidate = tokens.join("+");
    return { preview: this.candidate, ...(this.invalid ? { error: "Use modifiers with one key or mouse side button. This combination cannot be recorded." } : {}) };
  }
  up(code: string): string | null {
    if (!this.held.delete(code) || this.held.size) return null;
    const result = this.invalid ? null : this.candidate;
    this.invalid = false;
    this.candidate = "";
    return result;
  }
}
