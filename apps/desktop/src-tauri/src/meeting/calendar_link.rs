//! Local calendar snapshots for Start and live edits. No provider runtime or
//! credential store is available here: every lookup reads the gated database
//! snapshot, and recovery needs only the plan already beside the recording.
use super::*;
use crate::calendar::{calendar_events_at, CalendarEventAt, CalendarProvider, CandidateEvent};
use kw_calendar_core::events_repo::EventRow;
use kw_notes_core::people::{self, Participant};
use std::{collections::HashSet, path::Path};

/// Retain usable invitees in provider order. Email identity is normalised before
/// excluding self and duplicates; malformed addresses can still contribute names.
/// Count omissions after filtering and deduplication so the count describes people
/// that could have been kept had the note's participant limit been larger.
pub fn participants(row: &EventRow, account_email: Option<&str>) -> (Vec<Participant>, u32) {
    let own = account_email.and_then(people::normalise_email);
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    let mut omitted = 0u32;
    for attendee in &row.attendees {
        if attendee.response_status.as_deref() == Some("declined") {
            continue;
        }
        let email = attendee.email.as_deref().and_then(people::normalise_email);
        let display_name = attendee
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        if let Some(email) = &email {
            if own.as_ref() == Some(email) || !seen.insert(email.clone()) {
                continue;
            }
        } else if display_name.is_none() {
            continue;
        }
        if kept.len() == people::MAX_PARTICIPANTS {
            omitted = omitted.saturating_add(1);
        } else {
            kept.push(Participant {
                display_name,
                email,
            });
        }
    }
    (kept, omitted)
}

pub(super) struct StartPlan {
    pub plan: MeetingPlan,
    pub notice: Option<CalendarLinkNotice>,
    pub choices: Vec<CandidateEvent>,
}

fn candidate(event: CalendarEventAt) -> CandidateEvent {
    CandidateEvent {
        id: event.row.id,
        provider: event.provider,
        calendar_id: event.row.calendar_id,
        title: event.row.summary,
        start: event.row.start_time,
        end: event.row.end_time,
        meeting_like: event.meeting_like,
    }
}

/// A typed title wins until the link itself says the title came from an event.
/// In particular an untitled replacement event clears the old event's title.
async fn link(db: &Db, plan: &mut MeetingPlan, event: CalendarEventAt) -> Result<(), String> {
    let key = match event.provider {
        CalendarProvider::Google => "calendar.google.account",
        CalendarProvider::Microsoft => "calendar.microsoft.account",
    };
    let account = kw_storage::settings::get::<String>(db.pool(), key)
        .await
        .map_err(|_| "could not read the calendar account".to_owned())?;
    let (participants, omitted_participants) = participants(&event.row, account.as_deref());
    let title = event.row.summary.as_deref().and_then(clean_title);
    let title_from_event = plan.title.as_deref().and_then(clean_title).is_none()
        || plan
            .calendar
            .as_ref()
            .is_some_and(|event| event.title_from_event);
    if title_from_event {
        plan.title.clone_from(&title);
    }
    plan.calendar = Some(LinkedEvent {
        event_id: event.row.id,
        provider: event.provider,
        title,
        title_from_event,
        participants,
        omitted_participants,
    });
    Ok(())
}

pub(super) async fn resolve(
    db: &Db,
    plan: MeetingPlan,
    choice: CalendarLinkChoice,
    at: chrono::DateTime<chrono::Utc>,
) -> StartPlan {
    let mut result = StartPlan {
        plan,
        notice: None,
        choices: Vec::new(),
    };
    if matches!(choice, CalendarLinkChoice::None) {
        return result;
    }
    let events = match calendar_events_at(db, at).await {
        Ok(events) => events,
        Err(_) => {
            // Never log event content or a provider's raw error body.
            tracing::warn!("could not read the local calendar snapshot at meeting Start");
            result.notice = Some(CalendarLinkNotice::CalendarUnreadable);
            return result;
        }
    };
    let selected = match choice {
        CalendarLinkChoice::None => None,
        CalendarLinkChoice::Event { event_id } => {
            let event = events.into_iter().find(|event| event.row.id == event_id);
            if event.is_none() {
                result.notice = Some(CalendarLinkNotice::ChoiceUnavailable);
            }
            event
        }
        CalendarLinkChoice::Auto => {
            let mut matches: Vec<_> = events
                .into_iter()
                .filter(|event| event.meeting_like)
                .collect();
            match matches.len() {
                0 => None,
                1 => matches.pop(),
                _ => {
                    result.notice = Some(CalendarLinkNotice::Overlap);
                    result.choices = matches.into_iter().map(candidate).collect();
                    None
                }
            }
        }
    };
    if let Some(event) = selected {
        if link(db, &mut result.plan, event).await.is_err() {
            tracing::warn!("could not snapshot local calendar participants at meeting Start");
            result.notice = Some(CalendarLinkNotice::CalendarUnreadable);
        }
    }
    result
}

