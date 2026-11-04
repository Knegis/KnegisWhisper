/** Trim and collapse inner whitespace, the same normalisation the backend applies, so the Save button's "unchanged" test agrees with what would be stored. */
export function normaliseSpeakerName(raw: string): string {
  return raw.trim().replace(/\s+/g, " ");
}

/** The form's heading for a speaker currently shown as `current`. */
export function renameHeading(current: string): string {
  return `Name for ${current}`;
}

/** Suggestions worth showing for `current`: the list minus the current display name itself (case-insensitive), in order. */
export function suggestionsFor(current: string, suggestions: readonly string[]): string[] {
  const currentLower = current.toLocaleLowerCase();
  return suggestions.filter((name) => name.toLocaleLowerCase() !== currentLower);
}

/**
 * What a speaker shows with nobody having named them, as Rust resolves it (`speaker_name`,
 * crates/notes-core/src/transcript.rs): `speaker_0` is "Speaker 1", anything else its own label.
 */
export function defaultSpeakerName(speakerId: string): string {
  const number = /^speaker_(\d+)$/.exec(speakerId)?.[1];
  return number === undefined ? speakerId : `Speaker ${Number(number) + 1}`;
}

/**
 * Whether somebody named this speaker. Rust sends only the name to show, so this asks whether it is
 * the one a speaker has without a name. Someone named "Speaker 1" while they are the first speaker
 * reads as unnamed, and removing that name would change nothing anyway.
 */
export function speakerIsNamed(current: string, speakerId: string): boolean {
  return current !== defaultSpeakerName(speakerId);
}

/** Whether Save would change anything. Cleared, it removes a name; with no name, there is none to remove. */
export function canSaveSpeakerName(value: string, target: { current: string; speakerId: string }): boolean {
  const name = normaliseSpeakerName(value);
  if (name === normaliseSpeakerName(target.current)) return false;
  return name !== "" || speakerIsNamed(target.current, target.speakerId);
}

/**
 * The suggestion pills for what is in the field. Typing narrows them, but a name that is already
 * whole doesn't: the speaker's own, as the form opens, or a suggestion just pressed, so the row
 * stays where it is and another can be pressed instead. The cap keeps the form to a short row in a
 * narrow column, and typing still reaches a name past it.
 */
export function matchingSuggestions(current: string, typed: string, suggestions: readonly string[], limit = 8): string[] {
  const offered = suggestionsFor(current, suggestions);
  const wanted = normaliseSpeakerName(typed).toLocaleLowerCase();
  const exact = offered.find((name) => name.toLocaleLowerCase() === wanted);
  const whole = exact !== undefined || wanted === current.toLocaleLowerCase();
  const shown = wanted === "" || whole ? offered : offered.filter((name) => name.toLocaleLowerCase().includes(wanted));
  const first = shown.slice(0, limit);
  // The name in the field keeps its pill, even from past the cap.
  return exact === undefined || first.includes(exact) ? first : [...first.slice(0, limit - 1), exact];
}
