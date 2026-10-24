import { useInsertionEffect, useRef, type RefObject } from "react";

/**
 * `value` as of the last commit, for the handlers, listeners and effects that read what is true
 * now without being made anew -- or run again -- when it changes (KNEG-455).
 *
 * It is written in an insertion effect, which React runs for the whole commit before any layout or
 * passive effect, wherever each is declared. So an effect reading it sees this commit's value in
 * whatever order the hooks holding them are called, and each hook can keep its own rather than all
 * of them sharing one that has to be declared first.
 */
export function useLatest<T>(value: T): RefObject<T> {
  const latest = useRef(value);
  useInsertionEffect(() => {
    latest.current = value;
  });
  return latest;
}