pub(super) fn session_id(path: &Path) -> Option<String> {
    path.file_stem()?.to_str().map(str::to_owned)
}

/// Status exposes an editable identity only while the recording owns the live
/// slot. Start and finalisation cannot advertise a session that accepts edits.
pub(super) fn running_session_id(phase: SessionPhase, path: Option<&Path>) -> Option<String> {
    if phase != SessionPhase::Recording {
        return None;
    }
    path.and_then(session_id)
}

/// Commit disk before memory, then notify. Keeping this order in one helper lets
/// failure tests verify that both windows keep seeing the previous durable link.
async fn edit(
    db: &Db,
    path: &Path,
    requested_session: &str,
    choice: CalendarSetChoice,
    plan: &mut MeetingPlan,
    notice: &mut Option<CalendarLinkNotice>,
    changed: impl FnOnce(),
) -> Result<(), String> {
    let stem = session_id(path).ok_or("meeting session unavailable")?;
    if stem != requested_session {
        return Err("the meeting session is no longer running".into());
    }
    let mut next = plan.clone();
    match choice {
        CalendarSetChoice::None => {
            if next
                .calendar
                .as_ref()
                .is_some_and(|event| event.title_from_event)
            {
                next.title = None;
            }
            next.calendar = None;
        }
        CalendarSetChoice::Event { event_id } => {
            let at = recording_started_at(&stem).ok_or("meeting start time unavailable")?;
            let event = calendar_events_at(db, at.with_timezone(&chrono::Utc))
                .await?
                .into_iter()
                .find(|event| event.row.id == event_id)
                .ok_or("the calendar event is unavailable at this meeting's start")?;
            link(db, &mut next, event).await?;
        }
    }
    draft::write_plan(path, &next)?;
    *plan = next;
    *notice = None;
    changed();
    Ok(())
}

impl MeetingRuntime {
    /// Serialize edits with Start and Stop so a late click cannot change the plan
    /// of a new recording or a plan already being consumed by finalisation.
    pub async fn set_calendar(
        &self,
        session: &str,
        choice: CalendarSetChoice,
    ) -> Result<(), String> {
        let lifecycle = self.lifecycle.lock().await;
        if lifecycle.phase != SessionPhase::Recording {
            return Err("the meeting session is no longer running".into());
        }
        let mut held = self.live.lock().await;
        let live = held
            .as_mut()
            .ok_or("the meeting session is no longer running")?;
        edit(
            &self.db,
            &live.recording_path,
            session,
            choice,
            &mut live.plan,
            &mut live.calendar_notice,
            || {
                let _ = self.app.emit(PLAN_CHANGED_EVENT, ());
            },
        )
        .await
    }
}

