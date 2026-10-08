//! How loud a capture is right now, for a meter on screen (KNEG-371).
//!
//! The capture callbacks run on the audio device's real-time thread, and the
//! one rule here is that a meter adds nothing to them that can wait: no
//! allocation, no lock, no event, no system call. What the callback does is
//! one pass over the block it was handed, for its RMS, and one atomic
//! `fetch_max` into a [`LevelMeter`]. Everything else -- reading the level,
//! turning it into a fraction of the meter, deciding whether to send it --
//! happens on whichever thread holds the [`LevelReader`].
//!
//! `fetch_max` on the bit pattern of an `f32` is only ordered like the float
//! for values that are not negative and not NaN. RMS is never negative, and
//! [`LevelMeter::record`] refuses NaN and zero before the atomic, so the
//! integer maximum is the loudest block.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

/// The quietest level a meter shows, in dBFS. At or below it, the meter is
/// empty.
pub const FLOOR_DBFS: f32 = -60.0;

/// The RMS and the peak amplitude of one block of normalised samples.
///
/// `(0.0, 0.0)` for an empty block. One pass, accumulated in `f64` so an hour
/// of quiet samples does not lose the few that matter.
#[must_use]
pub fn block_rms_peak(samples: &[f32]) -> (f32, f32) {
    rms_peak(samples.iter().copied())
}

