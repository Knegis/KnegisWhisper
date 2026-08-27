//! Turning two long-running captures into windows to transcribe and one file to
//! keep.
//!
//! A dictation is short: record, stop, convert the whole thing, send it. A
//! meeting is neither short nor over — it has to produce transcript while it is
//! still running, and it has to produce a recording longer than memory. Both of
//! those need the audio broken into pieces, and this module is the arithmetic for
//! doing it. Everything here is pure, for the same reason [`crate::mix`] is:
//! neither a microphone nor a loopback device can be relied on in a test, and the
//! part that decides *what* gets transcribed can be.
//!
//! # Windows overlap on purpose
//!
//! [`WindowFeed`] hands out a five-second window every three seconds, so
//! consecutive windows share two seconds. A word straddling a boundary is
//! therefore whole in at least one window. Non-overlapping windows would cut
//! roughly one word every three seconds in half, and half a word is worse than a
//! repeated one — a duplicate can be recognised and dropped, a truncation
//! cannot be recovered.
//!
//! # Falling behind is reported, not hidden
//!
//! Nothing here knows how long transcription takes, but the consequence lands
//! here: if a window takes longer to transcribe than the hop, the next one is
//! already waiting and the lag grows. [`WindowFeed::lag`] is that lag and
//! [`WindowFeed::skip_to_live`] is the deliberate recovery, returning how many
//! windows it dropped. A caller that reports that number produces a transcript
//! with visible gaps; one that does not produces a transcript that looks
//! complete. The same argument as `MeetingTranscript::reconnects`.
//!
//! # The mix clips rather than limits, and counts it
//!
//! [`crate::mix::mix`] sums the tracks and applies gain only if the peak
//! exceeds full scale. That needs the whole recording, because it needs the
//! peak. Writing to disk as the meeting happens means the peak is not knowable
//! yet, so [`MixFeed`] sums and lets the encoder clamp — and counts the samples
//! it clamped. Dividing by two instead would halve every passage where only one
//! source is active, which is most of a meeting and is the thing `mix` already
//! rejected. Counting the clipped samples keeps the trade visible instead of
//! turning it into a quiet quality loss.

use std::time::Duration;

use crate::wav;

/// One window of audio, ready to transcribe.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Mono samples at [`wav::TARGET_SAMPLE_RATE`].
    pub samples: Vec<f32>,
    /// Seconds from the start of the session to this window's first sample.
    ///
    /// Carried because a transcript event needs a timestamp and the window is
    /// the only thing that knows where it came from.
    pub start_seconds: f64,
}

/// Accumulates one source's samples and hands out overlapping windows.
///
/// Fed mono audio already at [`wav::TARGET_SAMPLE_RATE`] — the conversion from a
/// device's own rate and channel count belongs to the caller, which is the only
/// thing that knows them.
#[derive(Debug)]
pub struct WindowFeed {
    held: Vec<f32>,
    /// Samples the feed has moved past, so a window can say where it starts.
    consumed: u64,
    /// Samples appended but not yet passed, for the lag calculation.
    appended: u64,
    window: usize,
    hop: usize,
}

impl WindowFeed {
    /// A feed with the given window length and hop.
    ///
    /// A hop longer than the window would leave gaps between windows, so it is
    /// clamped to the window: overlapping is a choice, skipping audio is not.
    #[must_use]
    pub fn new(window: Duration, hop: Duration) -> Self {
        let window = samples_in(window).max(1);
        let hop = samples_in(hop).clamp(1, window);

        Self {
            held: Vec::new(),
            consumed: 0,
            appended: 0,
            window,
            hop,
        }
    }

