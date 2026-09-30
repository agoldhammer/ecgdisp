//! GUI-independent geometry: time-axis ticks, time/pixel mapping, cursor
//! sample lookup and vertical scaling of the traces.

/// Spacing of the light grid lines, in milliseconds.
pub const MINOR_TICK_MS: u64 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TickLevel {
    /// Every 25 ms: light tick and grid line.
    Minor,
    /// Every 0.5 s: bolder.
    Half,
    /// Every 1.0 s: boldest.
    Second,
}

/// All tick positions (seconds) from 0 up to and including `duration_s`.
/// Computed in integer milliseconds so that levels are exact.
pub fn time_ticks(duration_s: f64) -> Vec<(f64, TickLevel)> {
    if duration_s.is_nan() || duration_s < 0.0 {
        return Vec::new();
    }
    let duration_ms = (duration_s * 1000.0).round() as u64;
    (0..=duration_ms / MINOR_TICK_MS)
        .map(|k| {
            let ms = k * MINOR_TICK_MS;
            let level = if ms.is_multiple_of(1000) {
                TickLevel::Second
            } else if ms.is_multiple_of(500) {
                TickLevel::Half
            } else {
                TickLevel::Minor
            };
            (ms as f64 / 1000.0, level)
        })
        .collect()
}

/// Linear mapping between time (s) and horizontal pixel position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeAxis {
    pub left: f32,
    pub right: f32,
    pub duration_s: f64,
}

impl TimeAxis {
    pub fn time_to_x(&self, t: f64) -> f32 {
        self.left + ((t / self.duration_s) as f32) * (self.right - self.left)
    }

    /// Inverse of [`time_to_x`](Self::time_to_x); `None` outside the axis.
    pub fn x_to_time(&self, x: f32) -> Option<f64> {
        if !(self.left..=self.right).contains(&x) || self.right <= self.left {
            return None;
        }
        Some(f64::from((x - self.left) / (self.right - self.left)) * self.duration_s)
    }
}

/// Index of the sample nearest to time `t`, or `None` if `t` lies outside
/// the recording (with half a sample period of slack at each end).
pub fn sample_index(t: f64, fs: f64, n_samples: usize) -> Option<usize> {
    if n_samples == 0 || !t.is_finite() {
        return None;
    }
    let idx = (t * fs).round();
    if idx < 0.0 {
        return None;
    }
    let idx = idx as usize;
    if idx < n_samples {
        Some(idx)
    } else if t * fs <= n_samples as f64 {
        Some(n_samples - 1) // cursor on the right edge of the last sample
    } else {
        None
    }
}

/// Vertical range (mV) of one trace strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub lo: f32,
    pub hi: f32,
}

impl Range {
    /// Map a value to a y pixel within `[top, bottom]` (screen y grows downwards).
    pub fn value_to_y(&self, v: f32, top: f32, bottom: f32) -> f32 {
        bottom - (v - self.lo) / (self.hi - self.lo) * (bottom - top)
    }
}

/// Smallest vertical span shown, so flat traces are not blown up.
pub const MIN_SPAN_MV: f32 = 1.0;
/// Fraction of the span added as headroom above and below the trace.
pub const PADDING: f32 = 0.05;

/// One range per trace. Every strip gets the same mV span (the largest
/// needed by any trace) so amplitudes are comparable across leads; each is
/// centred on its own trace. NaN samples are ignored.
pub fn strip_ranges(traces: &[&[f32]]) -> Vec<Range> {
    let extents: Vec<Option<(f32, f32)>> = traces
        .iter()
        .map(|s| {
            s.iter()
                .filter(|v| v.is_finite())
                .fold(None, |acc, &v| match acc {
                    None => Some((v, v)),
                    Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
                })
        })
        .collect();
    let span = extents
        .iter()
        .flatten()
        .map(|(lo, hi)| hi - lo)
        .fold(MIN_SPAN_MV, f32::max)
        * (1.0 + 2.0 * PADDING);
    extents
        .into_iter()
        .map(|e| {
            let mid = e.map_or(0.0, |(lo, hi)| (lo + hi) / 2.0);
            Range {
                lo: mid - span / 2.0,
                hi: mid + span / 2.0,
            }
        })
        .collect()
}