/// [`block_rms_peak`] over samples that are converted as they are read, so a
/// callback handed `i16` does not have to allocate an `f32` copy first.
#[must_use]
pub fn rms_peak(samples: impl IntoIterator<Item = f32>) -> (f32, f32) {
    let mut count = 0_u64;
    let mut sum_squares = 0.0_f64;
    let mut peak = 0.0_f32;
    for sample in samples {
        count += 1;
        sum_squares += f64::from(sample) * f64::from(sample);
        peak = peak.max(sample.abs());
    }
    if count == 0 {
        return (0.0, 0.0);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let rms = (sum_squares / count as f64).sqrt() as f32;
    (rms, peak)
}

/// Where an RMS sits on the meter: [`FLOOR_DBFS`] is 0, full scale is 1.
///
/// Linear in decibels, which is how loudness is heard, and clamped at both
/// ends. NaN, zero and anything negative are 0.
#[must_use]
pub fn level_from_rms(rms: f32) -> f32 {
    if rms.is_nan() || rms <= 0.0 {
        return 0.0;
    }
    let dbfs = 20.0 * rms.log10();
    ((dbfs - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0)
}

/// The writing half: the loudest block since the reader last looked.
///
/// Clone it into the capture callback. Dropping every clone is what tells the
/// [`LevelReader`] the capture has stopped.
#[derive(Debug, Clone, Default)]
pub struct LevelMeter {
    /// The loudest block's RMS, as `f32` bits. Zero is silence.
    loudest: Arc<AtomicU32>,
}

impl LevelMeter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep `rms` if it is the loudest since the last [`LevelReader::take`].
    ///
    /// Real-time safe: one atomic read-modify-write, nothing that allocates
    /// or waits.
    pub fn record(&self, rms: f32) {
        // Refused here, not filtered later: NaN's bits are larger than any
        // real level's, so once in the atomic it would win every comparison.
        if rms.is_nan() || rms <= 0.0 {
            return;
        }
        self.loudest.fetch_max(rms.to_bits(), Ordering::Relaxed);
    }

    /// [`LevelMeter::record`] for a block of normalised samples.
    pub fn observe(&self, samples: &[f32]) {
        let (rms, _) = block_rms_peak(samples);
        self.record(rms);
    }

    /// The reading half, which does not keep the meter alive.
    #[must_use]
    pub fn reader(&self) -> LevelReader {
        LevelReader {
            loudest: Arc::downgrade(&self.loudest),
        }
    }
}

/// The reading half of a [`LevelMeter`].
///
/// Holds the meter weakly, so a reader left behind cannot keep a stopped
/// capture's meter alive, and can tell that it stopped.
#[derive(Debug, Clone)]
pub struct LevelReader {
    loudest: Weak<AtomicU32>,
}

impl LevelReader {
    /// The loudest level since the last take, as [`level_from_rms`] places
    /// it, and silence again from here.
    ///
    /// `None` once the capture is gone: every [`LevelMeter`] clone has been
    /// dropped with the stream that held it.
    #[must_use]
    pub fn take(&self) -> Option<f32> {
        let loudest = self.loudest.upgrade()?;
        let rms = f32::from_bits(loudest.swap(0, Ordering::Relaxed));
        Some(level_from_rms(rms))
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

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    /// The scale the owner will read: −60 dBFS is empty, full scale is full,
    /// and the decibels between are spread evenly.
    #[test]
    fn the_meter_runs_from_minus_sixty_dbfs_to_full_scale() {
        assert!(close(level_from_rms(1.0), 1.0));
        assert!(close(level_from_rms(0.001), 0.0), "-60 dBFS is the floor");
        assert!(close(level_from_rms(0.01), 1.0 / 3.0), "-40 dBFS");
        assert!(close(level_from_rms(0.1), 2.0 / 3.0), "-20 dBFS");
    }

    /// Clamped at both ends: quieter than the floor is empty, and a block
    /// louder than full scale (possible from a float device) is full.
    #[test]
    fn the_meter_is_clamped_at_both_ends() {
        assert!(close(level_from_rms(0.000_1), 0.0));
        assert!(close(level_from_rms(4.0), 1.0));
        assert!(close(level_from_rms(f32::INFINITY), 1.0));
    }

    /// Nothing that is not a level reads as one: NaN from a bad block, zero
    /// from digital silence, and a negative that no RMS can be.
    #[test]
    fn nan_zero_and_negative_are_silence() {
        assert!(close(level_from_rms(f32::NAN), 0.0));
        assert!(close(level_from_rms(0.0), 0.0));
        assert!(close(level_from_rms(-0.5), 0.0));
    }

    /// A square wave at ±0.5 has RMS and peak both exactly 0.5, and the
    /// converting form agrees with the slice form.
    #[test]
    fn rms_and_peak_are_computed_for_a_known_signal() {
        let samples: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let (rms, peak) = block_rms_peak(&samples);
        assert!(close(rms, 0.5), "rms was {rms}");
        assert!(close(peak, 0.5), "peak was {peak}");

        let as_i16: Vec<i16> = samples.iter().map(|s| (s * 32_768.0) as i16).collect();
        let (rms16, peak16) = rms_peak(as_i16.iter().map(|s| f32::from(*s) / 32_768.0));
        assert!(close(rms16, 0.5), "rms from i16 was {rms16}");
        assert!(close(peak16, 0.5), "peak from i16 was {peak16}");
    }

    #[test]
    fn an_empty_block_has_no_level() {
        assert_eq!(block_rms_peak(&[]), (0.0, 0.0));
        assert_eq!(rms_peak(std::iter::empty()), (0.0, 0.0));
    }

    /// Max-hold: a quiet block after a loud one does not lower what the
    /// reader sees, because the meter is about the loudest moment since it
    /// last looked.
    #[test]
    fn the_meter_holds_the_loudest_block_until_it_is_read() {
        let meter = LevelMeter::new();
        let reader = meter.reader();
        meter.record(0.01);
        meter.record(0.1);
        meter.record(0.001);
        let level = reader.take().unwrap();
        assert!(close(level, level_from_rms(0.1)), "level was {level}");
    }

    /// Taking resets the meter, so a sound that stopped stops showing.
    #[test]
    fn taking_the_level_resets_it_to_silence() {
        let meter = LevelMeter::new();
        let reader = meter.reader();
        meter.record(0.1);
        assert!(reader.take().unwrap() > 0.0);
        assert_eq!(reader.take(), Some(0.0), "nothing since the last take");
    }

    /// NaN would compare as louder than any real block if it reached the
    /// atomic, and stay there; it is refused before it does.
    #[test]
    fn a_nan_block_cannot_pin_the_meter_full() {
        let meter = LevelMeter::new();
        let reader = meter.reader();
        meter.record(f32::NAN);
        meter.record(0.01);
        let level = reader.take().unwrap();
        assert!(close(level, level_from_rms(0.01)), "level was {level}");
    }

    /// The reader learns the capture stopped, which is what ends a meter's
    /// publisher: every clone of the meter dropped, the reader gets `None`.
    #[test]
    fn the_reader_gets_none_once_the_capture_is_dropped() {
        let meter = LevelMeter::new();
        let in_callback = meter.clone();
        let reader = meter.reader();
        drop(meter);
        assert_eq!(
            reader.take(),
            Some(0.0),
            "the callback's clone still holds it"
        );
        drop(in_callback);
        assert_eq!(reader.take(), None);
    }

    #[test]
    fn observing_samples_records_their_rms() {
        let meter = LevelMeter::new();
        let reader = meter.reader();
        meter.observe(&[0.1, -0.1, 0.1, -0.1]);
        let level = reader.take().unwrap();
        assert!(close(level, level_from_rms(0.1)), "level was {level}");
    }
}
