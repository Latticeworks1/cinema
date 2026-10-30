//! A validated view of `analysis.json`, the one input of record for every later stage.
//!
//! The Python exporter (`python/export_analysis.py`) measures the recording and records the
//! SHA-256 of the audio and of the transcription. This crate refuses anything that is
//! malformed or inconsistent, so no stage downstream has to guess: notes are sorted and
//! inside the recording, the loudness curve covers every frame, and the structure is ordered.

use serde::Deserialize;
use std::{collections::BTreeMap, fmt, fs, path::Path};

pub const SCHEMA_VERSION: u32 = 2;
pub mod output;
pub use output::FPS;

#[derive(Debug)]
pub enum TrackError {
    Io(String),
    Parse(String),
    Invalid(String),
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackError::Io(m) => write!(f, "cannot read analysis: {m}"),
            TrackError::Parse(m) => write!(f, "cannot parse analysis: {m}"),
            TrackError::Invalid(m) => write!(f, "invalid analysis: {m}"),
        }
    }
}

impl std::error::Error for TrackError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub onset: f32,
    pub offset: f32,
    pub pitch: u8,
}

impl Note {
    pub fn duration(&self) -> f32 {
        self.offset - self.onset
    }
}

/// One instrument line, notes sorted by onset.
#[derive(Clone, Debug, PartialEq)]
pub struct Stem {
    pub name: String,
    pub notes: Vec<Note>,
}

impl Stem {
    pub fn pitch_range(&self) -> (u8, u8) {
        let lo = self.notes.iter().map(|n| n.pitch).min().unwrap_or(0);
        let hi = self.notes.iter().map(|n| n.pitch).max().unwrap_or(0);
        (lo, hi)
    }

    /// Index of the last note whose onset is at or before `t`.
    pub fn last_onset_at(&self, t: f32) -> Option<usize> {
        self.notes.partition_point(|n| n.onset <= t).checked_sub(1)
    }

    /// Median gap between distinct onsets (simultaneous notes count once).
    pub fn median_ioi(&self) -> Option<f32> {
        let mut gaps: Vec<f32> = self
            .notes
            .windows(2)
            .map(|w| w[1].onset - w[0].onset)
            .filter(|g| *g > 0.01)
            .collect();
        if gaps.is_empty() {
            return None;
        }
        gaps.sort_by(f32::total_cmp);
        Some(gaps[gaps.len() / 2])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub t: f32,
    pub name: String,
}

/// The loudest frame of the mix: its time in seconds and its level in dBFS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudest {
    pub t: f32,
    pub db: f32,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub audio_file: String,
    pub audio_sha256: String,
    pub duration_s: f32,
    pub bpm: f32,
    /// Seconds per bar, measured from the transcription.
    pub bar_s: f32,
    /// First bass note: the section that follows is the body of the track.
    pub drop_t: f32,
    /// The beat after the last kick.
    pub impact_t: f32,
    pub chords: Vec<Chord>,
    pub stems: Vec<Stem>,
    /// Mixed-signal loudness per video frame, dBFS.
    pub level_db: Vec<f32>,
    /// Peak amplitude of the mix, 0..1, at `waveform_cols_per_s` columns per second.
    pub waveform: Vec<f32>,
    pub waveform_cols_per_s: f32,
    /// Dominant frequency of each named stem, Hz.
    pub peaks_hz: BTreeMap<String, f32>,
    pub loudest: Loudest,
}

#[derive(Deserialize)]
struct RawNote {
    onset: f32,
    offset: f32,
    pitch: u8,
}

#[derive(Deserialize)]
struct RawSource {
    audio_file: String,
    audio_sha256: String,
    audio_duration_s: f32,
    total_frames_at_30fps: usize,
}

#[derive(Deserialize)]
struct RawTempo {
    bpm: f32,
}

#[derive(Deserialize)]
struct RawStructure {
    drop_t: f32,
    bar_s: f32,
    impact_t: f32,
}

#[derive(Deserialize)]
struct RawChord {
    t: f32,
    name: String,
}

#[derive(Deserialize)]
struct RawWaveform {
    columns_per_second: f32,
    peaks: Vec<f32>,
}

#[derive(Deserialize)]
struct RawLoudest {
    t: f32,
    db: f32,
}

#[derive(Deserialize)]
struct RawMeasurements {
    peaks_hz: BTreeMap<String, f32>,
    loudest: RawLoudest,
}

#[derive(Deserialize)]
struct RawAnalysis {
    schema_version: u32,
    source: RawSource,
    tempo: RawTempo,
    measurements: RawMeasurements,
    waveform: RawWaveform,
    structure: RawStructure,
    chords: Vec<RawChord>,
    rms_curve_db: Vec<f32>,
    stems: BTreeMap<String, Vec<RawNote>>,
}

