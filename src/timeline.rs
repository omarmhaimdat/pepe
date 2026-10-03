use std::collections::VecDeque;
use std::time::Duration;

use crate::metrics::Histogram;
use crate::response::ResponseStats;

/// Width of one point on the dashboard's time-series charts
pub const BUCKET: Duration = Duration::from_secs(1);
/// Points kept: ten minutes of history at one per second
const KEEP: usize = 600;
/// Width of one point of the long series, kept for the whole run: a day
/// is 1,440 of them
pub const MINUTE: Duration = Duration::from_secs(60);

/// Latency bins for the heatmap: log-spaced, 24 per decade from 10µs to 100s,
/// so each bin is about 10% wide
pub const BINS_PER_DECADE: usize = 24;
pub const BINS: usize = BINS_PER_DECADE * 7;
const LOWEST_US: f64 = 10.0;

/// Request counts per latency bin for one bucket
pub type LatencyBins = [u32; BINS];

/// Heatmap bin for a latency in microseconds
pub fn bin_of(us: u64) -> usize {
    let us = us as f64;
    if us <= LOWEST_US {
        return 0;
    }
    (((us / LOWEST_US).log10() * BINS_PER_DECADE as f64) as usize).min(BINS - 1)
}

/// Lower edge of heatmap bin `bin`, in microseconds
pub fn bin_floor_us(bin: usize) -> f64 {
    LOWEST_US * 10f64.powf(bin as f64 / BINS_PER_DECADE as f64)
}

/// One closed bucket of the timeline
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// End of the bucket, in seconds since the run started
    pub at: f64,
    pub rps: f64,
    /// Non-2xx responses and failed requests per second
    pub errors: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
}

/// Per-second series for the live charts. One histogram is reused for the
/// open bucket, so memory stays flat however long the run is.
#[derive(Debug, Default)]
pub struct Timeline {
    samples: VecDeque<Sample>,
    /// Heatmap column for each sample, kept apart so `Sample` stays small
    bins: VecDeque<LatencyBins>,
    current: Histogram,
    current_bins: Option<Box<LatencyBins>>,
    errors: u64,
    /// Start of the open bucket, relative to the run start
    bucket_start: Duration,
    /// One point per minute for as long as the run goes, for soak runs:
    /// the per-second points above only reach back ten minutes
    minutes: Vec<Sample>,
    minute: Histogram,
    minute_errors: u64,
    minute_start: Duration,
}

impl Timeline {
    pub fn record(&mut self, stat: &ResponseStats) {
        let us = stat.duration.as_micros() as u64;
        self.current.record(us);
        self.minute.record(us);
        self.current_bins.get_or_insert_with(|| Box::new([0; BINS]))[bin_of(us)] += 1;
        if !stat.status_code.is_some_and(|code| code.is_success()) {
            self.errors += 1;
            self.minute_errors += 1;
        }
    }

    /// Close every bucket that ended by `now` (time since the run started)
    pub fn advance(&mut self, now: Duration) {
        while now >= self.bucket_start + BUCKET {
            self.close(BUCKET);
        }
    }

    /// Close the last, partial bucket once the run is over
    pub fn finish(&mut self, now: Duration) {
        self.advance(now);
        let rest = now.saturating_sub(self.bucket_start);
        // A sliver of a bucket would show as a spike or a dip; drop it
        if rest >= BUCKET / 4 && self.current.count() > 0 {
            self.close(rest);
        }
        // The last, partial minute counts if it saw a full second or more
        let rest = self.bucket_start.saturating_sub(self.minute_start);
        if rest >= BUCKET && self.minute.count() > 0 {
            self.close_minute(rest);
        }
    }

    fn close_minute(&mut self, length: Duration) {
        let secs = length.as_secs_f64();
        let ms = |q| self.minute.percentile(q) as f64 / 1000.0;
        self.minutes.push(Sample {
            at: (self.minute_start + length).as_secs_f64(),
            rps: self.minute.count() as f64 / secs,
            errors: self.minute_errors as f64 / secs,
            p50_ms: ms(50.0),
            p90_ms: ms(90.0),
            p99_ms: ms(99.0),
        });
        self.minute.clear();
        self.minute_errors = 0;
        self.minute_start += length;
    }

