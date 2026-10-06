//! Rolling throughput math over a monotonically increasing "jobs done" counter.
//! Times are seconds since program start (f64) so this is trivially testable.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub per_sec: f64,
    /// Fewer seconds of history than the window asked for.
    pub partial: bool,
}

#[derive(Debug)]
pub struct Stats {
    /// `(t, adjusted_total)`; adjusted totals never decrease.
    samples: VecDeque<(f64, u64)>,
    history: f64,
    last_raw: Option<u64>,
    /// Added to raw values so a counter reset (worker restart) doesn't look like negative work.
    offset: u64,
    first_total: Option<u64>,
    /// Rates never reach back before this time; see [`Stats::restart_window`].
    floor: f64,
}

impl Stats {
    pub fn new(history_secs: f64) -> Self {
        Self { samples: VecDeque::new(), history: history_secs, last_raw: None, offset: 0, first_total: None, floor: f64::NEG_INFINITY }
    }

    pub fn push(&mut self, t: f64, raw: u64) {
        if let Some(last) = self.last_raw
            && raw < last
        {
            self.offset += last;
        }
        self.last_raw = Some(raw);
        let total = raw + self.offset;
        self.first_total.get_or_insert(total);
        self.samples.push_back((t, total));
        // Keep one sample older than the horizon so a full-history window can still be answered.
        while self.samples.len() > 2 && self.samples[1].0 < t - self.history {
            self.samples.pop_front();
        }
    }

    pub fn now(&self) -> Option<f64> {
        self.samples.back().map(|s| s.0)
    }

    /// Jobs done since the first sample.
    pub fn total_done(&self) -> u64 {
        match (self.samples.back(), self.first_total) {
            (Some(&(_, last)), Some(first)) => last - first,
            _ => 0,
        }
    }

    /// Start rates afresh from the newest sample: until more samples arrive there is no rate,
    /// and afterwards windows are clipped to here (marked partial) instead of averaging in
    /// whatever ran before. Totals are untouched. Call while the work source is idle.
    pub fn restart_window(&mut self) {
        if let Some(now) = self.now() {
            self.floor = now;
        }
    }

    /// Average rate over the trailing `window` seconds ending at the newest sample.
    pub fn rate(&self, window: f64) -> Option<Rate> {
        let &(now, total_now) = self.samples.back()?;
        let last = self.samples.len() - 1;
        let cutoff = now - window;
        let (idx, partial) =
            if cutoff < self.floor { (self.window_start(last, self.floor).0, true) } else { self.window_start(last, cutoff) };
        let (t0, total0) = self.samples[idx];
        let dt = now - t0;
        (dt > 0.0).then(|| Rate { per_sec: (total_now - total0) as f64 / dt, partial })
    }

    /// Newest sample index at or before `cutoff` (searching from `from` backwards), or the oldest.
    fn window_start(&self, from: usize, cutoff: f64) -> (usize, bool) {
        (0..=from).rev().find(|&i| self.samples[i].0 <= cutoff).map_or((0, true), |i| (i, false))
    }

    /// Trailing `window`-second average evaluated at every sample: `(t, jobs/s)`.
    pub fn avg_series(&self, window: f64) -> Vec<(f64, f64)> {
        let mut out = Vec::with_capacity(self.samples.len());
        let mut start = 0;
        for (i, &(t, total)) in self.samples.iter().enumerate().skip(1) {
            while start + 1 < i && self.samples[start + 1].0 <= t - window {
                start += 1;
            }
            let (t0, total0) = self.samples[start];
            if t > t0 {
                out.push((t, (total - total0) as f64 / (t - t0)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// 10 jobs/s for 0..=60s, sampled every second.
    fn steady() -> Stats {
        let mut s = Stats::new(600.0);
        for t in 0..=60 {
            s.push(t as f64, 1000 + t * 10);
        }
        s
    }

    #[test]
    fn windowed_rates() {
        let s = steady();
        let r5 = s.rate(5.0).unwrap();
        assert!(approx(r5.per_sec, 10.0) && !r5.partial);
        let r60 = s.rate(60.0).unwrap();
        assert!(approx(r60.per_sec, 10.0) && !r60.partial);
        let r300 = s.rate(300.0).unwrap();
        assert!(approx(r300.per_sec, 10.0) && r300.partial);
        assert_eq!(s.total_done(), 600);
    }

    #[test]
    fn rate_change_shows_in_short_window_first() {
        let mut s = steady();
        for t in 61..=70u64 {
            s.push(t as f64, 1600 + (t - 60) * 20);
        }
        assert!(approx(s.rate(5.0).unwrap().per_sec, 20.0));
        let r30 = s.rate(30.0).unwrap().per_sec;
        assert!(r30 > 10.0 && r30 < 20.0);
    }

    #[test]
    fn counter_reset_is_rebased() {
        let mut s = Stats::new(600.0);
        s.push(0.0, 100);
        s.push(1.0, 110);
        s.push(2.0, 5); // restart: 5 new jobs since reset
        s.push(3.0, 15);
        let series = s.avg_series(0.0);
        assert!(series.iter().all(|&(_, r)| r >= 0.0));
        assert_eq!(series.iter().map(|&(_, r)| r).collect::<Vec<_>>(), vec![10.0, 5.0, 10.0]);
        assert_eq!(s.total_done(), 25);
    }

    #[test]
    fn history_is_pruned_but_keeps_horizon_anchor() {
        let mut s = Stats::new(10.0);
        for t in 0..=100 {
            s.push(t as f64, t * 2);
        }
        assert!(s.samples.len() <= 12);
        let r = s.rate(10.0).unwrap();
        assert!(approx(r.per_sec, 2.0) && !r.partial);
    }

    #[test]
    fn avg_series_matches_rate() {
        let s = steady();
        let avg = s.avg_series(30.0);
        let &(t, r) = avg.last().unwrap();
        assert!(approx(t, 60.0) && approx(r, s.rate(30.0).unwrap().per_sec));
    }

    #[test]
    fn restart_window_drops_old_work() {
        let mut s = steady(); // 10/s up to t=60
        for t in 61..=70 {
            s.push(t as f64, 1600); // idle
            s.restart_window();
        }
        assert!(s.rate(60.0).is_none(), "idle reads as no rate, not a decaying average");
        for t in 71..=75u64 {
            s.push(t as f64, 1600 + (t - 70) * 3);
        }
        let r = s.rate(60.0).unwrap();
        assert!(approx(r.per_sec, 3.0) && r.partial, "resumed rate ignores the old 10/s");
        assert_eq!(s.total_done(), 615);
    }

    #[test]
    fn single_sample_has_no_rate() {
        let mut s = Stats::new(60.0);
        s.push(0.0, 5);
        assert!(s.rate(5.0).is_none());
        assert!(s.avg_series(0.0).is_empty());
    }
}
