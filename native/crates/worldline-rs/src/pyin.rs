//! pYIN F0 estimator — port of libpyin (`pyin.c` / `yin.c` /
//! `math-funcs.c`) plus the one libgvps routine it uses
//! (`gvps_sparse_sampled_hidden_static`).
//!
//! The reference `WorldlineRenderer` estimates F0 with pYIN for any sample
//! that ships no `.frq` file (see `FrqEstimator` vs `PyinEstimator`). The
//! bundled banks always ship `.frq`, but an imported bank may not — without
//! this port the Rust engine would fall back to DIO and drift from the
//! `.so` path.
//!
//! The gvps Viterbi decoder is a macro-generated family in C
//! (`gvps_viterbi.hc`); only the `SAMPLED + STATIC + HIDDEN`, non-circular,
//! sparse variant is instantiated here, matching the reference call:
//!
//! ```c
//! gvps_sparse_sampled_hidden_static(pint, &param, smtdesc.nq, obsrv,
//!     ptransition_same, ptransition_diff, fntran, 4);
//! ```
//!
//! Every array in this module is finite (the C uses calloc-zeroed storage),
//! so indices are bounds-checked by construction.

/// libpyin `FP_TYPE` and `EPS` (`common.h`).
const EPS: f64 = 0.0000000001;

/// `yincorr` threshold "increase the probability of a candidate" step.
const VALLEY_STEP: f64 = 0.01;

/// `pyin_config` — the reference's `pyin_init` defaults.
#[derive(Debug, Clone, Copy)]
pub struct PyinConfig {
    pub fmin: f64,
    pub fmax: f64,
    pub nq: i32,
    pub w: i32,
    pub beta_a: f64,
    pub beta_u: f64,
    pub threshold: f64,
    pub bias: f64,
    pub ptrans: f64,
    pub trange: i32,
    pub nf: i32,
    pub nhop: i32,
}

impl PyinConfig {
    /// `pyin_init(nhop)`.
    pub fn new(nhop: i32) -> Self {
        PyinConfig {
            fmin: 50.0,
            fmax: 800.0,
            nq: 480,
            w: 300,
            beta_a: 1.7,
            beta_u: 0.2,
            threshold: 0.02,
            bias: 1.0,
            trange: 12,
            ptrans: 0.003,
            nf: 1024,
            nhop,
        }
    }
}

/// `pyin_semitone_wrapper` (`math-funcs.h`).
#[derive(Debug, Clone, Copy)]
struct SemitoneWrapper {
    a: f64,
    b: f64,
    nq: i32,
    fconst: f64,
}

const FCONST: f64 = 127.09;
const NQDEFAULT: i32 = 480;

/// `pyin_wrapper_from_frange`.
fn wrapper_from_frange(fmin: f64, fmax: f64) -> SemitoneWrapper {
    let k1 = (fmax / FCONST).log2();
    let k2 = (fmin / FCONST).log2();
    SemitoneWrapper {
        a: k1 - k2,
        b: k2 / (k2 - k1),
        nq: NQDEFAULT,
        fconst: FCONST,
    }
}

/// `pyin_semitone_from_freq` (floored, as the C returns `int`).
fn semitone_from_freq(w: &SemitoneWrapper, freq: f64) -> i32 {
    let semitone_fp = (freq / w.fconst).log2() * f64::from(w.nq) / w.a + w.b * f64::from(w.nq);
    semitone_fp.floor() as i32
}

/// `pyin_freq_from_semitone`.
fn freq_from_semitone(w: &SemitoneWrapper, semitone: i32) -> f64 {
    let semitone_fp = f64::from(semitone);
    (2f64).powf((semitone_fp - w.b * f64::from(w.nq)) * w.a / f64::from(w.nq)) * w.fconst
}

/// `pyin_beta_b_from_au`.
fn beta_b_from_au(a: f64, u: f64) -> f64 {
    (a - a * u) / u
}