    fn close(&mut self, length: Duration) {
        let secs = length.as_secs_f64();
        let ms = |q| self.current.percentile(q) as f64 / 1000.0;
        let sample = Sample {
            at: (self.bucket_start + length).as_secs_f64(),
            rps: self.current.count() as f64 / secs,
            errors: self.errors as f64 / secs,
            p50_ms: ms(50.0),
            p90_ms: ms(90.0),
            p99_ms: ms(99.0),
        };
        if self.samples.len() == KEEP {
            self.samples.pop_front();
            self.bins.pop_front();
        }
        self.samples.push_back(sample);
        let bins = self.current_bins.get_or_insert_with(|| Box::new([0; BINS]));
        self.bins.push_back(**bins);
        bins.fill(0);
        self.current.clear();
        self.errors = 0;
        self.bucket_start += length;
        if self.bucket_start >= self.minute_start + MINUTE {
            self.close_minute(MINUTE);
        }
    }

    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }

    /// One point per minute since the run started, oldest first
    pub fn minutes(&self) -> &[Sample] {
        &self.minutes
    }

    /// The series that covers the whole run: the minutes once there are
    /// enough of them to say anything, the seconds before that
    pub fn whole_run(&self) -> Vec<Sample> {
        if self.minutes.len() >= 3 {
            self.minutes.clone()
        } else {
            self.samples.iter().copied().collect()
        }
    }

    pub fn last(&self) -> Option<&Sample> {
        self.samples.back()
    }

    /// Latency bins, one per sample, oldest first
    pub fn bins(&self) -> &VecDeque<LatencyBins> {
        &self.bins
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn stat(ms: u64, status: u16) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_millis(ms),
            status_code: Some(StatusCode::from_u16(status).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn buckets_close_once_per_second() {
        let mut t = Timeline::default();
        for _ in 0..10 {
            t.record(&stat(10, 200));
        }
        t.record(&stat(100, 500));
        t.advance(Duration::from_millis(900));
        assert!(t.samples().is_empty());

        t.advance(Duration::from_millis(1_100));
        let s = *t.last().unwrap();
        assert_eq!((s.at, s.rps, s.errors), (1.0, 11.0, 1.0));
        // Histogram buckets are within 1% of the true value
        assert!((9.9..=10.1).contains(&s.p50_ms), "p50={}", s.p50_ms);
        assert!((99.0..=101.0).contains(&s.p99_ms), "p99={}", s.p99_ms);

        let bins = t.bins().back().unwrap();
        assert_eq!(bins[bin_of(10_000)], 10);
        assert_eq!(bins[bin_of(100_000)], 1);

        // Quiet seconds still produce (empty) points, so the chart shows gaps
        t.advance(Duration::from_millis(3_000));
        assert_eq!(t.samples().len(), 3);
        assert_eq!(t.bins().len(), 3);
        assert!(t.bins().back().unwrap().iter().all(|&n| n == 0));
        assert_eq!(t.last().unwrap().rps, 0.0);
    }

    #[test]
    fn finish_keeps_a_meaningful_partial_bucket() {
        let mut t = Timeline::default();
        t.record(&stat(1, 200));
        t.finish(Duration::from_millis(500));
        let s = *t.last().unwrap();
        assert_eq!((s.at, s.rps), (0.5, 2.0));

        let mut t = Timeline::default();
        t.record(&stat(1, 200));
        t.finish(Duration::from_millis(100));
        assert!(t.samples().is_empty());
    }

    #[test]
    fn minutes_cover_the_whole_run() {
        let mut t = Timeline::default();
        // Two and a half minutes: ten requests a second, one in ten failing
        for second in 0..150u64 {
            for i in 0..10 {
                t.record(&stat(
                    if i == 0 { 100 } else { 10 },
                    if i == 0 { 500 } else { 200 },
                ));
            }
            t.advance(Duration::from_secs(second + 1));
        }
        assert_eq!(t.minutes().len(), 2);
        let first = t.minutes()[0];
        assert_eq!((first.at, first.rps, first.errors), (60.0, 10.0, 1.0));
        assert!((9.9..=10.1).contains(&first.p50_ms));
        assert!((99.0..=101.0).contains(&first.p99_ms));
        assert_eq!(t.minutes()[1].at, 120.0);
        // The partial last minute is kept once the run is over
        t.finish(Duration::from_secs(150));
        assert_eq!(t.minutes().len(), 3);
        assert_eq!(t.minutes()[2].at, 150.0);
        assert_eq!(t.minutes()[2].rps, 10.0);
        // Seconds are bounded, minutes are not
        assert_eq!(t.samples().len(), 150);
        assert_eq!(t.whole_run().len(), 3, "the minutes once there are three");
        let short = Timeline::default();
        assert!(short.whole_run().is_empty());
    }

    #[test]
    fn history_is_bounded() {
        let mut t = Timeline::default();
        t.advance(Duration::from_secs(KEEP as u64 + 50));
        assert_eq!(t.samples().len(), KEEP);
        assert_eq!(t.bins().len(), KEEP);
        assert_eq!(t.samples().front().unwrap().at, 51.0);
    }

    #[test]
    fn bins_are_log_spaced() {
        assert_eq!(bin_of(0), 0);
        assert_eq!(bin_of(10), 0);
        assert_eq!(bin_of(100), BINS_PER_DECADE);
        assert_eq!(bin_of(1_000), 2 * BINS_PER_DECADE);
        assert_eq!(bin_of(u64::MAX), BINS - 1);
        for us in [37u64, 1_234, 56_789, 3_000_000] {
            let b = bin_of(us);
            assert!(bin_floor_us(b) <= us as f64 * 1.000_001 && (us as f64) < bin_floor_us(b + 1));
        }
    }
}
