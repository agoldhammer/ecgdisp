//! Automated 12-lead measurements, ported from the `ekg-analyze` script in
//! `../ekgdata` (`src/ekgdata/analyze.py`): QRS detection across all leads,
//! heart rate and RR statistics, PR/QRS/QT from a multi-lead median beat,
//! QRS axis and LVH voltage criteria. Estimates, not a clinical read.

use crate::dsp::{self, Band};
use crate::wfdb::Record;

/// Median-beat window around each R peak, in seconds.
const PRE: f64 = 0.25;
const POST: f64 = 0.45;

/// `int(s * fs)` as Python computes it (truncating toward zero).
fn samples(s: f64, fs: f64) -> i64 {
    (s * fs) as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Intervals {
    /// `None` when the QRS starts too early in the window to search for a P wave.
    pub pr_ms: Option<f64>,
    pub qrs_ms: f64,
    /// The QRS end ran into the ST segment (e.g. bundle branch block), so
    /// `qrs_ms` is a lower bound.
    pub qrs_wide: bool,
    pub qt_ms: f64,
    pub qtc_bazett_ms: f64,
    pub qtc_fridericia_ms: f64,
}

/// LVH voltage criteria from median-beat R and S amplitudes, mV.
#[derive(Debug, Clone, PartialEq)]
pub struct Lvh {
    /// SV1 + max(RV5, RV6), > 3.5 mV.
    pub sokolow_lyon: f64,
    /// RaVL + SV3, > 2.8 mV (men) / 2.0 mV (women).
    pub cornell: f64,
    /// RaVL, > 1.1 mV.
    pub ravl: f64,
}

pub const SOKOLOW_LYON_MV: f64 = 3.5;
pub const RAVL_MV: f64 = 1.1;

impl Lvh {
    pub fn cornell_threshold(sex: Option<Sex>) -> f64 {
        match sex {
            Some(Sex::Female) => 2.0,
            // Unknown sex: only flag what is positive for either.
            Some(Sex::Male) | None => 2.8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Sample index of each detected QRS complex.
    pub qrs: Vec<usize>,
    pub hr_bpm: f64,
    pub rr_mean_ms: f64,
    /// Population standard deviation of the RR intervals.
    pub rr_sd_ms: f64,
    pub rr_min_ms: f64,
    pub rr_max_ms: f64,
    pub rmssd_ms: f64,
    /// `None` when no beat has a complete median-beat window.
    pub intervals: Option<Intervals>,
    /// Frontal QRS axis in degrees, when leads I and aVF are present.
    pub qrs_axis_deg: Option<f64>,
    /// When leads V1, V3, V5, V6 and aVL are present.
    pub lvh: Option<Lvh>,
    pub cautions: Vec<String>,
}

/// Spatial slope magnitude of the band-passed leads, smoothed over 100 ms:
/// one hump per QRS even when the complex is notched or wide.
fn qrs_energy(band: &[Vec<f64>], fs: f64) -> Vec<f64> {
    let grads: Vec<Vec<f64>> = band.iter().map(|l| dsp::gradient(l)).collect();
    let e: Vec<f64> = (0..band[0].len())
        .map(|t| grads.iter().map(|g| g[t] * g[t]).sum::<f64>().sqrt())
        .collect();
    dsp::moving_average(&e, samples(0.1, fs).max(1) as usize)
}

fn detect_qrs(energy: &[f64], fs: f64) -> Vec<usize> {
    let height = 0.35 * dsp::percentile(energy, 99.5);
    dsp::find_peaks(energy, height, samples(0.3, fs).max(1) as usize)
}

fn unique(mut v: Vec<usize>) -> Vec<usize> {
    v.sort_unstable();
    v.dedup();
    v
}

/// Snap each detection to the QRS energy maximum within ±80 ms.
fn align(r: &[usize], energy: &[f64], fs: f64) -> Vec<usize> {
    let h = samples(0.08, fs) as usize;
    unique(
        r.iter()
            .map(|&p| {
                let lo = p.saturating_sub(h);
                let hi = (p + h).min(energy.len());
                lo + dsp::argmax(&energy[lo..hi])
            })
            .collect(),
    )
}

/// Shift each beat (±40 ms) to best match the median QRS template across all leads.
fn refine(mut r: Vec<usize>, band: &[Vec<f64>], fs: f64) -> Vec<usize> {
    let (a, b, h) = (
        samples(0.1, fs) as usize,
        samples(0.15, fs) as usize,
        samples(0.04, fs) as usize,
    );
    let n = band[0].len();
    let ok = |p: usize| p >= a + h && p + b + h <= n;
    for _ in 0..2 {
        let good: Vec<usize> = r.iter().copied().filter(|&p| ok(p)).collect();
        if good.is_empty() {
            return r;
        }
        // tmpl[lead][t] = median over beats of band[lead][p - a + t]
        let tmpl: Vec<Vec<f64>> = band
            .iter()
            .map(|lead| {
                (0..a + b)
                    .map(|t| {
                        let vals: Vec<f64> = good.iter().map(|&p| lead[p - a + t]).collect();
                        dsp::median(&vals)
                    })
                    .collect()
            })
            .collect();
        r = unique(
            r.iter()
                .map(|&p| {
                    if !ok(p) {
                        return p;
                    }
                    let score: Vec<f64> = (0..=2 * h)
                        .map(|s| {
                            let start = p + s - h - a;
                            band.iter()
                                .zip(&tmpl)
                                .map(|(lead, tm)| {
                                    lead[start..start + a + b]
                                        .iter()
                                        .zip(tm)
                                        .map(|(x, y)| x * y)
                                        .sum::<f64>()
                                })
                                .sum()
                        })
                        .collect();
                    p + dsp::argmax(&score) - h
                })
                .collect(),
        );
    }
    r
}

/// Median beat per lead around the QRS positions, baselined on the PR
/// segment; `None` when no beat has a complete window.
fn median_beats(sig: &[Vec<f64>], r: &[usize], fs: f64) -> Option<Vec<Vec<f64>>> {
    let (w0, w1) = (samples(PRE, fs) as usize, samples(POST, fs) as usize);
    let n = sig[0].len();
    let starts: Vec<usize> = r
        .iter()
        .filter(|&&p| p >= w0 && p + w1 <= n)
        .map(|&p| p - w0)
        .collect();
    if starts.is_empty() {
        return None;
    }
    let (b0, b1) = (
        w0 - samples(0.08, fs) as usize,
        w0 - samples(0.05, fs) as usize,
    );
    Some(
        sig.iter()
            .map(|lead| {
                let beat: Vec<f64> = (0..w0 + w1)
                    .map(|t| dsp::median(&starts.iter().map(|&s| lead[s + t]).collect::<Vec<_>>()))
                    .collect();
                let base = dsp::median(&beat[b0..b1]);
                beat.iter().map(|v| v - base).collect()
            })
            .collect(),
    )
}

/// Sample indices within the median-beat window.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fiducials {
    p_on: Option<i64>,
    qrs_on: usize,
    qrs_off: usize,
    t_end: i64,
    wide: bool,
}

/// P onset, QRS on/off and T end from all leads of the median beat.
fn fiducials(beats: &[Vec<f64>], fs: f64) -> Fiducials {
    let len = beats[0].len();
    let w0 = samples(PRE, fs);
    let at = |s: f64| (w0 + samples(s, fs)).clamp(0, len as i64) as usize;
    let (q0, q1) = (at(-0.05), at(0.06));

    let grads: Vec<Vec<f64>> = beats.iter().map(|b| dsp::gradient(b)).collect();
    let slope: Vec<f64> = (0..len)
        .map(|t| grads.iter().map(|g| g[t] * g[t]).sum::<f64>().sqrt() * fs)
        .collect();
    // 5% tracks QRS width on synthetic beats of 65-130 ms.
    let thr = 0.05 * slope[q0..q1].iter().copied().fold(f64::MIN, f64::max);
    let search = at(-0.10);
    let on = search + slope[search..q1].iter().position(|&s| s > thr).unwrap_or(0);
    let end = at(0.12).max(on);
    let off = on + slope[on..end].iter().rposition(|&s| s > thr).unwrap_or(0);
    // Slope still high at the search limit: QRS runs straight into ST (e.g. bundle branch block).
    let wide = off + 2 >= at(0.12);

    let mag: Vec<f64> = (0..len)
        .map(|t| beats.iter().map(|b| b[t] * b[t]).sum::<f64>().sqrt())
        .collect();
    let tpk = at(0.15) + dsp::argmax(&mag[at(0.15)..at(0.42)]);
    // T end: tangent at the steepest downslope after the T peak, extended to baseline.
    let seg = &mag[tpk..at(0.44).max(tpk)];
    let g = dsp::gradient(seg);
    let k = dsp::argmin(&g);
    let floor = mag[at(0.43)..].iter().copied().fold(f64::MAX, f64::min);
    let t_end = match g.get(k) {
        // Like the Python original, also when the T wave still rises at the
        // end of the window (gk > 0); only a flat one has no tangent.
        Some(&gk) if gk != 0.0 => {
            tpk as i64 + k as i64 + ((seg[k] - floor) / -gk).round_ties_even() as i64
        }
        _ => (tpk + k) as i64,
    };

    let p_lo = at(-0.24);
    let p_hi = (on as i64 - samples(0.02, fs)).clamp(0, len as i64) as usize;
    let p_on = (p_hi > p_lo).then(|| {
        let p = &mag[p_lo..p_hi];
        let head = &p[..p.len().min(10)];
        let rest = head.iter().sum::<f64>() / head.len() as f64;
        let peak = p.iter().copied().fold(f64::MIN, f64::max);
        (p_lo
            + p.iter()
                .position(|&v| v > rest + 0.2 * (peak - rest))
                .unwrap_or(0)) as i64
    });
    Fiducials {
        p_on,
        qrs_on: on,
        qrs_off: off,
        t_end,
        wide,
    }
}

/// Measure `record`, detecting QRS complexes on all of its leads.
pub fn analyze(record: &Record) -> Result<Analysis, String> {
    let fs = record.fs;
    if record.leads.is_empty() || record.n_samples() == 0 {
        return Err("no signal to analyze".into());
    }
    let raw: Vec<Vec<f64>> = record
        .leads
        .iter()
        .map(|l| l.samples.iter().map(|&v| f64::from(v)).collect())
        .collect();
    let nyq = fs / 2.0 - 1.0;
    // High-pass only: low-pass smoothing flattens R waves and would bias
    // every voltage criterion low.
    let clean: Vec<Vec<f64>> = raw
        .iter()
        .map(|x| dsp::filter(x, fs, Band::Highpass(0.5), 5))
        .collect();
    let band: Vec<Vec<f64>> = raw
        .iter()
        .map(|x| dsp::filter(x, fs, Band::Bandpass(5.0, nyq.min(30.0)), 3))
        .collect();
    let energy = qrs_energy(&band, fs);
    let r = refine(align(&detect_qrs(&energy, fs), &energy, fs), &band, fs);
    if r.len() < 3 {
        return Err(format!("only {} QRS complexes found", r.len()));
    }
    let rr: Vec<f64> = r
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64 / fs * 1000.0)
        .collect();
    let mean = rr.iter().sum::<f64>() / rr.len() as f64;
    let sd = (rr.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rr.len() as f64).sqrt();
    let rmssd = {
        let d: Vec<f64> = rr.windows(2).map(|w| (w[1] - w[0]).powi(2)).collect();
        if d.is_empty() {
            0.0
        } else {
            (d.iter().sum::<f64>() / d.len() as f64).sqrt()
        }
    };
    let rr_min = rr.iter().copied().fold(f64::MAX, f64::min);
    let rr_max = rr.iter().copied().fold(f64::MIN, f64::max);

    let mut cautions = vec![];
    if sd / mean > 0.1 {
        cautions.push("irregular rhythm (RR CV > 10%): median beat and PR unreliable".into());
    }
    if rr_min / 1000.0 < PRE + POST {
        cautions.push(format!(
            "shortest RR {rr_min:.0} ms is inside the {:.0} ms beat window: T / QT may overlap the next beat",
            (PRE + POST) * 1000.0
        ));
    }

    // Timing from a 40 Hz low-passed beat (steadier boundaries); amplitudes from `clean`.
    let smooth: Vec<Vec<f64>> = clean
        .iter()
        .map(|x| dsp::filter(x, fs, Band::Lowpass(nyq.min(40.0)), 4))
        .collect();
    let mut intervals = None;
    let mut qrs_axis_deg = None;
    let mut lvh = None;
    if let (Some(timing), Some(beats)) =
        (median_beats(&smooth, &r, fs), median_beats(&clean, &r, fs))
    {
        let f = fiducials(&timing, fs);
        let ms = |a: i64, b: i64| (b - a) as f64 / fs * 1000.0;
        let (on, off) = (f.qrs_on as i64, f.qrs_off as i64);
        let qt = ms(on, f.t_end);
        let rr_s = dsp::median(&rr) / 1000.0;
        intervals = Some(Intervals {
            pr_ms: f.p_on.map(|p| ms(p, on)),
            qrs_ms: ms(on, off),
            qrs_wide: f.wide,
            qt_ms: qt,
            qtc_bazett_ms: qt / rr_s.sqrt(),
            qtc_fridericia_ms: qt / rr_s.cbrt(),
        });

        let qrs = f.qrs_on..f.qrs_off + 1;
        let lead = |name: &str| {
            record
                .leads
                .iter()
                .position(|l| l.name.eq_ignore_ascii_case(name))
                .map(|i| &beats[i][qrs.clone()])
        };
        let r_amp = |w: &[f64]| w.iter().copied().fold(f64::MIN, f64::max);
        let s_amp = |w: &[f64]| w.iter().copied().fold(f64::MAX, f64::min);
        if let (Some(i), Some(avf)) = (lead("I"), lead("AVF")) {
            let sum = |w: &[f64]| w.iter().sum::<f64>();
            qrs_axis_deg = Some(sum(avf).atan2(sum(i)).to_degrees());
        }
        if let (Some(v1), Some(v3), Some(v5), Some(v6), Some(avl)) =
            (lead("V1"), lead("V3"), lead("V5"), lead("V6"), lead("AVL"))
        {
            lvh = Some(Lvh {
                sokolow_lyon: -s_amp(v1) + r_amp(v5).max(r_amp(v6)),
                cornell: r_amp(avl) - s_amp(v3),
                ravl: r_amp(avl),
            });
        }
    }

    Ok(Analysis {
        qrs: r,
        hr_bpm: 60000.0 / mean,
        rr_mean_ms: mean,
        rr_sd_ms: sd,
        rr_min_ms: rr_min,
        rr_max_ms: rr_max,
        rmssd_ms: rmssd,
        intervals,
        qrs_axis_deg,
        lvh,
        cautions,
    })
}

/// One piece of the measurement read-out; `alert` marks a criterion met
/// or a caution, drawn in a warning colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub text: String,
    pub alert: bool,
}