/// A Start write is best effort for recording, but mandatory for linking. Stop
/// must not save a link that crash recovery could never discover on disk.
pub(super) fn persist_start(
    path: &Path,
    plan: &mut MeetingPlan,
    notice: &mut Option<CalendarLinkNotice>,
) {
    if let Err(error) = draft::write_plan(path, plan) {
        plan.calendar = None;
        *notice = Some(CalendarLinkNotice::NotSaved);
        tracing::warn!(%error, path = %draft::plan_path(path).display(),
            "could not write the meeting's plan beside its recording");
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use kw_calendar_core::{events_repo, sync_support::StoredAttendee};
    use kw_storage::settings;
    use std::cell::Cell;

    #[test]
    fn status_session_identity_exists_only_between_start_and_stop() {
        let mut lifecycle = SessionLifecycle::default();
        let path = Path::new("20261111-120000-fixture.wav");
        assert_eq!(running_session_id(lifecycle.phase, None), None);
        let generation = lifecycle.begin_start().unwrap();
        assert_eq!(running_session_id(lifecycle.phase, Some(path)), None);
        assert!(lifecycle.started(generation));
        assert_eq!(
            running_session_id(lifecycle.phase, Some(path)).as_deref(),
            Some("20261111-120000-fixture")
        );
        assert_eq!(running_session_id(lifecycle.phase, None), None);
        assert_eq!(lifecycle.begin_finish().unwrap(), generation);
        assert_eq!(running_session_id(lifecycle.phase, Some(path)), None);
        assert!(lifecycle.finished(generation));
        assert_eq!(running_session_id(lifecycle.phase, None), None);
    }

    fn at() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-11-11T12:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn attendee(email: Option<&str>, name: Option<&str>, status: Option<&str>) -> StoredAttendee {
        StoredAttendee {
            email: email.map(str::to_owned),
            name: name.map(str::to_owned),
            response_status: status.map(str::to_owned),
        }
    }

    fn row(id: &str, title: Option<&str>, solo: bool) -> EventRow {
        EventRow {
            id: format!("google:cal:{id}"),
            calendar_id: "cal".into(),
            provider: "google",
            summary: title.map(str::to_owned),
            start_time: at().to_rfc3339(),
            end_time: (at() + chrono::Duration::hours(1)).to_rfc3339(),
            is_all_day: false,
            status: "confirmed".into(),
            hangout_link: (!solo).then(|| "https://example.test/meeting".into()),
            conference_data: None,
            organizer_email: None,
            attendees: vec![attendee(
                Some(" Person@Example.com "),
                Some(" Person "),
                None,
            )],
        }
    }

    async fn db() -> Db {
        let db = Db::open_in_memory().await.unwrap();
        settings::set(db.pool(), "calendar.google.account", &"self@example.com")
            .await
            .unwrap();
        settings::set(
            db.pool(),
            "calendar.google.lastSync",
            &serde_json::json!({
                "at": at().to_rfc3339(), "account": "self@example.com", "snapshotVersion": 1,
                "outcome": {"state": "failed", "message": "fixture"}
            }),
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO google_calendars (id, summary, account_email, is_selected) VALUES ('cal', 'Calendar', 'self@example.com', 1)")
            .execute(db.pool()).await.unwrap();
        db
    }

    async fn start(db: &Db, title: Option<&str>, choice: CalendarLinkChoice) -> StartPlan {
        resolve(
            db,
            MeetingPlan {
                title: title.and_then(clean_title),
                ..Default::default()
            },
            choice,
            at(),
        )
        .await
    }

    #[tokio::test]
    async fn start_choices_titles_solo_and_overlap_use_the_gated_snapshot() {
        let db = db().await;
        let empty = start(&db, None, CalendarLinkChoice::Auto).await;
        assert!(empty.plan.calendar.is_none());
        assert_eq!(empty.notice, None);
        let a = row("a", Some("  Event A\n review "), false);
        events_repo::upsert_event(db.pool(), &a).await.unwrap();
        let auto = start(&db, Some("  "), CalendarLinkChoice::Auto).await;
        let linked = auto.plan.calendar.unwrap();
        assert_eq!(linked.event_id, a.id);
        assert!(linked.title_from_event);
        assert_eq!(auto.plan.title.as_deref(), Some("Event A review"));
        assert_eq!(
            linked.participants[0].email.as_deref(),
            Some("person@example.com")
        );
        let typed = start(
            &db,
            Some(" My title "),
            CalendarLinkChoice::Event {
                event_id: a.id.clone(),
            },
        )
        .await;
        assert_eq!(typed.plan.title.as_deref(), Some("My title"));
        assert!(!typed.plan.calendar.unwrap().title_from_event);
        let mut solo = row("solo", Some("Focus"), true);
        solo.attendees.clear();
        events_repo::upsert_event(db.pool(), &solo).await.unwrap();
        let explicit = start(
            &db,
            None,
            CalendarLinkChoice::Event {
                event_id: solo.id.clone(),
            },
        )
        .await;
        assert_eq!(explicit.plan.calendar.unwrap().event_id, solo.id);
        assert!(start(&db, None, CalendarLinkChoice::Auto)
            .await
            .plan
            .calendar
            .is_some());
        let b = row("b", Some("Event B"), false);
        events_repo::upsert_event(db.pool(), &b).await.unwrap();
        let overlap = start(&db, None, CalendarLinkChoice::Auto).await;
        assert!(overlap.plan.calendar.is_none());
        assert_eq!(overlap.notice, Some(CalendarLinkNotice::Overlap));
        assert_eq!(
            overlap
                .choices
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            [a.id.as_str(), b.id.as_str()]
        );
        assert!(start(&db, None, CalendarLinkChoice::None)
            .await
            .plan
            .calendar
            .is_none());
        let mut ended = row("ended", Some("Ended"), false);
        ended.end_time = at().to_rfc3339();
        events_repo::upsert_event(db.pool(), &ended).await.unwrap();
        for event_id in ["missing".to_owned(), ended.id] {
            let absent = start(&db, None, CalendarLinkChoice::Event { event_id }).await;
            assert!(absent.plan.calendar.is_none());
            assert_eq!(absent.notice, Some(CalendarLinkNotice::ChoiceUnavailable));
        }
        let untrusted = MeetingPlan {
            calendar: Some(linked),
            ..Default::default()
        };
        let built = plan_at_start(&db, untrusted, "fixture", CalendarLinkChoice::None).await;
        assert!(built.plan.calendar.is_none());
    }

    #[tokio::test]
    async fn unreadable_calendar_does_not_prevent_start_and_none_needs_no_read() {
        let db = db().await;
        sqlx::query("DROP TABLE calendar_events")
            .execute(db.pool())
            .await
            .unwrap();
        let result = start(&db, None, CalendarLinkChoice::Auto).await;
        assert_eq!(result.notice, Some(CalendarLinkNotice::CalendarUnreadable));
        assert!(result.plan.calendar.is_none());
        assert_eq!(
            start(&db, None, CalendarLinkChoice::None).await.notice,
            None
        );
    }

    #[tokio::test]
    async fn live_edits_use_start_time_commit_sidecar_and_preserve_title_provenance() {
        let db = db().await;
        let a = row("a", Some("A"), false);
        let mut b = row("b", Some("B"), false);
        events_repo::upsert_event(db.pool(), &a).await.unwrap();
        events_repo::upsert_event(db.pool(), &b).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = recording_start(dir.path(), at(), uuid::Uuid::new_v4());
        let session = session_id(&path).unwrap();
        assert!(session.starts_with("20261111-120000-"));
        let overlap = start(&db, None, CalendarLinkChoice::Auto).await;
        let mut plan = overlap.plan;
        let mut notice = overlap.notice;
        let emitted = Cell::new(0);
        assert!(edit(
            &db,
            &path,
            "stale",
            CalendarSetChoice::None,
            &mut plan,
            &mut notice,
            || emitted.set(emitted.get() + 1)
        )
        .await
        .is_err());
        assert_eq!(emitted.get(), 0);
        for (id, title) in [(&a.id, "A"), (&b.id, "B")] {
            edit(
                &db,
                &path,
                &session,
                CalendarSetChoice::Event {
                    event_id: id.clone(),
                },
                &mut plan,
                &mut notice,
                || emitted.set(emitted.get() + 1),
            )
            .await
            .unwrap();
            assert_eq!(plan.title.as_deref(), Some(title));
            assert_eq!(draft::read_plan(&path), Some(plan.clone()));
            assert_eq!(notice, None);
        }
        let before = plan.clone();
        assert!(edit(
            &db,
            &path,
            &session,
            CalendarSetChoice::Event {
                event_id: "gone".into()
            },
            &mut plan,
            &mut notice,
            || emitted.set(emitted.get() + 1)
        )
        .await
        .is_err());
        assert_eq!(plan, before);
        assert_eq!(emitted.get(), 2);
        assert_eq!(draft::read_plan(&path), Some(before));
        // Removing a titled event must clear the inherited title, too.
        edit(
            &db,
            &path,
            &session,
            CalendarSetChoice::None,
            &mut plan,
            &mut notice,
            || {},
        )
        .await
        .unwrap();
        assert!(plan.title.is_none());
        assert!(plan.calendar.is_none());
        assert_eq!(draft::read_plan(&path), Some(plan.clone()));
        edit(
            &db,
            &path,
            &session,
            CalendarSetChoice::Event {
                event_id: a.id.clone(),
            },
            &mut plan,
            &mut notice,
            || {},
        )
        .await
        .unwrap();
        b.summary = None;
        events_repo::upsert_event(db.pool(), &b).await.unwrap();
        edit(
            &db,
            &path,
            &session,
            CalendarSetChoice::Event {
                event_id: b.id.clone(),
            },
            &mut plan,
            &mut notice,
            || {},
        )
        .await
        .unwrap();
        assert!(plan.title.is_none());
        edit(
            &db,
            &path,
            &session,
            CalendarSetChoice::None,
            &mut plan,
            &mut notice,
            || emitted.set(emitted.get() + 1),
        )
        .await
        .unwrap();
        assert!(plan.calendar.is_none());
        assert!(plan.title.is_none());
        assert_eq!(draft::read_plan(&path), Some(plan));
        assert_eq!(emitted.get(), 3);
        // These fixed-time events still work after their end time because live
        // edits validate against the recording stem, never today's wall clock.
        let mut plan = MeetingPlan {
            title: Some("Typed".into()),
            ..Default::default()
        };
        for choice in [
            CalendarSetChoice::Event { event_id: a.id },
            CalendarSetChoice::Event { event_id: b.id },
            CalendarSetChoice::None,
        ] {
            edit(&db, &path, &session, choice, &mut plan, &mut notice, || {})
                .await
                .unwrap();
            assert_eq!(plan.title.as_deref(), Some("Typed"));
        }
    }

    #[tokio::test]
    async fn failed_writes_keep_the_previous_live_link_but_start_drops_its_link() {
        let db = db().await;
        let a = row("a", Some("A"), false);
        events_repo::upsert_event(db.pool(), &a).await.unwrap();
        let mut start = start(&db, None, CalendarLinkChoice::Auto).await;
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = recording_start(dir.path(), at(), uuid::Uuid::new_v4());
        draft::write_plan(&path, &start.plan).unwrap();
        let mut temp_name = draft::plan_path(&path).into_os_string();
        temp_name.push(".tmp");
        std::fs::create_dir(std::path::PathBuf::from(temp_name)).unwrap();
        let previous = start.plan.clone();
        let emitted = Cell::new(false);
        assert!(edit(
            &db,
            &path,
            &session_id(&path).unwrap(),
            CalendarSetChoice::None,
            &mut start.plan,
            &mut start.notice,
            || emitted.set(true)
        )
        .await
        .is_err());
        assert!(!emitted.get());
        assert_eq!(start.plan, previous);
        assert_eq!(draft::read_plan(&path), Some(previous));
        let missing = dir.path().join("missing").join("new.wav");
        persist_start(&missing, &mut start.plan, &mut start.notice);
        assert!(start.plan.calendar.is_none());
        assert_eq!(start.notice, Some(CalendarLinkNotice::NotSaved));
        assert!(draft::read_plan(&missing).is_none());
    }

    async fn assert_saved(db: &Db, id: i64, snapshot: &LinkedEvent) {
        let (event_id, json): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT calendar_event_id, participants FROM notes WHERE id = ?")
                .bind(id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(event_id.as_deref(), Some(snapshot.event_id.as_str()));
        assert_eq!(
            serde_json::from_str::<Vec<Participant>>(&json.unwrap()).unwrap(),
            snapshot.participants
        );
    }

    #[tokio::test]
    async fn stopped_discarded_api_shared_save_and_sidecar_recovery_keep_the_snapshot() {
        let db = db().await;
        let a = row("a", Some("A"), false);
        events_repo::upsert_event(db.pool(), &a).await.unwrap();
        let plan = start(&db, None, CalendarLinkChoice::Auto).await.plan;
        let snapshot = plan.calendar.as_ref().unwrap();
        // Calendar deletion cannot prevent saving or change the captured people.
        sqlx::query("DELETE FROM calendar_events")
            .execute(db.pool())
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (path, started) = recording_start(dir.path(), at(), uuid::Uuid::new_v4());
        std::fs::write(&path, b"fixture recording").unwrap();
        let note = || kw_notes_core::notes::NewNote {
            title: "Saved".into(),
            content: "Typed".into(),
            ..Default::default()
        };
        let stopped = save_stopped(&db, note(), &plan, &path, 12.0, None)
            .await
            .unwrap();
        assert_saved(&db, stopped.note_id, snapshot).await;
        let discarded = save_discarded(&db, &path, "Kept notes", &plan, started)
            .await
            .unwrap();
        assert_saved(&db, discarded, snapshot).await;
        // API stop delegates to the same runtime Stop; exercise its shared seam
        // without starting microphone capture or connecting a live provider.
        let (api, _) = create_meeting_note_labelling(&db, note(), &plan, None, None)
            .await
            .unwrap();
        assert_saved(&db, api, snapshot).await;
        let (recovered_path, _) = recording_start(dir.path(), at(), uuid::Uuid::new_v4());
        std::fs::write(&recovered_path, b"fixture recording").unwrap();
        draft::write_plan(&recovered_path, &plan).unwrap();
        let outcome = save_recovered(
            &db,
            &recovered_path,
            &session_id(&recovered_path).unwrap(),
            "Recovered",
            &[],
            "fixture",
            12.0,
        )
        .await
        .unwrap();
        let MeetingOutcome::Saved { note_id, .. } = outcome else {
            panic!("expected saved")
        };
        assert_saved(&db, note_id, snapshot).await;
        assert!(!draft::plan_path(&recovered_path).exists());
        // Existing people are never overwritten by an event snapshot.
        let mut existing = note();
        existing.participants = Some("[]".into());
        let (id, _) = create_meeting_note(&db, existing, &plan, None)
            .await
            .unwrap();
        let stored = kw_notes_core::notes::get(db.pool(), id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.participants.as_deref(), Some("[]"));
        let mut empty_plan = plan;
        empty_plan.calendar.as_mut().unwrap().participants.clear();
        let (id, _) = create_meeting_note(&db, note(), &empty_plan, None)
            .await
            .unwrap();
        assert!(kw_notes_core::notes::get(db.pool(), id)
            .await
            .unwrap()
            .unwrap()
            .participants
            .is_none());
    }

    #[test]
    fn participant_filter_normalisation_deduplication_and_cap() {
        let mut event = row("a", None, false);
        event.attendees = vec![
            attendee(Some(" SELF@EXAMPLE.COM "), Some("Self"), None),
            attendee(Some("no@example.com"), Some("No"), Some("declined")),
            attendee(Some(" A@EXAMPLE.COM "), Some(" A "), None),
            attendee(Some("a@example.com"), Some("Duplicate"), None),
            attendee(Some("invalid"), Some(" Name only "), None),
            attendee(Some("invalid"), Some("  "), None),
            attendee(None, None, None),
        ];
        let (people, omitted) = participants(&event, Some("self@example.com"));
        assert_eq!(omitted, 0);
        assert_eq!(
            people,
            vec![
                Participant {
                    email: Some("a@example.com".into()),
                    display_name: Some("A".into())
                },
                Participant {
                    email: None,
                    display_name: Some("Name only".into())
                }
            ]
        );
        event
            .attendees
            .extend((0..201).map(|n| attendee(Some(&format!("p{n}@example.com")), None, None)));
        event
            .attendees
            .push(attendee(Some("p200@example.com"), None, None));
        let (people, omitted) = participants(&event, Some("self@example.com"));
        assert_eq!(people.len(), 200);
        assert_eq!(omitted, 3);
    }
}