fn invalid<T>(msg: impl Into<String>) -> Result<T, TrackError> {
    Err(TrackError::Invalid(msg.into()))
}

impl Track {
    pub fn load(path: impl AsRef<Path>) -> Result<Track, TrackError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|e| TrackError::Io(format!("{}: {e}", path.display())))?;
        Track::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Track, TrackError> {
        let raw: RawAnalysis =
            serde_json::from_str(text).map_err(|e| TrackError::Parse(e.to_string()))?;
        if raw.schema_version != SCHEMA_VERSION {
            return invalid(format!(
                "schema_version {} (this build reads {SCHEMA_VERSION})",
                raw.schema_version
            ));
        }
        let duration_s = raw.source.audio_duration_s;
        if !(duration_s.is_finite() && duration_s > 0.0) {
            return invalid("audio_duration_s must be positive");
        }
        let frames = output::frame_count(duration_s);
        if raw.source.total_frames_at_30fps != frames || raw.rms_curve_db.len() != frames {
            return invalid(format!(
                "{} loudness values and {} declared frames for {frames} expected",
                raw.rms_curve_db.len(),
                raw.source.total_frames_at_30fps
            ));
        }
        if raw.rms_curve_db.iter().any(|v| !v.is_finite()) {
            return invalid("loudness curve holds a non-finite value");
        }
        let s = &raw.structure;
        if !(s.bar_s > 0.0 && 0.0 <= s.drop_t && s.drop_t < s.impact_t && s.impact_t <= duration_s)
        {
            return invalid(format!(
                "structure out of order: drop {} impact {} bar {} duration {duration_s}",
                s.drop_t, s.impact_t, s.bar_s
            ));
        }
        if !(raw.tempo.bpm > 0.0) {
            return invalid("bpm must be positive");
        }
        let w = &raw.waveform;
        if !(w.columns_per_second.is_finite() && w.columns_per_second > 0.0) {
            return invalid("waveform columns_per_second must be positive");
        }
        if w.peaks.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return invalid("waveform holds a peak that is negative or not finite");
        }
        let covered = w.peaks.len() as f32 / w.columns_per_second;
        if covered < duration_s - 0.2 {
            return invalid(format!(
                "waveform covers {covered} s of a {duration_s} s recording"
            ));
        }
        let m = &raw.measurements;
        if m.peaks_hz.values().any(|v| !(v.is_finite() && *v > 0.0)) {
            return invalid("every peaks_hz value must be positive");
        }
        let loudest = Loudest { t: m.loudest.t, db: m.loudest.db };
        if !(loudest.t.is_finite() && (0.0..=duration_s).contains(&loudest.t)) {
            return invalid(format!("loudest time {} is outside the recording", loudest.t));
        }
        if !loudest.db.is_finite() {
            return invalid("loudest level is not finite");
        }
        let mut chords: Vec<Chord> = raw
            .chords
            .into_iter()
            .map(|c| Chord { t: c.t, name: c.name })
            .collect();
        if chords.windows(2).any(|w| w[1].t <= w[0].t) {
            return invalid("chords are not in time order");
        }
        chords.shrink_to_fit();

        let mut stems: Vec<Stem> = Vec::new();
        for (name, raw_notes) in &raw.stems {
            let mut notes = Vec::with_capacity(raw_notes.len());
            for n in raw_notes {
                if !(n.onset.is_finite() && n.offset.is_finite())
                    || n.onset < 0.0
                    || n.offset < n.onset
                    || n.offset > duration_s + 0.05
                {
                    return invalid(format!(
                        "{name} note at {} s (offset {}) is outside the recording",
                        n.onset, n.offset
                    ));
                }
                notes.push(Note { onset: n.onset, offset: n.offset, pitch: n.pitch });
            }
            notes.sort_by(|a, b| a.onset.total_cmp(&b.onset));
            stems.push(Stem { name: name.clone(), notes });
        }
        if stems.is_empty() {
            return invalid("no stems");
        }

        Ok(Track {
            audio_file: raw.source.audio_file,
            audio_sha256: raw.source.audio_sha256,
            duration_s,
            bpm: raw.tempo.bpm,
            bar_s: s.bar_s,
            drop_t: s.drop_t,
            impact_t: s.impact_t,
            chords,
            stems,
            level_db: raw.rms_curve_db,
            waveform: raw.waveform.peaks,
            waveform_cols_per_s: raw.waveform.columns_per_second,
            peaks_hz: raw.measurements.peaks_hz,
            loudest,
        })
    }

    /// False for a track made from a transcription alone.
    pub fn has_audio(&self) -> bool {
        !self.audio_file.is_empty()
    }

    pub fn frames(&self) -> usize {
        self.level_db.len()
    }

    pub fn stem(&self, name: &str) -> Option<&Stem> {
        self.stems.iter().find(|s| s.name == name)
    }

    pub fn stem_names(&self) -> Vec<&str> {
        self.stems.iter().map(|s| s.name.as_str()).collect()
    }

    /// The chord of the bar containing `t`, if any chord has started.
    pub fn chord_at(&self, t: f32) -> Option<&Chord> {
        self.chords.partition_point(|c| c.t <= t).checked_sub(1).map(|i| &self.chords[i])
    }

    /// A copy of the track without the named stem, for testing what depends on it.
    pub fn without_stem(&self, name: &str) -> Track {
        let mut t = self.clone();
        t.stems.retain(|s| s.name != name);
        t
    }
}

