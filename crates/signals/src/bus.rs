use std::{collections::HashMap, fmt, sync::Arc};
use crate::rhythm::{MAX_OSCILLATORS, Rhythm, RhythmSignal};
use crate::settings::{BusSettings, SignalSettings};
use track::{FPS, Stem, Track};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StemSignal {
    /// Impulse at each onset with an exponential tail.
    Hit,
    /// 1 while a note of the stem is held.
    Gate,
    /// Register of the latest event, 0 at the stem's lowest note and 1 at its highest.
    Pitch,
    /// Interval size from the previous event, relative to the stem's range, decaying with `Hit`.
    Leap,
    /// Onsets in the last bar relative to the stem's busiest bar.
    Density,
    /// Time since the stem last sounded, in bars, up to 1.
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MasterSignal {
    /// Mixed loudness, 0 at the quiet end of the track and 1 at the loud end.
    Level,
    /// Position inside the bar, counted from the drop.
    BarPhase,
    /// Position inside the beat, counted from the drop.
    BeatPhase,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Stem(String, StemSignal),
    Master(MasterSignal),
    /// An inferred rhythmic oscillator, ranked from the shortest period.
    Rhythm(usize, RhythmSignal),
}

#[derive(Debug, PartialEq)]
pub enum SourceError {
    Syntax(String),
    UnknownSignal(String),
    UnknownStem(String),
    UnknownOscillator(usize),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Syntax(s) => write!(f, "source `{s}` is not of the form `stem.signal`"),
            SourceError::UnknownSignal(s) => write!(f, "unknown signal in `{s}`"),
            SourceError::UnknownStem(s) => write!(f, "no stem named `{s}` in the track"),
            SourceError::UnknownOscillator(k) => write!(f, "no oscillator rank {k} (the bus has {MAX_OSCILLATORS})"),
        }
    }
}

impl std::error::Error for SourceError {}

impl Source {
    /// Parse `kick.hit`, `piano.pitch`, `master.level`, `rhythm.0.pulse`. Stem names are checked by [`Bus::check`].
    pub fn parse(text: &str) -> Result<Source, SourceError> {
        let (head, tail) = text.split_once('.').ok_or_else(|| SourceError::Syntax(text.into()))?;
        if head == "master" {
            let m = match tail {
                "level" => MasterSignal::Level,
                "bar_phase" => MasterSignal::BarPhase,
                "beat_phase" => MasterSignal::BeatPhase,
                _ => return Err(SourceError::UnknownSignal(text.into())),
            };
            return Ok(Source::Master(m));
        }
        if head == "rhythm" {
            let syntax = || SourceError::Syntax(text.into());
            let (rank, sig) = tail.split_once('.').ok_or_else(syntax)?;
            let rank: usize = rank.parse().map_err(|_| syntax())?;
            let sig = RhythmSignal::parse(sig).ok_or_else(|| SourceError::UnknownSignal(text.into()))?;
            return Ok(Source::Rhythm(rank, sig));
        }
        let s = match tail {
            "hit" => StemSignal::Hit,
            "gate" => StemSignal::Gate,
            "pitch" => StemSignal::Pitch,
            "leap" => StemSignal::Leap,
            "density" => StemSignal::Density,
            "quiet" => StemSignal::Quiet,
            _ => return Err(SourceError::UnknownSignal(text.into())),
        };
        Ok(Source::Stem(head.into(), s))
    }
}

struct StemInfo {
    decay_s: f32,
    lo: u8,
    hi: u8,
    max_density: f32,
}

pub struct Bus {
    track: Arc<Track>,
    info: HashMap<String, StemInfo>,
    level_lo: f32,
    level_hi: f32,
    rhythm: Rhythm,
    cfg: BusSettings,
}

fn percentile(values: &[f32], p: f32) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * p).round() as usize]
}

