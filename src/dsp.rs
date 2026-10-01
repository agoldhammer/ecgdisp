//! Signal-processing building blocks for the ECG analysis, matching the
//! scipy/numpy routines the reference Python analyzer (`ekgdata`) uses:
//! Butterworth design as second-order sections (`scipy.signal.butter`),
//! zero-phase filtering (`sosfiltfilt`), `find_peaks` with height and
//! distance, and numpy's `gradient`, `median`, `percentile` and
//! `convolve(mode="same")`.

use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy, PartialEq)]
struct C {
    re: f64,
    im: f64,
}

const fn c(re: f64, im: f64) -> C {
    C { re, im }
}

impl C {
    fn sqrt(self) -> C {
        let r = self.re.hypot(self.im).sqrt();
        let th = self.im.atan2(self.re) / 2.0;
        c(r * th.cos(), r * th.sin())
    }
}

impl Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        c(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        c(self.re - o.re, self.im - o.im)
    }
}

impl Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        c(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Div for C {
    type Output = C;
    fn div(self, o: C) -> C {
        let d = o.re * o.re + o.im * o.im;
        c(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

impl Neg for C {
    type Output = C;
    fn neg(self) -> C {
        c(-self.re, -self.im)
    }
}

fn prod(v: &[C]) -> C {
    v.iter().fold(c(1.0, 0.0), |a, &b| a * b)
}

/// One second-order section `[b0, b1, b2, 1, a1, a2]`.
pub type Sos = [f64; 6];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Band {
    Lowpass(f64),
    Highpass(f64),
    Bandpass(f64, f64),
}

/// Digital Butterworth filter of `order` with cutoffs in Hz, as cascaded
/// second-order sections (gain in the first section).
pub fn butter(order: usize, band: Band, fs: f64) -> Vec<Sos> {
    // Analog prototype: poles on the left unit semicircle, no zeros.
    let n = order as f64;
    let proto: Vec<C> = (0..order)
        .map(|i| {
            let m = -n + 1.0 + 2.0 * i as f64;
            let th = PI * m / (2.0 * n);
            -c(th.cos(), th.sin())
        })
        .collect();
    // Pre-warp for the bilinear transform at fs = 2 (scipy's convention).
    let warp = |f: f64| 4.0 * (PI * (2.0 * f / fs) / 2.0).tan();
    let (mut z, mut p, mut k): (Vec<C>, Vec<C>, f64);
    match band {
        Band::Lowpass(f) => {
            let wo = warp(f);
            z = vec![];
            p = proto.iter().map(|&q| q * c(wo, 0.0)).collect();
            k = wo.powi(order as i32);
        }
        Band::Highpass(f) => {
            let wo = warp(f);
            z = vec![c(0.0, 0.0); order];
            p = proto.iter().map(|&q| c(wo, 0.0) / q).collect();
            k = (c(1.0, 0.0) / prod(&proto.iter().map(|&q| -q).collect::<Vec<_>>())).re;
        }
        Band::Bandpass(lo, hi) => {
            let (wl, wh) = (warp(lo), warp(hi));
            let (wo, bw) = ((wl * wh).sqrt(), wh - wl);
            let wo2 = c(wo * wo, 0.0);
            p = Vec::with_capacity(2 * order);
            let half: Vec<C> = proto.iter().map(|&q| q * c(bw / 2.0, 0.0)).collect();
            for &q in &half {
                p.push(q + (q * q - wo2).sqrt());
            }
            for &q in &half {
                p.push(q - (q * q - wo2).sqrt());
            }
            z = vec![c(0.0, 0.0); order];
            k = bw.powi(order as i32);
        }
    }
    // Bilinear transform; zeros at infinity map to Nyquist (-1).
    let fs2 = c(4.0, 0.0);
    let degree = p.len() - z.len();
    k *= (prod(&z.iter().map(|&x| fs2 - x).collect::<Vec<_>>())
        / prod(&p.iter().map(|&x| fs2 - x).collect::<Vec<_>>()))
    .re;
    z = z.iter().map(|&x| (fs2 + x) / (fs2 - x)).collect();
    z.extend(std::iter::repeat_n(c(-1.0, 0.0), degree));
    p = p.iter().map(|&x| (fs2 + x) / (fs2 - x)).collect();

    let quads = |roots: Vec<C>, n_sections: usize| -> Vec<[f64; 3]> {
        // Complex roots with their conjugates, then real roots in pairs,
        // padded with roots at the origin.
        let mut out = vec![];
        let mut real = vec![];
        for r in roots {
            if r.im > 1e-12 {
                out.push([1.0, -2.0 * r.re, r.re * r.re + r.im * r.im]);
            } else if r.im.abs() <= 1e-12 {
                real.push(r.re);
            }
        }
        real.resize(real.len().max(2 * n_sections - 2 * out.len()), 0.0);
        for pair in real.chunks(2) {
            out.push([1.0, -(pair[0] + pair[1]), pair[0] * pair[1]]);
        }
        out
    };
    let n_sections = p.len().max(z.len()).div_ceil(2);
    let (bs, as_) = (quads(z, n_sections), quads(p, n_sections));
    bs.iter()
        .zip(&as_)
        .enumerate()
        .map(|(i, (b, a))| {
            let g = if i == 0 { k } else { 1.0 };
            [g * b[0], g * b[1], g * b[2], a[0], a[1], a[2]]
        })
        .collect()
}

/// Run `x` through the sections (direct form II transposed), starting
/// from state `zi`, which is left as the final state.
fn sosfilt(sos: &[Sos], x: &mut [f64], zi: &mut [[f64; 2]]) {
    for v in x {
        let mut s = *v;
        for (sec, z) in sos.iter().zip(zi.iter_mut()) {
            let y = sec[0] * s + z[0];
            z[0] = sec[1] * s - sec[4] * y + z[1];
            z[1] = sec[2] * s - sec[5] * y;
            s = y;
        }
        *v = s;
    }
}

/// Per-section initial state for a unit step (scipy's `sosfilt_zi`).
fn sosfilt_zi(sos: &[Sos]) -> Vec<[f64; 2]> {
    let mut scale = 1.0;
    sos.iter()
        .map(|s| {
            let (b, a) = ([s[0], s[1], s[2]], [s[4], s[5]]);
            // Solve [[1 + a1, -1], [a2, 1]] zi = [b1 - a1 b0, b2 - a2 b0].
            let (r0, r1) = (b[1] - a[0] * b[0], b[2] - a[1] * b[0]);
            let z0 = (r0 + r1) / (1.0 + a[0] + a[1]);
            let z1 = r1 - a[1] * z0;
            let zi = [scale * z0, scale * z1];
            scale *= b.iter().sum::<f64>() / (1.0 + a[0] + a[1]);
            zi
        })
        .collect()
}

/// Forward-backward (zero-phase) filtering with odd extension at the ends,
/// like `scipy.signal.sosfiltfilt` with its default padding.
pub fn sosfiltfilt(sos: &[Sos], x: &[f64]) -> Vec<f64> {
    let n = x.len();
    let zero_b2 = sos.iter().filter(|s| s[2] == 0.0).count();
    let zero_a2 = sos.iter().filter(|s| s[5] == 0.0).count();
    let ntaps = 2 * sos.len() + 1 - zero_b2.min(zero_a2);
    let edge = (3 * ntaps).min(n.saturating_sub(1));
    if n == 0 {
        return vec![];
    }
    let mut ext = Vec::with_capacity(n + 2 * edge);
    ext.extend((1..=edge).rev().map(|i| 2.0 * x[0] - x[i]));
    ext.extend_from_slice(x);
    ext.extend((1..=edge).map(|i| 2.0 * x[n - 1] - x[n - 1 - i]));

    let zi = sosfilt_zi(sos);
    let run = |v: &mut [f64]| {
        let v0 = v[0];
        let mut state: Vec<[f64; 2]> = zi.iter().map(|z| [z[0] * v0, z[1] * v0]).collect();
        sosfilt(sos, v, &mut state);
    };
    run(&mut ext);
    ext.reverse();
    run(&mut ext);
    ext.reverse();
    ext[edge..edge + n].to_vec()
}

/// Filter a signal as `neurokit2.signal_filter(method="butterworth")` does.
/// Non-finite samples are bridged by linear interpolation first.
pub fn filter(x: &[f64], fs: f64, band: Band, order: usize) -> Vec<f64> {
    sosfiltfilt(&butter(order, band, fs), &fill_gaps(x))
}

/// Replace non-finite samples by linear interpolation between their finite
/// neighbours (held constant past the ends; all zero if none are finite).
pub fn fill_gaps(x: &[f64]) -> Vec<f64> {
    let known: Vec<usize> = (0..x.len()).filter(|&i| x[i].is_finite()).collect();
    if known.len() == x.len() {
        return x.to_vec();
    }
    let (Some(&first), Some(&last)) = (known.first(), known.last()) else {
        return vec![0.0; x.len()];
    };
    let mut out = x.to_vec();
    out[..first].fill(x[first]);
    out[last..].fill(x[last]);
    for w in known.windows(2) {
        let (a, b) = (w[0], w[1]);
        for (i, v) in out.iter_mut().enumerate().take(b).skip(a + 1) {
            *v = x[a] + (x[b] - x[a]) * (i - a) as f64 / (b - a) as f64;
        }
    }
    out
}

/// `numpy.gradient`: central differences inside, one-sided at the ends.
pub fn gradient(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return vec![0.0; n];
    }
    (0..n)
        .map(|i| match i {
            0 => x[1] - x[0],
            i if i == n - 1 => x[n - 1] - x[n - 2],
            i => (x[i + 1] - x[i - 1]) / 2.0,
        })
        .collect()
}

/// Moving average of width `k`, aligned like `numpy.convolve(x, ones(k)/k, "same")`.
pub fn moving_average(x: &[f64], k: usize) -> Vec<f64> {
    let n = x.len() as isize;
    let shift = (k as isize - 1) / 2;
    (0..n)
        .map(|i| {
            let hi = (i + shift).min(n - 1);
            let lo = (i + shift - k as isize + 1).max(0);
            (lo..=hi).map(|j| x[j as usize]).sum::<f64>() / k as f64
        })
        .collect()
}

/// `numpy.median`; NaN for an empty slice.
pub fn median(x: &[f64]) -> f64 {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// `numpy.percentile` with linear interpolation, `q` in 0..=100.
pub fn percentile(x: &[f64], q: f64) -> f64 {
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return f64::NAN;
    }
    let pos = (v.len() - 1) as f64 * q / 100.0;
    let (i, frac) = (pos.floor() as usize, pos.fract());
    if i + 1 < v.len() {
        v[i] + (v[i + 1] - v[i]) * frac
    } else {
        v[i]
    }
}

/// Index of the first maximum (`numpy.argmax`); 0 for an empty slice.
pub fn argmax(x: &[f64]) -> usize {
    let mut best = 0;
    for (i, &v) in x.iter().enumerate() {
        if v > x[best] {
            best = i;
        }
    }
    best
}

/// Index of the first minimum (`numpy.argmin`); 0 for an empty slice.
pub fn argmin(x: &[f64]) -> usize {
    let mut best = 0;
    for (i, &v) in x.iter().enumerate() {
        if v < x[best] {
            best = i;
        }
    }
    best
}

/// `scipy.signal.find_peaks(x, height=height, distance=distance)`: local
/// maxima (plateaus count once, at their middle) at least `height` high,
/// thinned so no two are closer than `distance`, keeping the taller one.
pub fn find_peaks(x: &[f64], height: f64, distance: usize) -> Vec<usize> {
    let mut peaks = vec![];
    let n = x.len();
    let mut i = 1;
    while i + 1 < n {
        if x[i - 1] < x[i] {
            let mut ahead = i + 1;
            while ahead < n - 1 && x[ahead] == x[i] {
                ahead += 1;
            }
            if x[ahead] < x[i] {
                peaks.push((i + ahead - 1) / 2);
                i = ahead;
            }
        }
        i += 1;
    }
    peaks.retain(|&p| x[p] >= height);

    let mut keep = vec![true; peaks.len()];
    let mut order: Vec<usize> = (0..peaks.len()).collect();
    order.sort_by(|&a, &b| x[peaks[b]].total_cmp(&x[peaks[a]]));
    for j in order {
        if !keep[j] {
            continue;
        }
        for k in (0..j).rev() {
            if peaks[j] - peaks[k] >= distance {
                break;
            }
            keep[k] = false;
        }
        for k in j + 1..peaks.len() {
            if peaks[k] - peaks[j] >= distance {
                break;
            }
            keep[k] = false;
        }
    }
    peaks
        .into_iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64], tol: f64) {
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert!((x - y).abs() <= tol, "[{i}]: {x} vs {y}");
        }
    }

    /// Overall transfer function of the sections at z = e^{jw}.
    fn response(sos: &[Sos], f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let z1 = c(w.cos(), -w.sin());
        let z2 = z1 * z1;
        sos.iter()
            .map(|s| {
                let num = c(s[0], 0.0) + c(s[1], 0.0) * z1 + c(s[2], 0.0) * z2;
                let den = c(s[3], 0.0) + c(s[4], 0.0) * z1 + c(s[5], 0.0) * z2;
                let h = num / den;
                h.re.hypot(h.im)
            })
            .product()
    }

    // Reference coefficients from scipy.signal.butter(..., output="sos", fs=500).
    #[test]
    fn butter_matches_scipy_responses() {
        let hp = butter(5, Band::Highpass(0.5), 500.0);
        assert_eq!(hp.len(), 3);
        let bp = butter(3, Band::Bandpass(5.0, 30.0), 500.0);
        assert_eq!(bp.len(), 3);
        let lp = butter(4, Band::Lowpass(40.0), 500.0);
        assert_eq!(lp.len(), 2);
        let half = 0.5f64.sqrt();
        for (sos, f, want) in [
            (&hp, 0.5, half),
            (&hp, 50.0, 1.0),
            (&bp, 5.0, half),
            (&bp, 30.0, half),
            (&lp, 40.0, half),
            (&lp, 1.0, 1.0),
        ] {
            assert!((response(sos, f, 500.0) - want).abs() < 1e-6, "{f} Hz");
        }
        assert!(response(&hp, 0.0, 500.0) < 1e-9);
        assert!(response(&bp, 0.0, 500.0) < 1e-9);
        assert!(response(&bp, 249.0, 500.0) < 1e-3);
        // scipy: butter(4, 40, fs=500, output="sos")[0, :3]
        close(
            &lp[0][..3],
            &[
                0.00223489169808233,
                0.00446978339616465,
                0.00223489169808233,
            ],
            1e-15,
        );
    }

    #[test]
    fn filtfilt_matches_scipy() {
        let x: Vec<f64> = (0..60)
            .map(|i| (i as f64 * 0.7).sin() + i as f64 * 0.05)
            .collect();
        let pick = |y: Vec<f64>| [0, 1, 30, 58, 59].map(|i| y[i]);
        // scipy.signal.sosfiltfilt(butter(..., output="sos", fs=500), x)[[0, 1, 30, 58, 59]]
        close(
            &pick(filter(&x, 500.0, Band::Highpass(0.5), 5)),
            &[
                -0.44501833434378346,
                0.22276743547945732,
                0.9776661266142159,
                0.6733423854473641,
                -0.0029097671515205814,
            ],
            1e-9,
        );
        close(
            &pick(filter(&x, 500.0, Band::Bandpass(5.0, 30.0), 3)),
            &[
                0.08749669743815289,
                0.08863446605729192,
                -0.04690924595723385,
                0.16207766165401277,
                0.054950749936035825,
            ],
            1e-9,
        );
    }

    #[test]
    fn filtfilt_passes_dc_through_lowpass_and_removes_it_with_highpass() {
        let x = vec![2.5; 400];
        close(&filter(&x, 500.0, Band::Lowpass(40.0), 4), &x, 1e-9);
        close(
            &filter(&x, 500.0, Band::Highpass(0.5), 5),
            &[0.0; 400],
            1e-9,
        );
    }

    #[test]
    fn filtfilt_has_zero_phase() {
        // A 5 Hz sine through a 40 Hz low-pass keeps its peaks in place.
        let x: Vec<f64> = (0..1000)
            .map(|i| (2.0 * PI * 5.0 * i as f64 / 500.0).sin())
            .collect();
        let y = filter(&x, 500.0, Band::Lowpass(40.0), 4);
        close(&y[100..900], &x[100..900], 1e-3);
    }

    #[test]
    fn numpy_helpers() {
        close(&gradient(&[1.0, 2.0, 4.0, 7.0]), &[1.0, 1.5, 2.5, 3.0], 0.0);
        // numpy.convolve([1,2,3,4,5], ones(2)/2, "same") == [0.5, 1.5, 2.5, 3.5, 4.5]
        close(
            &moving_average(&[1.0, 2.0, 3.0, 4.0, 5.0], 2),
            &[0.5, 1.5, 2.5, 3.5, 4.5],
            1e-12,
        );
        // ... and with ones(3)/3: [1, 2, 3, 4, 3]
        close(
            &moving_average(&[1.0, 2.0, 3.0, 4.0, 5.0], 3),
            &[1.0, 2.0, 3.0, 4.0, 3.0],
            1e-12,
        );
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), 2.5);
        assert_eq!(percentile(&[0.0, 10.0], 99.5), 9.95);
        assert_eq!(argmax(&[1.0, 3.0, 3.0]), 1);
        assert_eq!(argmin(&[1.0, -3.0, -3.0]), 1);
        close(
            &fill_gaps(&[f64::NAN, 1.0, f64::NAN, 3.0, f64::NAN]),
            &[1.0, 1.0, 2.0, 3.0, 3.0],
            0.0,
        );
    }

    #[test]
    fn find_peaks_applies_height_distance_and_plateaus() {
        let x = [
            0.0, 1.0, 0.0, 5.0, 0.0, 2.0, 2.0, 2.0, 0.0, 0.5, 0.0, 4.0, 0.0,
        ];
        assert_eq!(find_peaks(&x, 0.0, 1), vec![1, 3, 6, 9, 11]);
        assert_eq!(find_peaks(&x, 1.5, 1), vec![3, 6, 11]);
        // The 5 at 3 suppresses everything within 3 samples; the 4 at 11 survives.
        assert_eq!(find_peaks(&x, 0.0, 4), vec![3, 11]);
    }
}
