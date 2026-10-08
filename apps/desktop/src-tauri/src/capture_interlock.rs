//! A microphone test is exclusive; existing simultaneous recording use is preserved.
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Default)]
struct Uses {
    recordings: usize,
    checking: bool,
}
#[derive(Debug, Default)]
pub struct CaptureInterlock(Mutex<Uses>);
#[derive(Debug)]
pub struct CaptureUse {
    gate: Arc<CaptureInterlock>,
    check: bool,
}
impl Drop for CaptureUse {
    fn drop(&mut self) {
        if let Ok(mut uses) = self.gate.0.lock() {
            if self.check {
                uses.checking = false;
            } else {
                uses.recordings = uses.recordings.saturating_sub(1);
            }
        }
    }
}
impl CaptureInterlock {
    pub fn shared() -> Arc<Self> {
        static GATE: OnceLock<Arc<CaptureInterlock>> = OnceLock::new();
        Arc::clone(GATE.get_or_init(|| Arc::new(Self::default())))
    }
    pub fn recording(self: &Arc<Self>) -> Result<CaptureUse, String> {
        let mut uses = self
            .0
            .lock()
            .map_err(|_| "The microphone state is unavailable.")?;
        if uses.checking {
            return Err("Not now: the microphone is being tested. Start recording once the test has finished. If the test timed out and has not closed, restart the app.".into());
        }
        uses.recordings += 1;
        Ok(CaptureUse {
            gate: Arc::clone(self),
            check: false,
        })
    }
    pub fn check(self: &Arc<Self>) -> Result<CaptureUse, String> {
        let mut uses = self
            .0
            .lock()
            .map_err(|_| "The microphone state is unavailable.")?;
        if uses.checking || uses.recordings > 0 {
            return Err("Not now: the microphone is already in use.".into());
        }
        uses.checking = true;
        Ok(CaptureUse {
            gate: Arc::clone(self),
            check: true,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn check_and_starts_are_atomic_but_two_recordings_still_coexist() {
        let gate = Arc::new(CaptureInterlock::default());
        let first = gate.recording().unwrap();
        let second = gate.recording().unwrap();
        assert!(gate.check().is_err());
        drop(first);
        assert!(gate.check().is_err());
        drop(second);
        let checking = gate.check().unwrap();
        assert!(gate.recording().is_err());
        assert!(gate.check().is_err());
        drop(checking);
        assert!(gate.recording().is_ok());
    }
}