fn onsets_in(stem: &Stem, t0: f32, t1: f32) -> usize {
    let a = stem.notes.partition_point(|n| n.onset <= t0);
    let b = stem.notes.partition_point(|n| n.onset <= t1);
    b - a
}

impl Bus {
    /// A bus with the settings shipped in `vocab/signals.json`.
    pub fn new(track: Arc<Track>) -> Bus {
        Bus::with_settings(track, &SignalSettings::shipped())
    }

    pub fn with_settings(track: Arc<Track>, settings: &SignalSettings) -> Bus {
        let cfg = settings.bus.clone();
        let info = track
            .stems
            .iter()
            .map(|s| {
                let (lo, hi) = s.pitch_range();
                let decay_s = (0.5 * s.median_ioi().unwrap_or(0.5)).clamp(cfg.decay_min_s, cfg.decay_max_s);
                let max_density = (0..track.frames())
                    .map(|f| onsets_in(s, f as f32 / FPS as f32 - track.bar_s, f as f32 / FPS as f32))
                    .max()
                    .unwrap_or(1)
                    .max(1) as f32;
                (s.name.clone(), StemInfo { decay_s, lo, hi, max_density })
            })
            .collect();
        let level_lo = percentile(&track.level_db, cfg.level_lo_pct);
        let level_hi = percentile(&track.level_db, cfg.level_hi_pct);
        let rhythm = Rhythm::fit(&track, &settings.rhythm);
        Bus { track, info, level_lo, level_hi, rhythm, cfg }
    }

    pub fn track(&self) -> &Track {
        &self.track
    }

    pub fn rhythm(&self) -> &Rhythm {
        &self.rhythm
    }

    /// Fails when the source names a stem the track does not have.
    pub fn check(&self, source: &Source) -> Result<(), SourceError> {
        match source {
            Source::Stem(name, _) if !self.info.contains_key(name) => {
                Err(SourceError::UnknownStem(name.clone()))
            }
            Source::Rhythm(rank, _) if *rank >= MAX_OSCILLATORS => Err(SourceError::UnknownOscillator(*rank)),
            _ => Ok(()),
        }
    }

    /// Decay constant of a stem's `Hit`: half its median gap between onsets, within limits.
    pub fn decay_s(&self, stem: &str) -> Option<f32> {
        self.info.get(stem).map(|i| i.decay_s)
    }

    /// Value of a source at time `t`, in 0..1. A stem the track does not have is silent, which
    /// is how a muted stem is expressed; callers refuse misspelled names with [`Bus::check`]
    /// or against their own declared stem list.
    pub fn value(&self, source: &Source, t: f32) -> f32 {
        match source {
            Source::Stem(name, sig) => match self.track.stem(name) {
                Some(stem) => self.stem_value(stem, &self.info[name], *sig, t),
                None => 0.0,
            },
            Source::Master(m) => self.master_value(*m, t),
            Source::Rhythm(rank, sig) => self.rhythm.value(*rank, *sig, t),
        }
    }

    fn master_value(&self, m: MasterSignal, t: f32) -> f32 {
        match m {
            MasterSignal::Level => {
                let x = (t * FPS as f32).clamp(0.0, (self.track.frames() - 1) as f32);
                let (i, u) = (x.floor() as usize, x.fract());
                let j = (i + 1).min(self.track.frames() - 1);
                let db = self.track.level_db[i] * (1.0 - u) + self.track.level_db[j] * u;
                ((db - self.level_lo) / (self.level_hi - self.level_lo)).clamp(0.0, 1.0)
            }
            MasterSignal::BarPhase => ((t - self.track.drop_t) / self.track.bar_s).rem_euclid(1.0),
            MasterSignal::BeatPhase => {
                ((t - self.track.drop_t) / (60.0 / self.track.bpm)).rem_euclid(1.0)
            }
        }
    }