fn item(text: String, alert: bool) -> Item {
    Item { text, alert }
}

impl Analysis {
    /// Rhythm, intervals and axis, for the first read-out line.
    pub fn summary(&self) -> Vec<Item> {
        let mut out = vec![
            item(format!("HR {:.0} bpm", self.hr_bpm), false),
            item(
                format!(
                    "RR {:.0} ± {:.0} ms ({} beats)",
                    self.rr_mean_ms,
                    self.rr_sd_ms,
                    self.qrs.len()
                ),
                false,
            ),
        ];
        if let Some(iv) = &self.intervals {
            let ge = if iv.qrs_wide { "≥" } else { "" };
            if let Some(pr) = iv.pr_ms {
                out.push(item(format!("PR {pr:.0} ms"), false));
            }
            out.push(item(format!("QRS {ge}{:.0} ms", iv.qrs_ms), iv.qrs_wide));
            out.push(item(format!("QT {:.0} ms", iv.qt_ms), false));
            out.push(item(
                format!(
                    "QTc {:.0} (Bazett) / {:.0} (Fridericia) ms",
                    iv.qtc_bazett_ms, iv.qtc_fridericia_ms
                ),
                false,
            ));
        }
        if let Some(a) = self.qrs_axis_deg {
            out.push(item(format!("QRS axis {a:.0}°"), false));
        }
        out
    }