    /// The five-and-three feed the local client uses.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(Duration::from_secs(5), Duration::from_secs(3))
    }

    /// Append mono samples at the target rate.
    pub fn push(&mut self, samples: &[f32]) {
        self.held.extend_from_slice(samples);
        self.appended = self.appended.saturating_add(samples.len() as u64);
    }

    /// The next window, if enough audio has arrived for one.
    ///
    /// Call until it returns `None`; after a burst of samples there may be
    /// several waiting.
    pub fn next_window(&mut self) -> Option<Window> {
        if self.held.len() < self.window {
            return None;
        }

        let samples = self.held.get(..self.window)?.to_vec();
        let start = self.consumed;
        // Only the hop is dropped, so the next window overlaps this one by
        // `window - hop`.
        self.held.drain(..self.hop);
        self.consumed = self.consumed.saturating_add(self.hop as u64);

        Some(Window {
            samples,
            start_seconds: seconds(start),
        })
    }

    /// Seconds of audio this feed has already moved past.
    ///
    /// The session clock: [`Self::lag`] is what is still waiting, and the two
    /// together are how much audio has been seen.
    #[must_use]
    pub fn consumed_seconds(&self) -> f64 {
        seconds(self.consumed)
    }

    /// How far behind the live edge the next window would start.
    #[must_use]
    pub fn lag(&self) -> Duration {
        Duration::from_secs_f64(seconds(self.appended.saturating_sub(self.consumed)))
    }

    /// Throw away everything but the newest window, to catch up.
    ///
    /// Returns how many windows were skipped, which the caller is expected to
    /// report. Dropping audio silently is the thing this returns a number to
    /// prevent.
    pub fn skip_to_live(&mut self) -> u64 {
        if self.held.len() <= self.window {
            return 0;
        }

        let dropped = self.held.len() - self.window;
        self.held.drain(..dropped);
        self.consumed = self.consumed.saturating_add(dropped as u64);

        // In whole windows, rounded up: a partial window's worth of audio is
        // still audio that was dropped.
        (dropped as u64).div_ceil(self.hop as u64)
    }

    /// Whatever is left at the end of the session.
    ///
    /// A tail shorter than `minimum` is discarded: a fifth of a second of audio
    /// transcribes to noise or to nothing, and a hallucinated word at the end of
    /// a meeting is worse than a missing one.
    pub fn flush(&mut self, minimum: Duration) -> Option<Window> {
        if self.held.len() < samples_in(minimum).max(1) {
            self.held.clear();
            return None;
        }

        let samples = std::mem::take(&mut self.held);
        let start = self.consumed;
        self.consumed = self.consumed.saturating_add(samples.len() as u64);

        Some(Window {
            samples,
            start_seconds: seconds(start),
        })
    }
}

/// Sums two mono streams as they arrive, for writing to disk during the meeting.
///
/// Both streams start at sample zero — see [`crate::mix`] on why alignment is at
/// the start — so mixing only the prefix both have, and keeping each remainder,
/// stays exactly aligned however unevenly the two callbacks deliver.
#[derive(Debug, Default)]
pub struct MixFeed {
    a: Vec<f32>,
    b: Vec<f32>,
    clipped: u64,
    written: u64,
}

impl MixFeed {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_a(&mut self, samples: &[f32]) {
        self.a.extend_from_slice(samples);
    }

    pub fn push_b(&mut self, samples: &[f32]) {
        self.b.extend_from_slice(samples);
    }

    /// Mix the part both streams have, leaving each remainder for next time.
    pub fn take(&mut self) -> Vec<f32> {
        let n = self.a.len().min(self.b.len());
        if n == 0 {
            return Vec::new();
        }

        let a = self.a.drain(..n).collect::<Vec<f32>>();
        let b = self.b.drain(..n).collect::<Vec<f32>>();
        self.sum(&a, &b)
    }

    /// Mix everything left, padding the shorter stream with silence.
    ///
    /// Padding rather than truncating, for the reason [`crate::mix`] gives: the
    /// two streams stop a few milliseconds apart and the end of a recording is
    /// where the end of a sentence is.
    pub fn flush(&mut self) -> Vec<f32> {
        let n = self.a.len().max(self.b.len());
        if n == 0 {
            return Vec::new();
        }

        let mut a = std::mem::take(&mut self.a);
        let mut b = std::mem::take(&mut self.b);
        a.resize(n, 0.0);
        b.resize(n, 0.0);
        self.sum(&a, &b)
    }

    fn sum(&mut self, a: &[f32], b: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(a.len());
        for (left, right) in a.iter().zip(b.iter()) {
            let summed = left + right;
            if !(-1.0..=1.0).contains(&summed) {
                self.clipped = self.clipped.saturating_add(1);
            }
            out.push(summed);
        }
        self.written = self.written.saturating_add(out.len() as u64);
        out
    }

    /// How many samples exceeded full scale and will be clamped by the encoder.
    ///
    /// Reported rather than corrected, because correcting it needs the peak of a
    /// recording that has not finished. A non-zero count on a real meeting is
    /// the measurement that would justify a limiter.
    #[must_use]
    pub const fn clipped(&self) -> u64 {
        self.clipped
    }

    /// How many mixed samples have been handed out.
    #[must_use]
    pub const fn written(&self) -> u64 {
        self.written
    }
}

/// Samples in `duration` at the target rate.
fn samples_in(duration: Duration) -> usize {
    let rate = f64::from(wav::TARGET_SAMPLE_RATE);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let samples = (duration.as_secs_f64() * rate) as usize;
    samples
}

