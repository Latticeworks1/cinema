//! The feel of a span of the track on two axes, measured against the rest of the same song.
//!
//! Edge is how angular the sound is: short notes, high register and fast onsets read as sharp
//! (the "kiki" end), long low slow notes as round (the "bouba" end). Energy is how forceful it
//! is: loud and busy against quiet and sparse. Each raw feature is standardised against the
//! distribution over every bar of the track, so a value of 0 means "typical for this song" and
//! a section is described by how far it departs from the rest.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use track::{FPS, Stem, Track};


#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeWeights {
    pub staccato: f32,
    pub register: f32,
    pub rate: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnergyWeights {
    pub level: f32,
    pub rate: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeelSettings {
    /// Notes starting within this of each other are one event.
    pub same_onset_s: f32,
    /// Smallest spread that counts as a difference when standardising: staccato, register,
    /// onsets per second, loudness in dB. Without it a near-constant feature would amplify noise.
    pub std_floor: [f32; 4],
    /// Length of the bars-windows the song's distribution is measured over.
    pub window_bars: f32,
    /// Pitch range that maps to register 0..1 for pitched stems.
    pub pitch_lo: f32,
    pub pitch_hi: f32,
    /// Register of stems whose MIDI pitch is a drum-kit slot and not a height.
    pub register_override: HashMap<String, f32>,
    pub edge: EdgeWeights,
    pub energy: EnergyWeights,
}

impl FeelSettings {
    pub fn from_json(text: &str) -> Result<FeelSettings, String> {
        let s: FeelSettings = serde_json::from_str(text).map_err(|e| format!("feel settings: {e}"))?;
        if !(s.window_bars > 0.0 && s.pitch_hi > s.pitch_lo && s.same_onset_s >= 0.0 && s.std_floor.iter().all(|f| *f > 0.0)) {
            return Err("feel settings: window_bars and std_floor must be positive, pitch_hi above pitch_lo".into());
        }
        Ok(s)
    }
}

/// Position of a span between the extremes of the song, each in -1..1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Axes {
    pub edge: f32,
    pub energy: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum Selection<'a> {
    /// Every stem pooled.
    All,
    Stem(&'a str),
}

#[derive(Clone, Copy)]
struct Dist {
    mean: [f32; 4],
    std: [f32; 4],
}

impl Dist {
    fn fit(samples: &[[f32; 4]], floor: &[f32; 4]) -> Dist {
        let n = samples.len().max(1) as f32;
        let mut mean = [0.0; 4];
        for s in samples {
            for k in 0..4 {
                mean[k] += s[k] / n;
            }
        }
        let mut std = [1.0; 4];
        for k in 0..4 {
            let var = samples.iter().map(|s| (s[k] - mean[k]).powi(2)).sum::<f32>() / n;
            std[k] = var.sqrt().max(floor[k]);
        }
        Dist { mean, std }
    }

    fn z(&self, f: [f32; 4]) -> [f32; 4] {
        std::array::from_fn(|k| (f[k] - self.mean[k]) / self.std[k])
    }
}

pub struct FeelModel {
    settings: FeelSettings,
    all: Dist,
    per_stem: Dist,
}

/// Staccato, register, onsets per second, loudness in dB, pooled over the notes starting in
/// `[t0, t1)`; `None` when no stem has a note there.
fn features(track: &Track, s: &FeelSettings, stems: &[&Stem], t0: f32, t1: f32) -> Option<[f32; 4]> {
    let (mut count, mut stac, mut reg, mut events) = (0usize, 0.0f32, 0.0f32, 0usize);
    for stem in stems {
        let a = stem.notes.partition_point(|n| n.onset < t0);
        let b = stem.notes.partition_point(|n| n.onset < t1);
        let fallback_gap = stem.median_ioi().unwrap_or(1.0);
        let mut prev_onset = f32::MIN;
        for i in a..b {
            let n = stem.notes[i];
            let gap = stem.notes[i + 1..]
                .iter()
                .find(|m| m.onset > n.onset + s.same_onset_s)
                .map_or(fallback_gap, |m| m.onset - n.onset);
            stac += 1.0 - (n.duration() / gap).clamp(0.0, 1.0);
            reg += s.register_override.get(&stem.name).copied().unwrap_or_else(|| {
                ((n.pitch as f32 - s.pitch_lo) / (s.pitch_hi - s.pitch_lo)).clamp(0.0, 1.0)
            });
            if n.onset - prev_onset >= s.same_onset_s {
                events += 1;
            }
            prev_onset = n.onset;
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let f0 = ((t0 * FPS as f32).floor().max(0.0) as usize).min(track.frames() - 1);
    let f1 = ((t1 * FPS as f32).ceil() as usize).clamp(f0 + 1, track.frames());
    let level = track.level_db[f0..f1].iter().sum::<f32>() / (f1 - f0) as f32;
    Some([stac / count as f32, reg / count as f32, events as f32 / (t1 - t0), level])
}

impl FeelModel {
    pub fn fit(track: &Track, settings: FeelSettings) -> FeelModel {
        let win = settings.window_bars * track.bar_s;
        let n = ((track.duration_s / win).round() as usize).max(1);
        let all_stems: Vec<&Stem> = track.stems.iter().collect();
        let mut all = vec![];
        let mut per_stem = vec![];
        for k in 0..n {
            let (t0, t1) = (k as f32 * win, ((k + 1) as f32 * win).min(track.duration_s));
            if let Some(f) = features(track, &settings, &all_stems, t0, t1) {
                all.push(f);
            }
            for stem in &track.stems {
                if let Some(f) = features(track, &settings, &[stem], t0, t1) {
                    per_stem.push(f);
                }
            }
        }
        FeelModel { all: Dist::fit(&all, &settings.std_floor), per_stem: Dist::fit(&per_stem, &settings.std_floor), settings }
    }

    pub fn settings(&self) -> &FeelSettings {
        &self.settings
    }

    /// Axes of the span `[t0, t1)`, or `None` when the selection plays no note in it.
    pub fn axes(&self, track: &Track, sel: Selection, t0: f32, t1: f32) -> Option<Axes> {
        let (f, dist) = match sel {
            Selection::All => {
                let stems: Vec<&Stem> = track.stems.iter().collect();
                (features(track, &self.settings, &stems, t0, t1)?, &self.all)
            }
            Selection::Stem(name) => {
                (features(track, &self.settings, &[track.stem(name)?], t0, t1)?, &self.per_stem)
            }
        };
        let z = dist.z(f);
        let (e, g) = (&self.settings.edge, &self.settings.energy);
        Some(Axes {
            edge: (e.staccato * z[0] + e.register * z[1] + e.rate * z[2]).tanh(),
            energy: (g.level * z[3] + g.rate * z[2]).tanh(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use track::Note;

    pub fn settings() -> FeelSettings {
        FeelSettings::from_json(include_str!("../../../vocab/feel.json")).unwrap()
    }

    fn real() -> Track {
        Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json")).unwrap()
    }

    /// Two stems with known character: a fast high staccato line and a slow low legato line.
    fn control() -> Track {
        let line = |name: &str, pitch: u8, step: f32, hold: f32| Stem {
            name: name.into(),
            notes: (0..(8.0 / step) as usize)
                .map(|i| Note { onset: i as f32 * step, offset: i as f32 * step + hold, pitch })
                .collect(),
        };
        Track {
            audio_file: String::new(),
            audio_sha256: String::new(),
            duration_s: 8.0,
            bpm: 120.0,
            bar_s: 2.0,
            drop_t: 0.0,
            impact_t: 7.0,
            chords: vec![],
            stems: vec![line("sharp", 84, 0.125, 0.02), line("round", 36, 1.0, 0.95)],
            level_db: vec![-20.0; 240],
            waveform: vec![],
            waveform_cols_per_s: 160.0,
            peaks_hz: std::collections::BTreeMap::new(),
            loudest: track::Loudest { t: 0.0, db: -20.0 },
        }
    }

    #[test]
    fn a_fast_high_staccato_line_is_sharper_than_a_slow_low_legato_line() {
        let t = control();
        let m = FeelModel::fit(&t, settings());
        let sharp = m.axes(&t, Selection::Stem("sharp"), 0.0, 8.0).unwrap();
        let round = m.axes(&t, Selection::Stem("round"), 0.0, 8.0).unwrap();
        assert!(sharp.edge > 0.5 && round.edge < -0.5, "{sharp:?} {round:?}");
        assert!(sharp.energy > round.energy, "{sharp:?} {round:?}");
    }

    #[test]
    fn a_span_without_notes_has_no_axes() {
        let t = real();
        let m = FeelModel::fit(&t, settings());
        assert!(m.axes(&t, Selection::Stem("bass"), 0.0, 1.0).is_none());
        assert!(m.axes(&t, Selection::Stem("tuba"), 0.0, 8.0).is_none());
    }

    #[test]
    fn every_bar_of_the_real_track_has_finite_axes_in_range() {
        let t = real();
        let m = FeelModel::fit(&t, settings());
        let win = m.settings().window_bars * t.bar_s;
        let mut k = 0.0;
        while k + win <= t.duration_s {
            let played = t.stems.iter().any(|s| s.notes.iter().any(|n| n.onset >= k && n.onset < k + win));
            let a = m.axes(&t, Selection::All, k, k + win);
            assert_eq!(a.is_some(), played, "a window has axes exactly when a note starts in it, at {k}");
            if let Some(a) = a {
                assert!(a.edge.abs() <= 1.0 && a.energy.abs() <= 1.0, "{a:?} at {k}");
            }
            k += win;
        }
    }

    #[test]
    fn the_drum_stems_differ_in_edge() {
        let t = real();
        let m = FeelModel::fit(&t, settings());
        let e = |s| m.axes(&t, Selection::Stem(s), t.drop_t, t.impact_t).unwrap().edge;
        assert!(e("hat") > e("kick"), "hat {} kick {}", e("hat"), e("kick"));
    }

    #[test]
    fn removing_the_piano_changes_the_pooled_edge_of_a_piano_section() {
        let t = real();
        let m = FeelModel::fit(&t, settings());
        let (a, b) = (t.drop_t + 4.0 * t.bar_s, t.drop_t + 8.0 * t.bar_s);
        let full = m.axes(&t, Selection::All, a, b).unwrap();
        let cut = t.without_stem("piano");
        let without = m.axes(&cut, Selection::All, a, b).unwrap();
        assert!((full.edge - without.edge).abs() > 0.01, "{full:?} {without:?}");
    }
}
