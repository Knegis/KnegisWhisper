/**
 * What the chosen meeting provider will actually do with a language and a set
 * of keyterms, asked of Rust as they change.
 *
 * Shared by the two meeting screens, and lifted out of `SettingsMeetings.tsx`
 * when the recording moved to its own page, because both need the same answer
 * from the same call: Settings > Meetings shows it beside the fields as they
 * are edited, and the Meetings page reads it to decide whether it can offer to
 * start at all (`meetingReadiness` in `lib/readiness.ts`). Two copies of this
 * effect would be two chances to preview a different configuration from the
 * one `start` is about to use.
 *
 * `kw_meeting_core::validate` does the arithmetic -- see `reviewMeetingConfig`
 * -- so nothing here knows a provider's cap or whether it detects a language
 * on its own.
 */

import { useEffect, useState } from "react";

import { reviewMeetingConfig, type ConfigReview } from "./ipc.js";

/**
 * One entry per line, trimmed, with blank lines dropped.
 *
 * Shared by the keyterms sent for review below and by both detection lists on
 * Settings > Meetings, because all three are the same thing and are normalised
 * the same way in Rust: keyterms by `normalize_keyterms`, the lists by
 * `kw_platform::meeting_detect::detect`, which trims every entry and skips the
 * empty ones. One parser rather than three is what keeps a stray blank line
 * from meaning something different in one of them.
 */
export function onePerLine(text: string): string[] {
  return text
    .split("\n")
    .map((k) => k.trim())
    .filter((k) => k.length > 0);
}

/**
 * The review for `provider`, or `null` while there is no provider, before the
 * first answer, or when the call failed.
 *
 * `keytermsText` is one term per line rather than an array so the effect's
 * dependency is a string, compared by value: an array rebuilt on every render
 * would re-ask Rust on every render.
 */
export function useMeetingReview(
  provider: string | null,
  language: string,
  keytermsText: string,
): ConfigReview | null {
  const [review, setReview] = useState<ConfigReview | null>(null);

  useEffect(() => {
    if (!provider) {
      setReview(null);
      return;
    }
    let cancelled = false;
    void reviewMeetingConfig(
      provider,
      language,
      onePerLine(keytermsText),
    )
      .then((result) => {
        if (!cancelled) setReview(result);
      })
      .catch(() => {
        if (!cancelled) setReview(null);
      });
    return () => {
      cancelled = true;
    };
  }, [provider, language, keytermsText]);

  return review;
}
