//! Runtime-only progress for Stop and recovery. Never a percentage or an ETA.
use serde::Serialize;
use std::time::Instant;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    PreparingRecording,
    FinishingLiveTranscript,
    TranscribingRecording,
    SavingNote,
}

#[derive(Debug, Clone, Copy, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Stop,
    Recovery,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FinalisationProgress {
    pub operation_id: u64,
    pub kind: Kind,
    pub stage: Stage,
    pub elapsed_seconds: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FinalisationUpdate {
    pub revision: u64,
    pub progress: Option<FinalisationProgress>,
}

#[derive(Default)]
pub struct Tracker {
    revision: u64,
    active: Option<(u64, Kind, Stage, Instant)>,
}
impl Tracker {
    pub fn begin(&mut self, id: u64, kind: Kind) -> FinalisationUpdate {
        self.active = Some((id, kind, Stage::PreparingRecording, Instant::now()));
        self.revision += 1;
        self.snapshot()
    }
    pub fn stage(&mut self, stage: Stage) -> Option<FinalisationUpdate> {
        let active = self.active.as_mut()?;
        if active.2 == stage {
            return None;
        }
        active.2 = stage;
        self.revision += 1;
        Some(self.snapshot())
    }
    pub fn end(&mut self, id: u64) -> Option<FinalisationUpdate> {
        if self.active.as_ref()?.0 != id {
            return None;
        }
        self.active = None;
        self.revision += 1;
        Some(self.snapshot())
    }
    pub fn snapshot(&self) -> FinalisationUpdate {
        FinalisationUpdate {
            revision: self.revision,
            progress: self
                .active
                .as_ref()
                .map(|(id, kind, stage, started)| FinalisationProgress {
                    operation_id: *id,
                    kind: *kind,
                    stage: *stage,
                    elapsed_seconds: started.elapsed().as_secs_f64(),
                }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn stages_preserve_identity_and_elapsed_and_stale_end_cannot_clear_new_operation() {
        let mut tracker = Tracker::default();
        assert!(tracker.stage(Stage::SavingNote).is_none());
        let initial = tracker.begin(10, Kind::Stop);
        let started = tracker.active.as_ref().unwrap().3;
        let stage = tracker.stage(Stage::TranscribingRecording).unwrap();
        assert!(stage.revision > initial.revision);
        let progress = stage.progress.unwrap();
        assert_eq!(progress.operation_id, 10);
        assert_eq!(progress.stage, Stage::TranscribingRecording);
        assert_eq!(tracker.active.as_ref().unwrap().3, started);
        assert!(tracker.stage(Stage::TranscribingRecording).is_none());
        tracker.begin(11, Kind::Recovery);
        assert!(tracker.end(10).is_none());
        assert_eq!(tracker.snapshot().progress.unwrap().operation_id, 11);
        assert!(tracker.end(11).unwrap().progress.is_none());
        assert!(tracker.end(11).is_none());
    }
}
