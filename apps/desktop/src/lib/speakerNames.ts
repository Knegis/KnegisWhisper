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