/// Index ranges of consecutive finite samples, so a trace can be drawn as
/// separate polylines with gaps where samples are missing (NaN).
pub fn finite_runs(samples: &[f32]) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, v) in samples.iter().enumerate() {
        match (v.is_finite(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push(s..samples.len());
    }
    runs
}

/// Cursor read-out for one trace, e.g. `+0.235 mV`.
pub fn format_value(v: f32, units: &str) -> String {
    if v.is_finite() {
        format!("{v:+.3} {units}")
    } else {
        "no data".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_runs_split_at_nan() {
        let n = f32::NAN;
        assert_eq!(
            finite_runs(&[1.0, 2.0, n, 3.0, n, n, 4.0, 5.0]),
            vec![0..2, 3..4, 6..8]
        );
        assert_eq!(finite_runs(&[n, 1.0]), vec![1..2]);
        assert!(finite_runs(&[n, n]).is_empty());
        assert!(finite_runs(&[]).is_empty());
    }

    #[test]
    fn formats_cursor_values() {
        assert_eq!(format_value(0.2346, "mV"), "+0.235 mV");
        assert_eq!(format_value(-1.0, "mV"), "-1.000 mV");
        assert_eq!(format_value(f32::NAN, "mV"), "no data");
    }

    #[test]
    fn ticks_every_25ms_over_ten_seconds() {
        let ticks = time_ticks(10.0);
        assert_eq!(ticks.len(), 401);
        assert_eq!(ticks.first(), Some(&(0.0, TickLevel::Second)));
        assert_eq!(ticks.last(), Some(&(10.0, TickLevel::Second)));
        assert!(
            ticks
                .windows(2)
                .all(|w| ((w[1].0 - w[0].0) - 0.025).abs() < 1e-12)
        );
    }

    #[test]
    fn tick_levels_follow_half_and_whole_seconds() {
        let ticks = time_ticks(10.0);
        let count = |lvl| ticks.iter().filter(|t| t.1 == lvl).count();
        assert_eq!(count(TickLevel::Second), 11);
        assert_eq!(count(TickLevel::Half), 10);
        assert_eq!(count(TickLevel::Minor), 401 - 21);
        let level_at = |t: f64| ticks.iter().find(|x| (x.0 - t).abs() < 1e-9).unwrap().1;
        assert_eq!(level_at(0.025), TickLevel::Minor);
        assert_eq!(level_at(0.5), TickLevel::Half);
        assert_eq!(level_at(1.0), TickLevel::Second);
        assert_eq!(level_at(7.5), TickLevel::Half);
        assert_eq!(level_at(7.475), TickLevel::Minor);
    }

    #[test]
    fn ticks_handle_degenerate_durations() {
        assert_eq!(time_ticks(0.0), vec![(0.0, TickLevel::Second)]);
        assert!(time_ticks(-1.0).is_empty());
        assert!(time_ticks(f64::NAN).is_empty());
        assert_eq!(time_ticks(0.06).len(), 3); // 0, 25, 50 ms
    }

    #[test]
    fn time_axis_round_trips() {
        let axis = TimeAxis {
            left: 100.0,
            right: 1100.0,
            duration_s: 10.0,
        };
        assert_eq!(axis.time_to_x(0.0), 100.0);
        assert_eq!(axis.time_to_x(10.0), 1100.0);
        assert_eq!(axis.time_to_x(2.5), 350.0);
        assert_eq!(axis.x_to_time(350.0), Some(2.5));
        assert_eq!(axis.x_to_time(99.0), None);
        assert_eq!(axis.x_to_time(1101.0), None);
    }

    #[test]
    fn sample_index_rounds_and_clamps() {
        assert_eq!(sample_index(0.0, 500.0, 5000), Some(0));
        assert_eq!(sample_index(0.0011, 500.0, 5000), Some(1));
        assert_eq!(sample_index(0.0009, 500.0, 5000), Some(0));
        assert_eq!(sample_index(9.998, 500.0, 5000), Some(4999));
        assert_eq!(sample_index(10.0, 500.0, 5000), Some(4999));
        assert_eq!(sample_index(10.01, 500.0, 5000), None);
        assert_eq!(sample_index(-0.01, 500.0, 5000), None);
        assert_eq!(sample_index(1.0, 500.0, 0), None);
    }

    #[test]
    fn range_maps_values_to_pixels() {
        let r = Range { lo: -1.0, hi: 1.0 };
        assert_eq!(r.value_to_y(-1.0, 0.0, 100.0), 100.0);
        assert_eq!(r.value_to_y(1.0, 0.0, 100.0), 0.0);
        assert_eq!(r.value_to_y(0.0, 0.0, 100.0), 50.0);
    }

    #[test]
    fn strip_ranges_share_span_and_centre_each_trace() {
        let a = [0.0, 2.0];
        let b = [-0.5, 0.5, f32::NAN];
        let r = strip_ranges(&[&a, &b]);
        let span = 2.0 * (1.0 + 2.0 * PADDING);
        for x in &r {
            assert!((x.hi - x.lo - span).abs() < 1e-6);
        }
        assert!(((r[0].lo + r[0].hi) / 2.0 - 1.0).abs() < 1e-6);
        assert!(((r[1].lo + r[1].hi) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn strip_ranges_enforce_minimum_span_for_flat_or_empty_traces() {
        let flat = [0.3; 10];
        let empty: [f32; 0] = [];
        let r = strip_ranges(&[&flat, &empty]);
        let span = MIN_SPAN_MV * (1.0 + 2.0 * PADDING);
        assert!((r[0].hi - r[0].lo - span).abs() < 1e-6);
        assert!(((r[0].lo + r[0].hi) / 2.0 - 0.3).abs() < 1e-6);
        assert_eq!((r[1].lo + r[1].hi) / 2.0, 0.0);
    }
}