    fn stem_value(&self, stem: &Stem, info: &StemInfo, sig: StemSignal, t: f32) -> f32 {
        let Some(i) = stem.last_onset_at(t) else {
            return match sig {
                StemSignal::Quiet => 1.0,
                _ => 0.0,
            };
        };
        let last = stem.notes[i];
        let hit = (-(t - last.onset) / info.decay_s).exp();
        match sig {
            StemSignal::Hit => hit,
            StemSignal::Gate => {
                let held = stem.notes[..=i]
                    .iter()
                    .rev()
                    .take(self.cfg.lookback)
                    .any(|n| n.onset <= t && t < n.offset);
                f32::from(held)
            }
            StemSignal::Pitch => self.register(stem, info, i),
            StemSignal::Leap => {
                let group_start = (0..=i)
                    .rev()
                    .take_while(|&j| last.onset - stem.notes[j].onset < self.cfg.same_onset_s)
                    .last()
                    .unwrap_or(i);
                let Some(prev) = group_start.checked_sub(1) else {
                    return 0.0;
                };
                let span = (info.hi - info.lo).max(1) as f32;
                let cur = self.top_pitch(stem, i) as f32;
                let before = self.top_pitch(stem, prev) as f32;
                ((cur - before).abs() / span).min(1.0) * hit
            }
            StemSignal::Density => {
                (onsets_in(stem, t - self.track.bar_s, t) as f32 / info.max_density).min(1.0)
            }
            StemSignal::Quiet => {
                let held = stem.notes[..=i]
                    .iter()
                    .rev()
                    .take(self.cfg.lookback)
                    .any(|n| n.onset <= t && t < n.offset);
                if held {
                    return 0.0;
                }
                let end = stem.notes[..=i]
                    .iter()
                    .rev()
                    .take(self.cfg.lookback)
                    .map(|n| n.offset)
                    .fold(f32::MIN, f32::max);
                ((t - end) / self.track.bar_s).clamp(0.0, 1.0)
            }
        }
    }

    /// Highest pitch among the notes that start together with note `i`.
    fn top_pitch(&self, stem: &Stem, i: usize) -> u8 {
        let onset = stem.notes[i].onset;
        stem.notes
            .iter()
            .skip(i.saturating_sub(self.cfg.lookback))
            .filter(|n| (n.onset - onset).abs() < self.cfg.same_onset_s)
            .map(|n| n.pitch)
            .max()
            .unwrap_or(stem.notes[i].pitch)
    }