/// `pyin_normalized_betapdf` — the beta prior over thresholds, with the
/// reference's monotonicity enforcement.
fn normalized_betapdf(a: f64, b: f64, from: f64, to: f64, number: usize) -> Vec<f64> {
    let mut ret = vec![0.0f64; number];
    let gallop = (to - from) / number as f64;
    for (i, item) in ret.iter_mut().enumerate() {
        let x = from + gallop * i as f64;
        *item = x.powf(a - 1.0) * (1.0 - x).powf(b - 1.0);
    }
    // "enforce monotonicity" — walk backwards and clamp upward.
    for i in (1..number.saturating_sub(1)).rev() {
        if ret[i] < ret[i + 1] {
            ret[i] = ret[i + 1];
        }
    }
    let norm: f64 = 1.0 / ret.iter().sum::<f64>();
    for item in ret.iter_mut() {
        *item *= norm;
    }
    ret
}

/// `fetch_frame` — `nf` samples centred on `center`, zero-padded at the edges.
fn fetch_frame(x: &[f64], center: usize, nf: usize) -> Vec<f64> {
    let mut y = vec![0.0f64; nf];
    for (i, item) in y.iter_mut().enumerate() {
        let isrc = center as i64 + i as i64 - (nf / 2) as i64;
        if isrc >= 0 && (isrc as usize) < x.len() {
            *item = x[isrc as usize];
        }
    }
    y
}

/// `diffsum` — squared difference over a window.
fn diffsum(x: &[f64], t: usize, w: usize) -> f64 {
    let mut ret = 0.0;
    for i in 0..w {
        let d = x[i] - x[t + i];
        ret += d * d;
    }
    ret
}

/// `pyin_yincorr` — the cumulative-mean-normalized difference function.
fn yincorr(x: &[f64], w: usize) -> Vec<f64> {
    let nd = x.len() - w;
    let mut d = vec![0.0f64; nd];
    if nd == 0 {
        return d;
    }
    d[0] = 1.0;
    let mut norm_factor = 0.0;
    for i in 1..nd {
        let diff = diffsum(x, i, w);
        norm_factor += diff;
        d[i] = diff * i as f64 / if norm_factor <= EPS { EPS } else { norm_factor };
    }
    d
}

/// `pyin_qinterp` — parabolic interpolation around `x[k]`.
///
/// Returns `(value_at_min, refined_position)`; the C writes the refined
/// position through `y`.
fn qinterp(x: &[f64], k: usize) -> (f64, f64) {
    let a = x[k - 1];
    let b = x[k];
    let c = x[k + 1];
    let a1 = (a + c) / 2.0 - b;
    let a2 = c - b - a1;
    let mut x1 = -a2 / a1 * 0.5;
    // "in case we get some x outside of [k-1, k+1]"
    if !(x1.abs() < 1.0) {
        x1 = 0.0;
    }
    (a1 * x1 * x1 + a2 * x1 + b, k as f64 + x1)
}

/// `find_valleys` — decreasing local minima below a shrinking threshold.
fn find_valleys(x: &[f64], threshold: f64, begin: i64, end: i64) -> Vec<usize> {
    let mut ret = Vec::new();
    let mut threshold = threshold;
    let nx = x.len() as i64;
    let stop = (nx - 1).min(end);
    for i in begin.max(1)..stop {
        let i = i as usize;
        if x[i - 1] > x[i] && x[i + 1] > x[i] && x[i] < threshold {
            threshold = x[i] - VALLEY_STEP;
            ret.push(i);
        }
    }
    ret
}

/// One observation: candidate states with probabilities.
struct ObsrvSlice {
    /// `(state, probability)` pairs, in candidate order.
    pairs: Vec<(i32, f64)>,
}