    /// LVH voltage criteria and cautions, for the second read-out line.
    pub fn voltage(&self, sex: Option<Sex>) -> Vec<Item> {
        let mut out = vec![];
        if let Some(l) = &self.lvh {
            let cornell_thr = match sex {
                Some(Sex::Male) => "2.8".to_owned(),
                Some(Sex::Female) => "2.0".to_owned(),
                None => "2.8 M / 2.0 F".to_owned(),
            };
            out.push(item(
                format!(
                    "Sokolow-Lyon SV1+R(V5|V6) {:.2} mV (>{SOKOLOW_LYON_MV})",
                    l.sokolow_lyon
                ),
                l.sokolow_lyon > SOKOLOW_LYON_MV,
            ));
            out.push(item(
                format!("Cornell RaVL+SV3 {:.2} mV (>{cornell_thr})", l.cornell),
                l.cornell > Lvh::cornell_threshold(sex),
            ));
            out.push(item(
                format!("RaVL {:.2} mV (>{RAVL_MV})", l.ravl),
                l.ravl > RAVL_MV,
            ));
        }
        out.extend(
            self.cautions
                .iter()
                .map(|c| item(format!("caution: {c}"), true)),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfdb::Lead;

    const FS: f64 = 500.0;
    const LEADS: [&str; 12] = [
        "I", "II", "III", "AVR", "AVL", "AVF", "V1", "V2", "V3", "V4", "V5", "V6",
    ];
    /// Frontal-plane angle of each limb lead, degrees.
    const LIMB_ANGLE: [f64; 6] = [0.0, 60.0, 120.0, -150.0, -30.0, 90.0];
    const PRECORDIAL: [f64; 6] = [-0.8, 0.3, 0.8, 1.2, 1.4, 1.0];

    /// One PQRST complex (mV) at time t (s) relative to the R peak; w widens the QRS.
    /// Nominal truth at w=1: PR ~170 ms, QRS ~65 ms, QT ~390 ms (as in the Python tests).
    fn beat(t: f64, w: f64) -> f64 {
        let g = |mu: f64, sd: f64, a: f64| a * (-0.5 * ((t - mu) / sd).powi(2)).exp();
        g(-0.160, 0.015, 0.15)
            + g(-0.020 * w, 0.006 * w, -0.10)
            + g(0.0, 0.008 * w, 1.0)
            + g(0.020 * w, 0.006 * w, -0.25)
            + g(0.260, 0.040, 0.30)
    }

    /// Small deterministic noise in ±5 µV.
    fn noise(i: usize, lead: usize) -> f64 {
        let x = ((i * 12 + lead) as u64).wrapping_mul(6364136223846793005);
        ((x >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.01
    }

    /// 12-lead record; limb leads project a frontal vector at `axis` degrees.
    fn synth(rr_ms: &[f64], axis: f64, precordial: [f64; 6], qrs_width: f64) -> Record {
        let mut r_times = vec![0.5];
        for rr in rr_ms {
            r_times.push(r_times.last().unwrap() + rr / 1000.0);
        }
        let n = ((r_times.last().unwrap() + 1.0) * FS) as usize;
        let base: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / FS;
                r_times.iter().map(|rt| beat(t - rt, qrs_width)).sum()
            })
            .collect();
        let gains: Vec<f64> = LIMB_ANGLE
            .iter()
            .map(|a| (axis - a).to_radians().cos())
            .chain(precordial)
            .collect();
        Record {
            name: "synthetic".into(),
            fs: FS,
            leads: LEADS
                .iter()
                .zip(&gains)
                .enumerate()
                .map(|(li, (name, g))| Lead {
                    name: (*name).into(),
                    units: "mV".into(),
                    samples: base
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (v * g + noise(i, li)) as f32)
                        .collect(),
                })
                .collect(),
        }
    }

    fn regular() -> Record {
        synth(&[1000.0; 9], 60.0, PRECORDIAL, 1.0)
    }

    #[test]
    fn regular_rhythm() {
        let a = analyze(&regular()).unwrap();
        assert_eq!(a.qrs.len(), 10);
        assert_eq!(a.hr_bpm.round(), 60.0);
        assert!(a.rr_sd_ms < 3.0, "{}", a.rr_sd_ms);
        assert!(a.cautions.is_empty(), "{:?}", a.cautions);
        // R peaks at 0.5 s + k s.
        for (k, &p) in a.qrs.iter().enumerate() {
            assert!((p as i64 - 250 - 500 * k as i64).abs() <= 3, "{p}");
        }
    }

    #[test]
    fn intervals_match_synthetic_truth() {
        let iv = analyze(&regular()).unwrap().intervals.unwrap();
        let pr = iv.pr_ms.unwrap();
        assert!((140.0..=190.0).contains(&pr), "PR {pr}");
        assert!((55.0..=80.0).contains(&iv.qrs_ms), "QRS {}", iv.qrs_ms);
        assert!((360.0..=420.0).contains(&iv.qt_ms), "QT {}", iv.qt_ms);
        assert!(!iv.qrs_wide);
        // At 60 bpm the corrected QT equals QT.
        assert!((iv.qtc_bazett_ms - iv.qt_ms).abs() <= 2.0);
    }

    #[test]
    fn wide_qrs() {
        let a = analyze(&synth(&[900.0; 9], 60.0, PRECORDIAL, 2.0)).unwrap();
        let iv = a.intervals.unwrap();
        assert!((115.0..=150.0).contains(&iv.qrs_ms), "QRS {}", iv.qrs_ms);
        assert!(!iv.qrs_wide);
    }

    #[test]
    fn qrs_axis() {
        for axis in [-45.0, 0.0, 60.0, 100.0] {
            let a = analyze(&synth(&[900.0; 9], axis, PRECORDIAL, 1.0)).unwrap();
            let got = a.qrs_axis_deg.unwrap();
            assert!((got - axis).abs() <= 8.0, "axis {axis}: {got}");
        }
    }

    #[test]
    fn lvh_voltages() {
        // V1 = -1.5 x template -> S ~1.5 mV; V5 R 2.5 mV; so Sokolow-Lyon ~4.0.
        let rec = synth(&[900.0; 9], 60.0, [-1.5, 0.3, -1.2, 1.2, 2.5, 1.0], 1.0);
        let lvh = analyze(&rec).unwrap().lvh.unwrap();
        assert!((lvh.sokolow_lyon - 4.0).abs() < 0.1, "{}", lvh.sokolow_lyon);
        let ravl = 90f64.to_radians().cos(); // aVL gain at the 60° axis
        assert!((lvh.ravl - ravl).abs() < 0.05, "{}", lvh.ravl);
        // V3 = -1.2 x template: its inverted R is an S of ~1.2 mV.
        assert!(
            (lvh.cornell - (lvh.ravl + 1.2)).abs() < 0.1,
            "{}",
            lvh.cornell
        );
    }

    #[test]
    fn amplitudes_not_attenuated() {
        // R in V5 is 1.4 x a 1.0 mV R wave; filtering must not shave the sharp peak.
        let lvh = analyze(&regular()).unwrap().lvh.unwrap();
        // V1 = -0.8 x template: S ~0.8; V5 R ~1.4.
        assert!((lvh.sokolow_lyon - 2.2).abs() < 0.1, "{}", lvh.sokolow_lyon);
    }

    #[test]
    fn irregular_rhythm_warns() {
        let rr = [800.0, 1200.0, 700.0, 1100.0, 900.0, 1300.0, 750.0, 1000.0];
        let a = analyze(&synth(&rr, 60.0, PRECORDIAL, 1.0)).unwrap();
        assert_eq!(a.qrs.len(), 9);
        assert!(a.cautions.iter().any(|c| c.contains("irregular rhythm")));
    }

    #[test]
    fn fast_rate_warns() {
        let a = analyze(&synth(&[450.0; 20], 60.0, PRECORDIAL, 1.0)).unwrap();
        assert_eq!(a.qrs.len(), 21);
        assert_eq!(a.hr_bpm.round(), 133.0);
        assert!(
            a.cautions
                .iter()
                .any(|c| c.contains("inside the 700 ms beat window"))
        );
    }

    #[test]
    fn flat_signal_is_an_error() {
        let mut rec = regular();
        for l in &mut rec.leads {
            l.samples.fill(0.0);
        }
        assert!(analyze(&rec).unwrap_err().contains("QRS complexes"));
    }

    #[test]
    fn missing_leads_skip_axis_and_lvh() {
        let mut rec = regular();
        rec.leads.retain(|l| l.name == "II" || l.name == "V1");
        let a = analyze(&rec).unwrap();
        assert_eq!(a.qrs.len(), 10);
        assert!(a.intervals.is_some());
        assert_eq!(a.qrs_axis_deg, None);
        assert_eq!(a.lvh, None);
    }

    #[test]
    fn align_snaps_to_energy_peak() {
        let mut energy = vec![0.0; 1000];
        energy[200] = 1.0;
        energy[600] = 1.0;
        assert_eq!(align(&[190, 230, 610], &energy, FS), vec![200, 600]);
    }

    #[test]
    fn readout_flags_met_criteria_and_cautions() {
        let a = Analysis {
            qrs: vec![0; 11],
            hr_bpm: 64.2,
            rr_mean_ms: 938.0,
            rr_sd_ms: 16.4,
            rr_min_ms: 900.0,
            rr_max_ms: 960.0,
            rmssd_ms: 20.0,
            intervals: Some(Intervals {
                pr_ms: Some(156.0),
                qrs_ms: 190.0,
                qrs_wide: true,
                qt_ms: 372.0,
                qtc_bazett_ms: 462.0,
                qtc_fridericia_ms: 430.0,
            }),
            qrs_axis_deg: Some(-23.4),
            lvh: Some(Lvh {
                sokolow_lyon: 4.47,
                cornell: 2.3,
                ravl: 0.5,
            }),
            cautions: vec!["something".into()],
        };
        let texts = |v: Vec<Item>| v.into_iter().map(|i| (i.text, i.alert)).collect::<Vec<_>>();
        assert_eq!(
            texts(a.summary()),
            [
                ("HR 64 bpm".into(), false),
                ("RR 938 ± 16 ms (11 beats)".into(), false),
                ("PR 156 ms".into(), false),
                ("QRS ≥190 ms".into(), true),
                ("QT 372 ms".into(), false),
                ("QTc 462 (Bazett) / 430 (Fridericia) ms".into(), false),
                ("QRS axis -23°".into(), false),
            ]
        );
        let v = texts(a.voltage(None));
        assert_eq!(
            v[0],
            ("Sokolow-Lyon SV1+R(V5|V6) 4.47 mV (>3.5)".into(), true)
        );
        assert_eq!(
            v[1],
            ("Cornell RaVL+SV3 2.30 mV (>2.8 M / 2.0 F)".into(), false)
        );
        assert_eq!(v[2], ("RaVL 0.50 mV (>1.1)".into(), false));
        assert_eq!(v[3], ("caution: something".into(), true));
        // A woman's Cornell threshold is 2.0 mV.
        assert_eq!(
            texts(a.voltage(Some(Sex::Female)))[1],
            ("Cornell RaVL+SV3 2.30 mV (>2.0)".into(), true)
        );
    }
}