    fn register(&self, stem: &Stem, info: &StemInfo, i: usize) -> f32 {
        if info.hi == info.lo {
            return 0.5;
        }
        (self.top_pitch(stem, i) as f32 - info.lo as f32) / (info.hi - info.lo) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Arc<Track> {
        Arc::new(Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json")).unwrap())
    }

    fn src(s: &str) -> Source {
        Source::parse(s).unwrap()
    }

    #[test]
    fn sources_parse_and_unknown_names_are_refused() {
        assert_eq!(src("kick.hit"), Source::Stem("kick".into(), StemSignal::Hit));
        assert_eq!(src("master.bar_phase"), Source::Master(MasterSignal::BarPhase));
        assert!(matches!(Source::parse("kick"), Err(SourceError::Syntax(_))));
        assert!(matches!(Source::parse("kick.loud"), Err(SourceError::UnknownSignal(_))));
        let t = track();
        let bus = Bus::new(t.clone());
        assert_eq!(bus.check(&src("tuba.hit")), Err(SourceError::UnknownStem("tuba".into())));
        assert!(bus.check(&src("kick.hit")).is_ok());
        assert_eq!(src("rhythm.1.pulse"), Source::Rhythm(1, RhythmSignal::Pulse));
        assert!(bus.check(&src("rhythm.5.amp")).is_ok());
        assert_eq!(bus.check(&src("rhythm.6.amp")), Err(SourceError::UnknownOscillator(6)));
        assert!(matches!(Source::parse("rhythm.x.amp"), Err(SourceError::Syntax(_))));
        assert!(matches!(Source::parse("rhythm.0.beat"), Err(SourceError::UnknownSignal(_))));
    }

    #[test]
    fn a_hit_is_one_on_the_onset_and_decays() {
        let t = track();
        let bus = Bus::new(t.clone());
        let onset = t.stem("kick").unwrap().notes[3].onset;
        let hit = src("kick.hit");
        assert!((bus.value(&hit, onset) - 1.0).abs() < 1e-6);
        let tau = bus.decay_s("kick").unwrap();
        assert!((bus.value(&hit, onset + tau) - (-1.0f32).exp()).abs() < 1e-3);
        assert_eq!(bus.value(&hit, t.stem("kick").unwrap().notes[0].onset - 0.01), 0.0);
    }

    #[test]
    fn a_held_bass_note_gates_and_the_gap_after_it_is_quiet() {
        let t = track();
        let bus = Bus::new(t.clone());
        let n = t.stem("bass").unwrap().notes[0];
        assert_eq!(bus.value(&src("bass.gate"), 0.5 * (n.onset + n.offset)), 1.0);
        assert_eq!(bus.value(&src("bass.quiet"), 0.5 * (n.onset + n.offset)), 0.0);
        let first_gap = t.stem("bass").unwrap().notes.windows(2).find(|w| w[1].onset - w[0].offset > 0.3);
        if let Some(w) = first_gap {
            let mid = 0.5 * (w[0].offset + w[1].onset);
            assert_eq!(bus.value(&src("bass.gate"), mid), 0.0);
            assert!(bus.value(&src("bass.quiet"), mid) > 0.0);
        }
    }

    #[test]
    fn the_bar_phase_starts_at_the_drop() {
        let t = track();
        let bus = Bus::new(t.clone());
        let p = src("master.bar_phase");
        assert!(bus.value(&p, t.drop_t) < 1e-4);
        assert!((bus.value(&p, t.drop_t + 0.5 * t.bar_s) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn every_signal_stays_in_unit_range_on_every_frame() {
        let t = track();
        let bus = Bus::new(t.clone());
        let mut sources = vec![src("master.level"), src("master.bar_phase"), src("master.beat_phase")];
        for k in 0..MAX_OSCILLATORS {
            for sig in ["phase", "pulse", "amp", "conf"] {
                sources.push(src(&format!("rhythm.{k}.{sig}")));
            }
        }
        for s in &t.stems {
            for sig in ["hit", "gate", "pitch", "leap", "density", "quiet"] {
                sources.push(src(&format!("{}.{sig}", s.name)));
            }
        }
        for f in 0..t.frames() {
            for s in &sources {
                let v = bus.value(s, f as f32 / FPS as f32);
                assert!((0.0..=1.0).contains(&v), "{s:?} at frame {f} is {v}");
            }
        }
    }

    #[test]
    fn a_leap_is_larger_for_a_bigger_interval() {
        let mk = |pitches: &[u8]| Track {
            audio_file: String::new(),
            audio_sha256: String::new(),
            duration_s: 4.0,
            bpm: 120.0,
            bar_s: 2.0,
            drop_t: 0.0,
            impact_t: 3.0,
            chords: vec![],
            stems: vec![Stem {
                name: "x".into(),
                notes: pitches
                    .iter()
                    .enumerate()
                    .map(|(i, &p)| track::Note { onset: i as f32 * 0.5, offset: i as f32 * 0.5 + 0.4, pitch: p })
                    .collect(),
            }],
            level_db: vec![-20.0; 120],
            waveform: vec![],
            waveform_cols_per_s: 160.0,
            peaks_hz: std::collections::BTreeMap::new(),
            loudest: track::Loudest { t: 0.0, db: -20.0 },
        };
        let step = mk(&[60, 62, 72]);
        let bus = Bus::new(Arc::new(step));
        assert!(bus.value(&src("x.leap"), 0.5) > 0.0);
        assert!(bus.value(&src("x.leap"), 1.0) > bus.value(&src("x.leap"), 0.5));
    }
}
