//! Rhythmic oscillators inferred from onset times alone.
//!
//! The authored tempo and bar length are not read here. All stems are pooled into one weighted
//! event process; the distribution of lags between event pairs shows which intervals recur, and
//! each recurring interval becomes an oscillator with its own period, phase, folded template,
//! local strength and confidence.
//!
//! A peak of the lag distribution is kept when it exceeds the largest peak found in any of
//! `surrogates` jittered copies of the track, which keeps the coarse rate of each stem and
//! destroys timing finer than `jitter_s`. A multiple of a shorter kept period is kept only if
//! its fold has structure at harmonics the shorter period does not already contain.

use crate::settings::RhythmSettings;
use std::f64::consts::TAU;
use track::{FPS, Track};

/// Oscillators the bus exposes, ranked from the shortest period.
pub const MAX_OSCILLATORS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RhythmSignal {
    /// Position in the cycle, 0 where onsets concentrate.
    Phase,
    /// Folded onset density at the current phase, 1 at its peak.
    Pulse,
    /// How strongly the events around this moment follow the period, above chance.
    Amp,
    /// 1 minus the family-wise false-positive probability of the period.
    Conf,
}

impl RhythmSignal {
    pub fn parse(name: &str) -> Option<RhythmSignal> {
        Some(match name {
            "phase" => RhythmSignal::Phase,
            "pulse" => RhythmSignal::Pulse,
            "amp" => RhythmSignal::Amp,
            "conf" => RhythmSignal::Conf,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    pub period_s: f64,
    /// Height of the lag-distribution peak in units of the surrogates' spread.
    pub z: f64,
    pub confidence: f64,
    /// Concentration of all events on the period, above chance, 0..1.
    pub strength: f64,
    peak_phase: f64,
    template: Vec<f32>,
    amp: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct Rhythm {
    oscillators: Vec<Oscillator>,
}

type Events = Vec<(f64, f64)>;
type Coeffs = Vec<(f64, f64)>;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Candidate {
    period: f64,
    z: f64,
    conf: f64,
}

/// All stems in one time-sorted list; each stem's events weigh the same in total.
fn pool(stems: &[Vec<f64>]) -> Events {
    let mut ev: Events =
        stems.iter().flat_map(|s| s.iter().map(move |&t| (t, 1.0 / s.len() as f64))).collect();
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    ev
}

fn jitter(stems: &[Vec<f64>], rng: &mut Rng, jitter_s: f64) -> Events {
    let moved: Vec<Vec<f64>> = stems
        .iter()
        .map(|s| s.iter().map(|&t| (t + jitter_s * (2.0 * rng.unit() - 1.0)).max(0.0)).collect())
        .collect();
    pool(&moved)
}

/// Weighted pairwise lag distribution on `bin_s` bins up to bin `bins`, smoothed over three
/// bins and divided by the share of the span in which a pair at that lag can be observed.
fn lag_hist(ev: &[(f64, f64)], bins: usize, span: f64, bin_s: f64) -> Vec<f64> {
    let mut h = vec![0.0; bins + 2];
    for (i, &(ti, wi)) in ev.iter().enumerate() {
        for &(tj, wj) in &ev[i + 1..] {
            let b = ((tj - ti) / bin_s).round() as usize;
            if b > bins {
                break;
            }
            h[b] += wi * wj;
        }
    }
    let mut s = h.clone();
    for b in 1..=bins {
        s[b] = 0.25 * h[b - 1] + 0.5 * h[b] + 0.25 * h[b + 1];
        s[b] /= 1.0 - b as f64 * bin_s / span;
    }
    s
}

fn coeffs(ev: &[(f64, f64)], period: f64, harmonics: usize) -> Coeffs {
    let mut c = vec![(0.0, 0.0); harmonics];
    let mut total = 0.0;
    for &(t, w) in ev {
        total += w;
        let phase = TAU * t / period;
        for (h, x) in c.iter_mut().enumerate() {
            let a = phase * (h + 1) as f64;
            x.0 += w * a.cos();
            x.1 -= w * a.sin();
        }
    }
    if total > 0.0 {
        for x in &mut c {
            x.0 /= total;
            x.1 /= total;
        }
    }
    c
}

/// Concentration over the harmonics that are not multiples of `m` (all of them when `m` <= 1).
fn power(c: &[(f64, f64)], m: usize) -> f64 {
    let sum: f64 = c
        .iter()
        .enumerate()
        .filter(|(i, _)| m <= 1 || (i + 1) % m != 0)
        .map(|(_, x)| x.0 * x.0 + x.1 * x.1)
        .sum();
    (sum / c.len() as f64).sqrt()
}

/// Concentration that `n` unrelated events of these weights would show by chance.
fn chance(ev: &[(f64, f64)]) -> f64 {
    let total: f64 = ev.iter().map(|e| e.1).sum();
    if total <= 0.0 { 1.0 } else { ev.iter().map(|e| e.1 * e.1).sum::<f64>().sqrt() / total }
}

fn excess(p: f64, chance: f64) -> f64 {
    if chance >= 1.0 { 0.0 } else { ((p - chance) / (1.0 - chance)).clamp(0.0, 1.0) }
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1]
}

fn refine(ev: &[(f64, f64)], lag: f64, cfg: &RhythmSettings) -> f64 {
    let steps = (cfg.refine_span_s / cfg.refine_step_s) as i32;
    (-steps..=steps)
        .map(|k| lag + k as f64 * cfg.refine_step_s)
        .map(|t| (t, power(&coeffs(ev, t, cfg.harmonics), 1)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(lag, |x| x.0)
}

fn detect(stems: &[Vec<f64>], ev: &[(f64, f64)], cfg: &RhythmSettings) -> Vec<Candidate> {
    if ev.len() < cfg.min_events {
        return vec![];
    }
    let span = ev[ev.len() - 1].0 - ev[0].0;
    let bins = (span / cfg.min_cycles / cfg.bin_s) as usize;
    let lo = (cfg.min_lag_s / cfg.bin_s).ceil() as usize;
    if bins < lo + 3 {
        return vec![];
    }
    let real = lag_hist(ev, bins, span, cfg.bin_s);
    let mut rng = Rng(cfg.seed);
    let sur: Vec<Events> = (0..cfg.surrogates).map(|_| jitter(stems, &mut rng, cfg.jitter_s)).collect();
    let hists: Vec<Vec<f64>> = sur.iter().map(|e| lag_hist(e, bins, span, cfg.bin_s)).collect();
    let n = cfg.surrogates as f64;
    let mean: Vec<f64> = (0..=bins).map(|b| hists.iter().map(|h| h[b]).sum::<f64>() / n).collect();
    let sd: Vec<f64> = (0..=bins)
        .map(|b| (hists.iter().map(|h| (h[b] - mean[b]).powi(2)).sum::<f64>() / n).sqrt())
        .collect();
    // A bin where every surrogate agrees would otherwise give an unbounded z.
    let floor = 0.5 * sd[lo..=bins].iter().sum::<f64>() / (bins - lo + 1) as f64;
    if floor <= 0.0 {
        return vec![];
    }
    let z = |h: &[f64], b: usize| (h[b] - mean[b]) / sd[b].max(floor);
    let mut null_max: Vec<f64> =
        hists.iter().map(|h| (lo..=bins).map(|b| z(h, b)).fold(f64::MIN, f64::max)).collect();
    null_max.sort_by(f64::total_cmp);
    let threshold = quantile(&null_max, 1.0 - cfg.alpha);

    let mut found: Vec<Candidate> = vec![];
    for b in lo + 1..bins {
        let zb = z(&real, b);
        if zb > threshold && zb >= z(&real, b - 1) && zb > z(&real, b + 1) {
            let p = (null_max.iter().filter(|&&m| m >= zb).count() as f64 + 1.0) / (n + 1.0);
            let c = Candidate { period: refine(ev, b as f64 * cfg.bin_s, cfg), z: zb, conf: 1.0 - p };
            match found.last_mut() {
                Some(l) if (c.period - l.period).abs() < cfg.period_tol * l.period => {
                    if c.z > l.z {
                        *l = c;
                    }
                }
                _ => found.push(c),
            }
        }
    }

    let mut kept: Vec<Candidate> = vec![];
    for c in found {
        let multiple_of = kept.iter().find_map(|s| {
            let m = (c.period / s.period).round();
            (m >= 2.0 && (c.period - m * s.period).abs() < cfg.period_tol * c.period).then_some(m as usize)
        });
        if let Some(m) = multiple_of {
            let mut null: Vec<f64> = sur.iter().map(|e| power(&coeffs(e, c.period, cfg.harmonics), m)).collect();
            null.sort_by(f64::total_cmp);
            if power(&coeffs(ev, c.period, cfg.harmonics), m) <= quantile(&null, 1.0 - cfg.alpha) {
                continue;
            }
        }
        kept.push(c);
    }
    kept.sort_by(|a, b| b.z.total_cmp(&a.z));
    kept.truncate(MAX_OSCILLATORS);
    kept.sort_by(|a, b| a.period.total_cmp(&b.period));
    kept
}

impl Oscillator {
    fn build(ev: &[(f64, f64)], c: &Candidate, frames: usize, cfg: &RhythmSettings) -> Oscillator {
        let co = coeffs(ev, c.period, cfg.harmonics);
        let bins = cfg.template_bins;
        let density: Vec<f64> = (0..bins)
            .map(|g| {
                let phi = g as f64 / bins as f64;
                let s: f64 = co
                    .iter()
                    .enumerate()
                    .map(|(i, &(re, im))| {
                        let a = TAU * (i + 1) as f64 * phi;
                        re * a.cos() - im * a.sin()
                    })
                    .sum();
                (1.0 + 2.0 * s).max(0.0)
            })
            .collect();
        let (peak, top) = density
            .iter()
            .enumerate()
            .fold((0, 0.0), |a, (i, &d)| if d > a.1 { (i, d) } else { a });
        let times: Vec<f64> = ev.iter().map(|e| e.0).collect();
        let window = (cfg.window_cycles * c.period).max(cfg.min_window_s);
        let amp = (0..frames)
            .map(|f| {
                let t = f as f64 / FPS as f64;
                let a = times.partition_point(|&x| x < t - window / 2.0);
                let b = times.partition_point(|&x| x <= t + window / 2.0);
                let s = &ev[a..b];
                if s.is_empty() { 0.0 } else { excess(power(&coeffs(s, c.period, cfg.harmonics), 1), chance(s)) as f32 }
            })
            .collect();
        Oscillator {
            period_s: c.period,
            z: c.z,
            confidence: c.conf,
            strength: excess(power(&co, 1), chance(ev)),
            peak_phase: peak as f64 / bins as f64,
            template: density.iter().map(|&d| (d / top.max(1e-9)) as f32).collect(),
            amp,
        }
    }
}

impl Rhythm {
    /// Fit to every stem of the track. Notes that start together count once.
    pub fn fit(track: &Track, cfg: &RhythmSettings) -> Rhythm {
        let stems: Vec<Vec<f64>> = track
            .stems
            .iter()
            .map(|s| {
                let mut out: Vec<f64> = vec![];
                for n in &s.notes {
                    if out.last().is_none_or(|&l| n.onset as f64 - l >= cfg.same_onset_s) {
                        out.push(n.onset as f64);
                    }
                }
                out
            })
            .collect();
        Rhythm::from_onsets(&stems, track.frames(), cfg)
    }

    /// Fit to onset times in seconds, one list per stem, with `frames` video frames of output.
    pub fn from_onsets(stems: &[Vec<f64>], frames: usize, cfg: &RhythmSettings) -> Rhythm {
        let ev = pool(stems);
        let oscillators = detect(stems, &ev, cfg).iter().map(|c| Oscillator::build(&ev, c, frames, cfg)).collect();
        Rhythm { oscillators }
    }

    /// Oscillators ranked from the shortest period.
    pub fn oscillators(&self) -> &[Oscillator] {
        &self.oscillators
    }

    /// 0..1; an oscillator the track does not have reads as 0, like a muted stem.
    pub fn value(&self, rank: usize, sig: RhythmSignal, t: f32) -> f32 {
        let Some(o) = self.oscillators.get(rank) else { return 0.0 };
        let cycles = t as f64 / o.period_s;
        match sig {
            RhythmSignal::Phase => (cycles - o.peak_phase).rem_euclid(1.0) as f32,
            RhythmSignal::Pulse => {
                o.template[(cycles.rem_euclid(1.0) * o.template.len() as f64) as usize % o.template.len()]
            }
            RhythmSignal::Amp => {
                let x = (t * FPS as f32).clamp(0.0, (o.amp.len() - 1) as f32);
                let (i, u) = (x.floor() as usize, x.fract());
                o.amp[i] * (1.0 - u) + o.amp[(i + 1).min(o.amp.len() - 1)] * u
            }
            RhythmSignal::Conf => o.confidence as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SignalSettings;

    fn cfg() -> RhythmSettings {
        SignalSettings::shipped().rhythm
    }

    fn train(first: f64, period: f64, n: usize, rng: &mut Rng) -> Vec<f64> {
        (0..n).map(|k| ((first + k as f64 * period + 0.01 * (rng.unit() - 0.5)) * 100.0).round() / 100.0).collect()
    }

    fn scatter(n: usize, span: f64, rng: &mut Rng) -> Vec<f64> {
        let mut v: Vec<f64> = (0..n).map(|_| (rng.unit() * span * 100.0).round() / 100.0).collect();
        v.sort_by(f64::total_cmp);
        v
    }

    fn has(r: &Rhythm, period: f64) -> Option<&Oscillator> {
        r.oscillators().iter().find(|o| (o.period_s - period).abs() < 0.015 * period)
    }

    #[test]
    fn two_unrelated_periods_are_both_recovered() {
        let mut rng = Rng(7);
        let stems = [train(0.2, 0.5, 48, &mut rng), train(0.35, 0.7, 34, &mut rng)];
        let r = Rhythm::from_onsets(&stems, 900, &cfg());
        for p in [0.5, 0.7] {
            let o = has(&r, p).unwrap_or_else(|| panic!("no oscillator near {p}: {:?}", r.oscillators().iter().map(|o| o.period_s).collect::<Vec<_>>()));
            assert!(o.confidence >= 0.95);
        }
    }

    #[test]
    fn aperiodic_onsets_rarely_give_an_oscillator() {
        let false_alarms = (0..10)
            .filter(|&seed| {
                let mut rng = Rng(100 + seed);
                let stems = [scatter(76, 29.0, &mut rng), scatter(36, 29.0, &mut rng), scatter(74, 29.0, &mut rng)];
                !Rhythm::from_onsets(&stems, 900, &cfg()).oscillators().is_empty()
            })
            .count();
        assert!(false_alarms <= 2, "{false_alarms} of 10 aperiodic inputs gave an oscillator");
    }

    #[test]
    fn the_template_peaks_on_the_onsets_and_the_phase_is_zero_there() {
        let mut rng = Rng(3);
        let r = Rhythm::from_onsets(&[train(0.2, 0.5, 48, &mut rng)], 900, &cfg());
        assert!(has(&r, 1.0).is_none(), "a pure period must not reappear as its own multiple");
        let rank = r.oscillators().iter().position(|o| (o.period_s - 0.5).abs() < 0.0075).unwrap();
        let on = 0.2 + 0.5 * 20.0;
        assert!(r.value(rank, RhythmSignal::Pulse, on) > 0.9);
        assert!(r.value(rank, RhythmSignal::Pulse, on + 0.25) < 0.5);
        let ph = r.value(rank, RhythmSignal::Phase, on);
        assert!(ph.min(1.0 - ph) < 0.03, "{ph}");
        assert!(r.value(rank, RhythmSignal::Amp, on) > 0.5);
        assert_eq!(r.value(MAX_OSCILLATORS, RhythmSignal::Pulse, on), 0.0);
    }

    #[test]
    fn a_pattern_of_alternating_accents_keeps_its_double_period() {
        let mut rng = Rng(5);
        let beat = train(0.2, 0.5, 48, &mut rng);
        let accent: Vec<f64> = train(0.2, 1.0, 24, &mut rng);
        let r = Rhythm::from_onsets(&[beat, accent], 900, &cfg());
        assert!(has(&r, 0.5).is_some() && has(&r, 1.0).is_some());
    }

    #[test]
    fn the_real_track_shows_its_kick_metronome() {
        let t = Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json")).unwrap();
        let r = Rhythm::fit(&t, &cfg());
        assert!(has(&r, 60.0 / t.bpm as f64).is_some(), "{:?}", r.oscillators().iter().map(|o| o.period_s).collect::<Vec<_>>());
    }
}