/// `gvps_sparse_sampled_hidden_static`.
///
/// `dst` receives the winning state per frame (values `>= nstate` mean
/// "unvoiced"); returns the best path log-probability.
fn sparse_sampled_hidden_static(
    dst: &mut [i32],
    param: &PyinConfig,
    nstate: i32,
    slices: &[ObsrvSlice],
    nhiddenprune: i32,
) -> f64 {
    let t_len = slices.len();
    if t_len == 0 {
        return 0.0;
    }
    let nstate = nstate as usize;
    // `#define nstate_at(t) nsparse[t]` / `state_at(t, n)` / `p_at(t, n)`.
    let n_at = |t: usize| slices[t].pairs.len();
    let state_at = |t: usize, n: usize| slices[t].pairs[n].0;
    let p_at = |t: usize, n: usize| slices[t].pairs[n].1;

    let mut a: Vec<Vec<f64>> = Vec::with_capacity(t_len);
    let mut p: Vec<Vec<usize>> = Vec::with_capacity(t_len);
    for t in 0..t_len {
        a.push(vec![0.0f64; n_at(t)]);
        p.push(vec![0usize; n_at(t)]);
    }
    // Hidden state layer (STATIC: one transition table, no time indexing).
    let mut a_h = vec![0.0f64; t_len * nstate];
    let mut p_h = vec![0usize; t_len * nstate];
    let transame_ = {
        let mut v = vec![0.0f64; nstate];
        for (d, item) in v.iter_mut().enumerate() {
            *item = (1.0 - param.ptrans)
                * (1.0 - d as f64 / f64::from(param.trange + 1))
                * f64::from(param.trange + 1);
        }
        v
    };
    let trandiff_ = {
        let mut v = vec![0.0f64; nstate];
        for (d, item) in v.iter_mut().enumerate() {
            *item = param.ptrans
                * (1.0 - d as f64 / f64::from(param.trange + 1))
                * f64::from(param.trange + 1);
        }
        v
    };
    let transame = |d: usize| transame_.get(d).copied().unwrap_or(0.0).ln();
    let trandiff = |d: usize| trandiff_.get(d).copied().unwrap_or(0.0).ln();
    let fntran = |_t: usize| param.trange as usize;

    // initialization
    let mut pexposed = 0.0f64;
    for i in 0..n_at(0) {
        let prob = p_at(0, i);
        a[0][i] = prob.ln();
        pexposed += prob;
    }
    let phidden = ((1.0 - pexposed).max(0.00001)).ln();
    for i in 0..nstate {
        a_h[i] = phidden;
    }

    // DP searching
    for t in 1..t_len {
        let ntrans = fntran(t);
        let ntransh = ntrans.min(nhiddenprune as usize);
        pexposed = 0.0;
        for n in 0..n_at(t) {
            let mut maxp = -30e10f64;
            let mut maxj = 0usize;
            let i = state_at(t, n) as i64;
            // hidden -> exposed (trandiff)
            let lo = (i - ntrans as i64).max(0);
            let hi = (i + ntrans as i64 + 1).min(nstate as i64);
            for j in lo..hi {
                let diff = (i - j).unsigned_abs() as usize;
                let psum = a_h[(t - 1) * nstate + j as usize] + trandiff(diff);
                if psum > maxp {
                    maxp = psum;
                    maxj = (-j - 1) as usize; // encoded hidden state (negative)
                }
            }
            // exposed -> exposed (transame)
            for nprv in 0..n_at(t - 1) {
                let j = state_at(t - 1, nprv) as i64;
                if (i - j).unsigned_abs() as usize > ntrans {
                    continue;
                }
                let diff = (i - j).unsigned_abs() as usize;
                let psum = a[t - 1][nprv] + transame(diff);
                if psum > maxp {
                    maxp = psum;
                    maxj = nprv;
                }
            }
            pexposed += p_at(t, n);
            a[t][n] = maxp + p_at(t, n).ln();
            p[t][n] = maxj;
        }

        let phidden = ((1.0 - pexposed).max(0.00001)).ln();
        // exposed/hidden -> hidden
        for i in 0..nstate {
            let mut maxp = -30e10f64;
            let mut maxj = 0usize;
            // hidden -> hidden (transame)
            let lo = (i as i64 - ntransh as i64).max(0);
            let hi = (i as i64 + ntransh as i64 + 1).min(nstate as i64);
            for j in lo..hi {
                let diff = (i as i64 - j).unsigned_abs() as usize;
                let psum = a_h[(t - 1) * nstate + j as usize] + transame(diff);
                if psum > maxp {
                    maxp = psum;
                    maxj = (-j - 1) as usize;
                }
            }
            // exposed -> hidden (trandiff)
            for nprv in 0..n_at(t - 1) {
                let j = state_at(t - 1, nprv) as i64;
                if (i as i64 - j).unsigned_abs() as usize > ntrans {
                    continue;
                }
                let diff = (i as i64 - j).unsigned_abs() as usize;
                let psum = a[t - 1][nprv] + trandiff(diff);
                if psum > maxp {
                    maxp = psum;
                    maxj = nprv;
                }
            }
            a_h[t * nstate + i] = maxp + phidden;
            p_h[t * nstate + i] = maxj;
        }
    }

    // termination
    let mut optim = -30e10f64;
    let mut maxi: i64 = 0;
    let t = t_len - 1;
    for i in 0..nstate {
        if a_h[t * nstate + i] > optim {
            optim = a_h[t * nstate + i];
            maxi = -(i as i64) - 1;
        }
    }
    for i in 0..n_at(t) {
        if a[t][i] > optim {
            optim = a[t][i];
            maxi = i as i64;
        }
    }

    // backtracking — `dst` holds encoded states (negative = hidden) until
    // the final offset pass.
    let mut enc = vec![0i64; t_len];
    enc[t] = maxi;
    let mut cur = t;
    while cur > 0 {
        let prev = cur - 1;
        let next = enc[cur];
        enc[prev] = if next < 0 {
            p_h[cur * nstate + (-next - 1) as usize] as i64
        } else {
            p[cur][next as usize] as i64
        };
        cur = prev;
    }
    // offset hidden states by nstate, then map pair index -> state
    for t in 0..t_len {
        dst[t] = if enc[t] < 0 {
            nstate as i32 - enc[t] as i32 - 1
        } else {
            state_at(t, enc[t] as usize)
        };
    }
    optim
}