/// `MIDI 74` -> `D5`.
pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[(pitch % 12) as usize], pitch as i32 / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Track {
        Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json"))
            .expect("data/analysis.json (run tools/render.sh to write it)")
    }

    #[test]
    fn midi_names() {
        assert_eq!(note_name(74), "D5");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(36), "C2");
    }

    #[test]
    fn the_recording_loads_with_split_stems() {
        let t = data();
        assert_eq!(t.frames(), 930);
        assert_eq!(t.stem_names(), ["bass", "hat", "kick", "piano", "snare"]);
        assert_eq!(t.stem("kick").unwrap().notes.len(), 76);
        assert!(t.drop_t < t.impact_t);
    }

    #[test]
    fn every_stem_is_sorted_and_inside_the_recording() {
        let t = data();
        for s in &t.stems {
            assert!(s.notes.windows(2).all(|w| w[0].onset <= w[1].onset), "{}", s.name);
            assert!(s.notes.iter().all(|n| n.offset <= t.duration_s + 0.05), "{}", s.name);
        }
    }

    #[test]
    fn the_bar_containing_a_time_names_its_chord() {
        let t = data();
        assert!(t.chord_at(t.drop_t - 0.5).is_none());
        assert_eq!(t.chord_at(t.drop_t + 0.1).unwrap().name, "Bm7");
    }

    #[test]
    fn schema_and_length_mismatches_are_refused() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json"))
            .unwrap();
        let wrong_version = text.replacen("\"schema_version\": 2", "\"schema_version\": 3", 1);
        assert!(matches!(Track::parse(&wrong_version), Err(TrackError::Invalid(_))));
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["rms_curve_db"].as_array_mut().unwrap().pop();
        assert!(matches!(Track::parse(&v.to_string()), Err(TrackError::Invalid(_))));
    }

    #[test]
    fn the_waveform_covers_the_track_and_the_measurements_are_loaded() {
        let t = data();
        assert_eq!(t.waveform_cols_per_s, 160.0);
        assert!(t.waveform.len() as f32 / t.waveform_cols_per_s >= t.duration_s - 0.2);
        assert!(t.peaks_hz.contains_key("kick") && t.peaks_hz.contains_key("bass"));
        assert!(t.loudest.t >= 0.0 && t.loudest.t <= t.duration_s);
    }

    fn mutated(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Track, TrackError> {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json"))
            .unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        edit(&mut v);
        Track::parse(&v.to_string())
    }

    #[test]
    fn a_malformed_waveform_is_refused() {
        // Too short: 100 columns at 160 per second is 0.6 s of a 31 s recording.
        let short = mutated(|v| v["waveform"]["peaks"].as_array_mut().unwrap().truncate(100));
        assert!(matches!(short, Err(TrackError::Invalid(m)) if m.contains("waveform covers")));
        let rate = mutated(|v| v["waveform"]["columns_per_second"] = 0.0.into());
        assert!(matches!(rate, Err(TrackError::Invalid(_))));
    }

    #[test]
    fn a_negative_peak_is_refused() {
        let neg = mutated(|v| v["waveform"]["peaks"][7] = (-0.1).into());
        assert!(matches!(neg, Err(TrackError::Invalid(m)) if m.contains("peak")));
    }

    #[test]
    fn a_non_positive_peak_frequency_or_an_out_of_range_loudest_is_refused() {
        let hz = mutated(|v| v["measurements"]["peaks_hz"]["kick"] = 0.0.into());
        assert!(matches!(hz, Err(TrackError::Invalid(_))));
        let late = mutated(|v| v["measurements"]["loudest"]["t"] = 99.0.into());
        assert!(matches!(late, Err(TrackError::Invalid(_))));
    }

    #[test]
    fn removing_a_stem_removes_only_that_stem() {
        let t = data();
        let b = t.without_stem("bass");
        assert!(b.stem("bass").is_none());
        assert_eq!(b.stems.len(), t.stems.len() - 1);
    }
}
