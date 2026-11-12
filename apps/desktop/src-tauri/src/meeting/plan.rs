//! Captured meeting plan and stable titles.
use super::*;

/// How long a meeting's name may be, in characters.
pub(super) const TITLE_LIMIT: usize = 200;

/// A name typed before Start, tidied into a title: one line, trimmed, at most
/// [`TITLE_LIMIT`] characters. `None` when nothing is left of it.
pub(super) fn clean_title(given: &str) -> Option<String> {
    let line = given
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let capped: String = line.chars().take(TITLE_LIMIT).collect();
    let title = capped.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

/// A meeting note's title: the name it was given, or when it **started**
/// (KNEG-365). It used to be the time it was saved, so a meeting stopped
/// after midnight, or recovered a day later, was named for the wrong day.
pub(super) fn note_title(given: Option<&str>, started: chrono::DateTime<chrono::Local>) -> String {
    given
        .and_then(clean_title)
        .unwrap_or_else(|| format!("Meeting — {}", started.format("%Y-%m-%d %H:%M")))
}

/// The title of the notes a discarded meeting keeps: its name, or the wording
/// that says only notes are inside.
pub(super) fn notes_only_title(
    given: Option<&str>,
    started: chrono::DateTime<chrono::Local>,
) -> String {
    given
        .and_then(clean_title)
        .unwrap_or_else(|| format!("Meeting notes — {}", started.format("%Y-%m-%d %H:%M")))
}

/// When a recording started, from the name `start_live` gave it:
/// `%Y%m%d-%H%M%S` in UTC, then a UUID (none before the UUID was added).
/// `None` for any other name.
pub(super) fn recording_started_at(storage_key: &str) -> Option<chrono::DateTime<chrono::Local>> {
    let stamp = storage_key.get(..15)?;
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S")
        .ok()
        .map(|utc| utc.and_utc().with_timezone(&chrono::Local))
}

/// Where a meeting starting `now` is recorded, in `directory`, and when it
/// started on the clock: both from the one instant, so the start a Stop note
/// is named for and the one recovery reads back from the file name
/// ([`recording_started_at`]) are the same minute.
pub(super) fn recording_start(
    directory: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
    id: uuid::Uuid,
) -> (std::path::PathBuf, chrono::DateTime<chrono::Local>) {
    let name = format!("{}-{id}.wav", now.format("%Y%m%d-%H%M%S"));
    (directory.join(name), now.with_timezone(&chrono::Local))
}

/// The plan a meeting starts with. The name is tidied once, here, so the live
/// title, the plan beside the recording and the saved note all say the same
/// name. Whether the recording is deleted once saved is read from the setting
/// now (KNEG-409), never taken from the caller, and a change to the setting
/// after this is the next meeting's.
pub(super) async fn plan_at_start(
    db: &Db,
    plan: MeetingPlan,
    provider_id: &str,
    choice: CalendarLinkChoice,
) -> calendar_link::StartPlan {
    let plan = MeetingPlan {
        title: plan.title.as_deref().and_then(clean_title),
        folder_id: plan.folder_id,
        delete_recording: delete_recordings(db).await,
        live_provider_id: Some(provider_id.to_owned()),
        calendar: None,
    };
    calendar_link::resolve(db, plan, choice, chrono::Utc::now()).await
}