/// `pyin_analyze` — estimate F0 (Hz; 0 = unvoiced) for every hop.
///
/// Mirrors the reference, including its backward voicing extension at
/// unvoiced -> voiced boundaries.
pub fn analyze(param: &PyinConfig, x: &[f64], fs: f64) -> Vec<f64> {
    let nf = param.nf as usize;
    let yin_w = param.w as usize;
    let nhop = param.nhop.max(1) as usize;
    let nfrm = x.len() / nhop;
    if nfrm == 0 || nf <= yin_w {
        return Vec::new();
    }
    let mut smtdesc = wrapper_from_frange(param.fmin, param.fmax);
    smtdesc.nq = param.nq;

    let betapdf = normalized_betapdf(
        param.beta_a,
        beta_b_from_au(param.beta_a, param.beta_u),
        0.0,
        1.0,
        100,
    );

    let mut ret = vec![0.0f64; nfrm];
    let mut slices: Vec<ObsrvSlice> = Vec::with_capacity(nfrm);
    let mut candf: Vec<Vec<f64>> = Vec::with_capacity(nfrm);

    for i in 0..nfrm {
        let mut xfrm = fetch_frame(x, i * nhop, nf);
        let xmean = xfrm.iter().sum::<f64>() / nf as f64;
        for v in xfrm.iter_mut() {
            *v -= xmean;
        }

        let d = yincorr(&xfrm, yin_w);
        let vi = find_valleys(
            &d,
            1.0,
            (fs / param.fmax) as i64,
            (fs / param.fmin) as i64,
        );
        let nv = vi.len();
        let mut pairs: Vec<(i32, f64)> = Vec::with_capacity(nv);
        let mut cand: Vec<f64> = Vec::with_capacity(nv);
        let mut ptotal = 0.0f64;
        for (j, &period) in vi.iter().enumerate() {
            let (_, pinterp) = qinterp(&d, period);
            let freq = fs / pinterp;
            cand.push(freq);
            let bin = semitone_from_freq(&smtdesc, freq);

            let v0 = if j == 0 { 1.0 } else { d[vi[j - 1]] + EPS };
            let v1 = if j == nv - 1 { 0.0 } else { d[vi[j + 1]] + EPS };
            let mut p = 0.0f64;
            let mut k = (v1 * 100.0).floor();
            while k < (v0 * 100.0).floor() {
                let weight = if d[vi[j]] < k / 100.0 { 1.0 } else { 0.01 };
                if let Some(bp) = betapdf.get(k as usize) {
                    p += bp * weight;
                }
                k += 1.0;
            }
            p = if p > 0.99 { 0.99 } else { p };
            p *= param.bias;
            if freq > param.fmax || freq < param.fmin {
                p = EPS;
            }
            pairs.push((bin, p));
            ptotal += p;
        }

        // YIN-correlation emphasis (the reference's "dirty remedy" for
        // near-equal 2×-pitch candidates).
        let mut ptotal_new = 0.0f64;
        const EMPHASIS: f64 = 5.0;
        for (j, pair) in pairs.iter_mut().enumerate() {
            if d[vi[j]] < param.threshold {
                pair.1 *= EMPHASIS;
            }
            ptotal_new += pair.1;
        }
        if ptotal_new != 0.0 {
            for pair in pairs.iter_mut() {
                pair.1 *= ptotal / ptotal_new;
            }
        }

        slices.push(ObsrvSlice { pairs });
        candf.push(cand);
    }

    let mut pint = vec![0i32; nfrm];
    sparse_sampled_hidden_static(&mut pint, param, smtdesc.nq, &slices, 4);

    for i in 0..nfrm {
        if pint[i] >= smtdesc.nq {
            ret[i] = 0.0;
        } else {
            let target = freq_from_semitone(&smtdesc, pint[i]);
            ret[i] = pick_nearest(&candf[i], target);
        }
    }

    // "YIN comes with a slight delay at unvoiced -> voiced boundary. We
    // compensate by extending the voicing region a few frames backward."
    let frame_offset = (nf as f64 / nhop as f64).ceil() as usize;
    for i in 1..nfrm {
        if pint[i] < smtdesc.nq && pint[i - 1] >= smtdesc.nq {
            for j in 1..=frame_offset {
                if i < j {
                    break;
                }
                ret[i - j] = ret[i];
            }
        }
    }

    ret
}