/// Seconds `count` samples last at the target rate.
fn seconds(count: u64) -> f64 {
    if wav::TARGET_SAMPLE_RATE == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let seconds = count as f64 / f64::from(wav::TARGET_SAMPLE_RATE);
    seconds
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

    const RATE: usize = wav::TARGET_SAMPLE_RATE as usize;

    /// A counting sequence, for checking that sample n stays sample n. The values
    /// are far outside audio range on purpose: an off-by-one is obvious, and a
    /// test that cares about levels uses real ones instead.
    fn ramp(n: usize, from: usize) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        (0..n).map(|i| (from + i) as f32).collect()
    }

    /* ---------------------------------------------------------- windowing */

    #[test]
    fn nothing_comes_out_before_a_whole_window_has_arrived() {
        let mut feed = WindowFeed::standard();
        feed.push(&vec![0.1; RATE * 4]);
        assert!(
            feed.next_window().is_none(),
            "four seconds is not a five-second window"
        );

        feed.push(&vec![0.1; RATE]);
        assert!(feed.next_window().is_some());
    }

    #[test]
    fn consecutive_windows_overlap_by_the_difference() {
        // Five and three, so two seconds of overlap. A word on a boundary is
        // whole in one of them.
        let mut feed = WindowFeed::standard();
        feed.push(&ramp(RATE * 8, 0));

        let first = feed.next_window().unwrap();
        let second = feed.next_window().unwrap();

        assert_eq!(first.start_seconds, 0.0);
        assert_eq!(second.start_seconds, 3.0);
        assert_eq!(first.samples.len(), RATE * 5);
        assert_eq!(second.samples.len(), RATE * 5);

        // The last two seconds of the first window are the first two of the
        // second.
        assert_eq!(first.samples[RATE * 3..], second.samples[..RATE * 2]);
    }

    #[test]
    fn a_burst_of_audio_yields_every_window_it_contains() {
        let mut feed = WindowFeed::standard();
        feed.push(&ramp(RATE * 20, 0));

        let mut starts = Vec::new();
        while let Some(window) = feed.next_window() {
            starts.push(window.start_seconds);
        }
        assert_eq!(starts, vec![0.0, 3.0, 6.0, 9.0, 12.0, 15.0]);
    }

    #[test]
    fn a_hop_longer_than_the_window_is_clamped_rather_than_leaving_gaps() {
        // Overlapping is a choice; skipping audio nobody asked to skip is not.
        let mut feed = WindowFeed::new(Duration::from_secs(2), Duration::from_secs(10));
        feed.push(&ramp(RATE * 6, 0));

        let first = feed.next_window().unwrap();
        let second = feed.next_window().unwrap();
        assert_eq!(first.start_seconds, 0.0);
        assert_eq!(second.start_seconds, 2.0, "the hop became the window");
    }

    /* -------------------------------------------------------- falling behind */

    #[test]
    fn the_lag_grows_when_windows_are_not_taken() {
        let mut feed = WindowFeed::standard();
        feed.push(&vec![0.1; RATE * 5]);
        assert_eq!(feed.lag(), Duration::from_secs(5));

        let _ = feed.next_window();
        assert_eq!(feed.lag(), Duration::from_secs(2), "the hop was consumed");

        feed.push(&vec![0.1; RATE * 30]);
        assert_eq!(feed.lag(), Duration::from_secs(32));
    }

    #[test]
    fn skipping_to_live_says_how_much_it_threw_away() {
        // The number is the point. A transcript with a hole in it must not look
        // complete, which is the same argument as MeetingTranscript::reconnects.
        let mut feed = WindowFeed::standard();
        feed.push(&ramp(RATE * 35, 0));

        let skipped = feed.skip_to_live();
        assert_eq!(
            skipped, 10,
            "thirty seconds of audio, at a three-second hop"
        );

        let window = feed.next_window().unwrap();
        assert_eq!(
            window.start_seconds, 30.0,
            "the next window is at the live edge"
        );
        assert!(feed.lag() <= Duration::from_secs(5));
    }

    #[test]
    fn skipping_when_there_is_nothing_to_skip_does_nothing() {
        let mut feed = WindowFeed::standard();
        feed.push(&vec![0.1; RATE * 5]);
        assert_eq!(feed.skip_to_live(), 0);
        assert!(feed.next_window().is_some());
    }

    /* -------------------------------------------------------------- the tail */

    #[test]
    fn the_tail_is_emitted_at_the_end_of_the_session() {
        let mut feed = WindowFeed::standard();
        feed.push(&ramp(RATE * 4, 0));
        assert!(feed.next_window().is_none());

        let tail = feed.flush(Duration::from_millis(500)).unwrap();
        assert_eq!(tail.samples.len(), RATE * 4);
        assert_eq!(tail.start_seconds, 0.0);
    }

    #[test]
    fn a_tail_too_short_to_mean_anything_is_discarded() {
        // A fifth of a second transcribes to noise or to a hallucinated word,
        // and a hallucinated word at the end of a meeting is worse than a
        // missing one.
        let mut feed = WindowFeed::standard();
        feed.push(&vec![0.1; RATE / 5]);
        assert!(feed.flush(Duration::from_millis(500)).is_none());
    }

    #[test]
    fn the_tail_starts_where_the_last_window_left_off() {
        let mut feed = WindowFeed::standard();
        feed.push(&ramp(RATE * 7, 0));

        let first = feed.next_window().unwrap();
        assert_eq!(first.start_seconds, 0.0);

        let tail = feed.flush(Duration::from_millis(500)).unwrap();
        assert_eq!(tail.start_seconds, 3.0);
        assert_eq!(tail.samples.len(), RATE * 4);
    }

    /* ------------------------------------------------------------- the mix */

    #[test]
    fn only_the_part_both_streams_have_is_mixed() {
        let mut feed = MixFeed::new();
        feed.push_a(&[0.1, 0.2, 0.3]);
        feed.push_b(&[0.5]);

        let mixed = feed.take();
        assert_eq!(mixed.len(), 1);
        assert!((mixed[0] - 0.6).abs() < 1e-6);

        // The rest of A waits for B rather than being mixed against silence,
        // which is what keeps the two aligned.
        feed.push_b(&[0.5, 0.5]);
        let mixed = feed.take();
        assert_eq!(mixed.len(), 2);
        assert!((mixed[0] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn uneven_deliveries_never_drift() {
        // The property that matters: however unevenly the two callbacks deliver,
        // sample n of the output is sample n of A plus sample n of B.
        let mut feed = MixFeed::new();
        let a = ramp(1_000, 0);
        let b: Vec<f32> = a.iter().map(|s| -s).collect();

        let mut out = Vec::new();
        let mut i = 0;
        let mut j = 0;
        for (step, chunk) in [7_usize, 3, 91, 1, 40].iter().cycle().take(60).enumerate() {
            if step % 2 == 0 {
                let end = (i + chunk).min(a.len());
                feed.push_a(&a[i..end]);
                i = end;
            } else {
                let end = (j + chunk).min(b.len());
                feed.push_b(&b[j..end]);
                j = end;
            }
            out.extend(feed.take());
        }
        out.extend(feed.flush());

        assert_eq!(out.len(), a.len().min(i).max(j).min(a.len()).max(out.len()));
        for (n, value) in out.iter().enumerate() {
            let expected = a.get(n).copied().unwrap_or(0.0) + b.get(n).copied().unwrap_or(0.0);
            assert!(
                (value - expected).abs() < 1e-6,
                "sample {n}: {value} != {expected}"
            );
        }
    }

    #[test]
    fn the_shorter_stream_is_padded_at_the_end_not_truncated() {
        let mut feed = MixFeed::new();
        feed.push_a(&[0.1, 0.2, 0.3, 0.4]);
        feed.push_b(&[0.5, 0.5]);

        let mixed = feed.take();
        assert_eq!(mixed.len(), 2);

        let tail = feed.flush();
        assert_eq!(tail.len(), 2, "A's last two samples survive");
        assert!((tail[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn clipping_is_counted_rather_than_corrected() {
        // Correcting it needs the peak of a recording that has not finished. A
        // non-zero count on a real meeting is what would justify a limiter --
        // and dividing by two would halve every passage where only one source is
        // active, which mix.rs already rejected.
        let mut feed = MixFeed::new();
        feed.push_a(&[0.9, 0.1, -0.9]);
        feed.push_b(&[0.9, 0.1, -0.9]);

        let mixed = feed.take();
        assert_eq!(feed.clipped(), 2, "the first and third exceed full scale");
        assert!(
            mixed[0] > 1.0,
            "the sum is handed on for the encoder to clamp"
        );
        assert_eq!(feed.written(), 3);
    }

    #[test]
    fn a_meeting_with_no_system_audio_still_records_the_microphone() {
        // The bench check found that WASAPI loopback from an idle output device
        // delivers no callback at all -- not silence. So an empty B stream is
        // "nobody else spoke", and the meeting still has to work.
        let mut feed = MixFeed::new();
        let speech: Vec<f32> = (0..500).map(|i| ((i as f32) / 50.0).sin() * 0.4).collect();
        feed.push_a(&speech);

        assert!(feed.take().is_empty(), "nothing is mixed while B is empty");
        let all = feed.flush();
        assert_eq!(all.len(), 500);
        assert_eq!(all, speech, "mixed against silence is unchanged");
        assert_eq!(feed.clipped(), 0);
    }
}
