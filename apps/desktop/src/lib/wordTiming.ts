/**
 * Which word of a playing transcript row is being said (KNEG-408).
 *
 * Local Whisper times each word of a row, and the note saves those times
 * (`TranscriptRow.words`). A row with them highlights word by word as its
 * recording plays; every other row -- notes from before, and every other
 * engine -- highlights as a row, as it always has.
 */
import type { Word } from "../generated/ipc-types.js";

/**
 * The index of the word being said at `seconds`: the last word that has
 * started. It stays the current one until the next word starts, through any
 * pause after it, so the highlight never blinks off between two words. `null`
 * before the first word, for no words, and for a time that is not a number.
 *
 * A binary search, because it runs on every animation frame while a row
 * plays, and an hour's row can be long.
 */
export function wordAt(words: readonly Word[], seconds: number): number | null {
  if (!Number.isFinite(seconds) || words.length === 0) return null;
  let low = 0;
  let high = words.length - 1;
  let found: number | null = null;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if ((words[middle]?.start ?? Number.POSITIVE_INFINITY) <= seconds) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}

/**
 * Whether `words` spell `text`, whitespace aside -- whether they can be shown
 * in its place. Rust already gives a row no words unless they do; this keeps
 * the screen from drawing words a row's text no longer has whatever sent the
 * row. False when there are no words to show.
 */
export function wordsMatchText(words: readonly Word[], text: string): boolean {
  if (words.length === 0) return false;
  const squeeze = (value: string) => value.replace(/\s+/gu, "");
  return squeeze(words.map((word) => word.text).join("")) === squeeze(text);
}