/// `pick_nearest_candidate`.
fn pick_nearest(list: &[f64], x: f64) -> f64 {
    if list.is_empty() {
        return x;
    }
    let mut dist = (list[0] - x).abs();
    let mut idx = 0usize;
    for (i, &value) in list.iter().enumerate().skip(1) {
        if (value - x).abs() < dist {
            dist = (value - x).abs();
            idx = i;
        }
    }
    list[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_matches_the_reference_defaults() {
        let c = PyinConfig::new(441);
        assert_eq!(c.fmin, 50.0);
        assert_eq!(c.fmax, 800.0);
        assert_eq!(c.nq, 480);
        assert_eq!(c.w, 300);
        assert_eq!(c.nf, 1024);
        assert_eq!(c.nhop, 441);
        assert_eq!(c.trange, 12);
    }

    #[test]
    fn semitone_round_trip_is_close() {
        let w = wrapper_from_frange(50.0, 800.0);
        for hz in [60.0, 110.0, 220.0, 440.0, 700.0] {
            let st = semitone_from_freq(&w, hz);
            let back = freq_from_semitone(&w, st);
            // Quantization is 480 steps over the range; a semitone step
            // bounds the round-trip error.
            let step: f64 = 800.0 / 480.0;
            assert!((back - hz).abs() < step.max(hz * 0.06), "{hz} -> {back}");
        }
    }

    #[test]
    fn tracks_a_synthetic_sawtooth() {
        // 220 Hz sawtooth, 2 s — unambiguously voiced and in range.
        let fs = 44100.0;
        let n = 88200;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let phase = (i as f64 * 220.0 / fs).fract();
                2.0 * phase - 1.0
            })
            .collect();
        let cfg = PyinConfig::new(441);
        let f0 = analyze(&cfg, &x, fs);
        assert!(!f0.is_empty(), "no frames produced");
        let voiced: Vec<f64> = f0.iter().copied().filter(|f| *f > 0.0).collect();
        assert!(
            voiced.len() as f64 > f0.len() as f64 * 0.5,
            "mostly unvoiced: {}/{ }",
            voiced.len(),
            f0.len()
        );
        let mean = voiced.iter().sum::<f64>() / voiced.len() as f64;
        assert!((mean - 220.0).abs() < 20.0, "mean f0 {mean:.1} not ~220");
    }

    #[test]
    fn silence_is_unvoiced() {
        let x = vec![0.0f64; 44100];
        let cfg = PyinConfig::new(441);
        let f0 = analyze(&cfg, &x, 44100.0);
        assert!(
            f0.iter().all(|f| *f <= 0.0),
            "silence produced voiced frames"
        );
    }
}
