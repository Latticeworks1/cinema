//! Scene derivation: where the cuts fall, who leads each scene, and what the scene looks like.
//!
//! Boundaries come from the track: the drop, the impact, phrase starts counted in bars from the
//! drop, and the entrance or exit of any stem after a silence. They are snapped onto a drum
//! onset, then thinned so no scene is shorter than a minimum number of bars (structural
//! boundaries win over incidental ones). The leading stem of a scene is the one doing the most
//! unusual amount of playing compared with its own average. The vocabulary entry for every
//! other choice is the one nearest to the scene's measured feel.

use crate::settings::{Attention, CameraLimits, Chrome, Ink, Lyric, Marks, Meter, Priority, Shots, Stage, Tracker};
use crate::vocab::{At, Vocabulary, nearest};
use serde::{Deserialize, Serialize};
use signals::{Axes, FeelModel, Selection};
use track::{FPS, Track};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Direction {
    pub phrase_bars: u32,
    pub min_scene_bars: f32,
    pub entrance_gap_bars: f32,
    pub snap_to: Vec<String>,
    pub snap_window_beats: f32,
    pub repeat_penalty: f32,
    pub palette_blend_s: f32,
    pub whip_s: f32,
    pub whip_blur: f32,
    pub fit: f32,
    pub panel_span_s: f32,
    pub now_frac: f32,
    pub cut_flash: [f32; 3],
    pub lock_frames: usize,
    pub outro_fade_s: f32,
    pub seed: u64,
    /// Spread below which a stem's bar-to-bar activity counts as constant.
    /// Family of the text drawn in both passes; its files are in the packet's media folder.
    pub font: String,
    pub activity_std_floor: f32,
    pub attention: Attention,
    pub priority: Priority,
    pub stage: Stage,
    pub camera: CameraLimits,
    pub marks: Marks,
    pub tracker: Tracker,
    pub shots: Shots,
    pub chrome: Chrome,
    pub ink: Ink,
    pub meter: Meter,
    #[serde(default)]
    pub lyric: Lyric,
    #[serde(default)]
    pub cues: Vec<Cue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Cue {
    pub t: f32,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub palette: Option<String>,
    #[serde(default)]
    pub behavior: Option<String>,
    #[serde(default)]
    pub look: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Real onset, in absolute track seconds, of every word of `text` in reading order (each
    /// `" / "` subline's words in turn). From a forced alignment of the known lyric words against
    /// a transcriber's real decode of the vocal, not authored: when present it replaces the
    /// uniform typing speed as the source of which word is newest, so the line reveals in time
    /// with what is actually sung instead of a constant pace. Absent for a cue with no sung text
    /// (a stage direction) or one nobody has aligned yet; `lines()` falls back to typing speed.
    #[serde(default)]
    pub word_times: Option<Vec<f32>>,
}

/// Every word of a cue's text, in the order `word_times` must list them: each `" / "` subline in
/// turn, split on whitespace.
pub fn cue_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(" / ").flat_map(|sub| sub.split_whitespace())
}

impl Direction {
    pub fn parse(json: &str) -> Result<Direction, String> {
        let d: Direction = serde_json::from_str(json).map_err(|e| format!("direction: {e}"))?;
        let ok = d.phrase_bars > 0
            && d.min_scene_bars > 0.0
            && d.snap_window_beats >= 0.0
            && d.whip_s > 0.0
            && (0.3..=1.0).contains(&d.fit)
            && d.panel_span_s > 0.0
            && (0.0..=1.0).contains(&d.now_frac)
            && d.lock_frames > 0
            && !d.font.is_empty()
            && d.activity_std_floor > 0.0
            && d.attention.lead_s >= 0.0
            && d.attention.fall_s > 0.0
            && d.attention.notes > 0
            && d.stage.w > 0.0
            && d.stage.h > 0.0
            && d.stage.grid_cols > 0
            && (0.0..1.0).contains(&d.stage.hero_share)
            && d.stage.lane_emphasis > 0.0
            && d.camera.max_zoom >= 1.0
            && (0.0..=1.0).contains(&d.camera.stage_slack)
            && (0.0..0.5).contains(&d.camera.safe_x)
            && (0.0..0.5).contains(&d.camera.safe_y)
            && d.marks.note_h > 0.0
            && d.shots.beats > 0.0
            && d.shots.whip_s > 0.0
            && d.shots.zoom > 0.0
            && (0.0..=1.0).contains(&d.shots.mix)
            && d.tracker.hold_s > 0.0
            && d.tracker.arm > 0.0
            && d.chrome.band_h > 0.0
            && d.chrome.fft_bands >= 1
            && d.chrome.fft_lo_hz > 0.0
            && d.chrome.fft_lo_hz < d.chrome.fft_hi_hz
            && d.chrome.fft_db_floor < d.chrome.fft_db_ceil
            && d.chrome.band_span_panels > 0.0
            && d.chrome.merge_px > 0.0
            && d.chrome.meter_floor_db < d.chrome.meter_ceil_db
            && d.cues.iter().all(|c| c.t >= 0.0)
            && d.cues.iter().all(|c| match &c.word_times {
                None => true,
                Some(wt) => {
                    wt.len() == c.text.as_deref().map_or(0, |t| cue_words(t).count())
                        && wt.windows(2).all(|w| w[1] >= w[0])
                        && wt.first().is_none_or(|&t0| t0 >= 0.0)
                }
            });
        if ok { Ok(d) } else { Err("direction: a value is out of range".into()) }
    }
}

#[derive(Clone, Debug)]
struct Boundary {
    t: f32,
    priority: u8,
    reason: String,
    /// Placed by a packet cue rather than derived from the track. A cue's timing is itself a
    /// measured event (the moment of a sung line), not incidental noise, so two cues are never
    /// thinned against each other for being close together; they may still suppress, or be
    /// suppressed by, a derived boundary within `min_scene_bars`.
    from_cue: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Scene {
    pub index: usize,
    pub t0: f32,
    pub t1: f32,
    /// Why the scene starts here.
    pub starts_with: String,
    /// Feel of the whole mix over the scene.
    pub axes: Axes,
    /// Stem that leads, with each stem's activity against its own average (in standard deviations).
    pub dominant: String,
    pub dominance: Vec<(String, f32)>,
    /// Feel of each stem in the scene, in vocabulary stem order; `None` where it does not play.
    pub stem_axes: Vec<Option<Axes>>,
    pub palette: usize,
    pub behavior: usize,
    pub look: usize,
    pub layout: usize,
    /// Display material of the screen outside any panel.
    pub screen_material: usize,
    /// Shape and display material per stem, in vocabulary stem order.
    pub shape_by_stem: Vec<usize>,
    pub material_by_stem: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The matched cue's real per-word onsets, aligned with `text`; see `Cue::word_times`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_times: Option<Vec<f32>>,
}

impl Scene {
    pub fn duration(&self) -> f32 {
        self.t1 - self.t0
    }
}

fn snap(t: f32, track: &Track, d: &Direction) -> (f32, Option<String>) {
    let window = d.snap_window_beats * 60.0 / track.bpm;
    let best = d
        .snap_to
        .iter()
        .filter_map(|name| track.stem(name))
        .flat_map(|s| s.notes.iter().map(move |n| (n.onset, s.name.as_str())))
        .filter(|(o, _)| (o - t).abs() <= window)
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()));
    match best {
        Some((o, name)) => (o, Some(name.to_string())),
        None => (t, None),
    }
}

fn candidates(track: &Track, d: &Direction) -> Vec<Boundary> {
    let bar = track.bar_s;
    let gap = d.entrance_gap_bars * bar;
    let b = |t: f32, priority: u8, reason: String| Boundary { t, priority, reason, from_cue: false };
    let mut out = vec![
        b(0.0, d.priority.start, "start".into()),
        b(track.drop_t, d.priority.drop, "drop (first bass note)".into()),
        b(track.impact_t, d.priority.impact, "impact (beat after the last kick)".into()),
    ];
    let phrase = d.phrase_bars as f32 * bar;
    if !d.cues.is_empty() {
        for cue in &d.cues {
            let mut cb = b(cue.t, cue.priority.unwrap_or(8), cue.label.clone().unwrap_or_else(|| format!("cue at {:.2}s", cue.t)));
            cb.from_cue = true;
            out.push(cb);
        }
    } else if track.drop_t > 0.0 {
        let mut pre_k = 1;
        while pre_k as f32 * phrase < track.drop_t {
            out.push(b(pre_k as f32 * phrase, d.priority.phrase, format!("phrase (pre-drop {})", pre_k)));
            pre_k += 1;
        }
    }
    let mut k = 1;
    while track.drop_t + k as f32 * phrase < track.impact_t {
        out.push(b(track.drop_t + k as f32 * phrase, d.priority.phrase, format!("phrase {} (every {} bars from the drop)", k + 1, d.phrase_bars)));
        k += 1;
    }
    for s in &track.stems {
        let mut end = f32::MIN;
        for (i, n) in s.notes.iter().enumerate() {
            let first = i == 0;
            if (first && n.onset > 0.5 * bar) || (!first && n.onset - end >= gap) {
                out.push(b(n.onset, d.priority.enter, format!("{} enters", s.name)));
            }
            end = end.max(n.offset);
            let next = s.notes.get(i + 1).map(|m| m.onset);
            let leaves = match next {
                Some(nx) => nx - end >= gap,
                None => track.duration_s - end >= gap,
            };
            if leaves {
                out.push(b(end, d.priority.exit, format!("{} leaves", s.name)));
            }
        }
    }
    for c in out.iter_mut().filter(|c| c.priority != d.priority.start) {
        if let (t, Some(stem)) = snap(c.t, track, d) {
            c.t = t;
            c.reason = format!("{}, cut on {stem}", c.reason);
        }
    }
    out
}

/// Keep high-priority boundaries first and drop any that fall within `min_gap` of a kept one.
/// Two cues are never thinned against each other: each is a measured moment (a sung line), not
/// incidental noise, so cues may cut as fast as the lyric actually arrives. A cue still competes
/// with, and by priority normally beats, a derived boundary within `min_gap`.
fn thin(mut cands: Vec<Boundary>, min_gap: f32) -> Vec<Boundary> {
    cands.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.t.total_cmp(&b.t)));
    let mut kept: Vec<Boundary> = vec![];
    for c in cands {
        if kept.iter().all(|k| (k.from_cue && c.from_cue) || (k.t - c.t).abs() >= min_gap) {
            kept.push(c);
        }
    }
    kept.sort_by(|a, b| a.t.total_cmp(&b.t));
    kept
}

/// Per-stem mean and spread of notes per bar over the whole track.
fn activity_norm(track: &Track, floor: f32) -> Vec<(f32, f32)> {
    let bars = (track.duration_s / track.bar_s).floor().max(1.0) as usize;
    track
        .stems
        .iter()
        .map(|s| {
            let counts: Vec<f32> = (0..bars)
                .map(|k| {
                    let (a, b) = (k as f32 * track.bar_s, (k + 1) as f32 * track.bar_s);
                    (s.notes.partition_point(|n| n.onset < b) - s.notes.partition_point(|n| n.onset < a)) as f32
                })
                .collect();
            let mean = counts.iter().sum::<f32>() / bars as f32;
            let var = counts.iter().map(|c| (c - mean).powi(2)).sum::<f32>() / bars as f32;
            (mean, var.sqrt().max(floor))
        })
        .collect()
}

pub fn derive(track: &Track, feel: &FeelModel, vocab: &Vocabulary, d: &Direction) -> Result<Vec<Scene>, String> {
    let bounds = thin(candidates(track, d), d.min_scene_bars * track.bar_s);
    let norm = activity_norm(track, d.activity_std_floor);
    let mut scenes: Vec<Scene> = vec![];
    for (i, bd) in bounds.iter().enumerate() {
        let t1 = bounds.get(i + 1).map_or(track.duration_s, |n| n.t);
        let t0 = bd.t;
        if t1 - t0 < 1.0 / FPS as f32 {
            return Err(format!("scene {i} is shorter than a frame ({t0}..{t1})"));
        }
        let axes = feel.axes(track, Selection::All, t0, t1).unwrap_or(Axes { edge: 0.0, energy: 0.0 });
        let at = At { edge: axes.edge, energy: axes.energy };

        let mut dominance = vec![];
        for (si, s) in track.stems.iter().enumerate() {
            let count = s.notes.partition_point(|n| n.onset < t1) - s.notes.partition_point(|n| n.onset < t0);
            let per_bar = count as f32 * track.bar_s / (t1 - t0);
            dominance.push((s.name.clone(), (per_bar - norm[si].0) / norm[si].1));
        }
        let order = |name: &str| vocab.stem_index(name).unwrap_or(usize::MAX);
        let dominant = dominance
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1).then(order(&b.0).cmp(&order(&a.0))))
            .map(|(n, _)| n.clone())
            .ok_or("track has no stems")?;

        let stem_axes: Vec<Option<Axes>> = vocab
            .stems
            .iter()
            .map(|sd| feel.axes(track, Selection::Stem(&sd.name), t0, t1))
            .collect();
        let pos = |a: &Option<Axes>| a.map_or(at, |a| At { edge: a.edge, energy: a.energy });
        let prev = scenes.last();
        let palette = nearest(&vocab.palettes, at, prev.map(|p| p.palette), d.repeat_penalty);
        let behavior = nearest(&vocab.behaviors, at, prev.map(|p| p.behavior), d.repeat_penalty);
        let look = nearest(&vocab.looks, at, prev.map(|p| p.look), d.repeat_penalty);
        let layout = nearest(&vocab.layouts, at, prev.map(|p| p.layout), d.repeat_penalty);
        let screen_material = nearest(&vocab.materials, at, None, 0.0);
        let shape_by_stem = stem_axes.iter().map(|a| nearest(&vocab.shapes, pos(a), None, 0.0)).collect();
        let material_by_stem = stem_axes.iter().map(|a| nearest(&vocab.materials, pos(a), None, 0.0)).collect();
        let matched_cue = d.cues.iter().find(|c| (c.t - t0).abs() < 0.25 || (snap(c.t, track, d).0 - t0).abs() < 0.05);
        let palette = matched_cue.and_then(|c| c.palette.as_deref()).and_then(|p| vocab.palette_index(p)).unwrap_or(palette);
        let behavior = matched_cue.and_then(|c| c.behavior.as_deref()).and_then(|b| vocab.behavior_index(b)).unwrap_or(behavior);
        let look = matched_cue.and_then(|c| c.look.as_deref()).and_then(|l| vocab.look_index(l)).unwrap_or(look);
        let layout = matched_cue.and_then(|c| c.layout.as_deref()).and_then(|l| vocab.layout_index(l)).unwrap_or(layout);
        let text = matched_cue.and_then(|c| c.text.clone());
        let word_times = matched_cue.and_then(|c| c.word_times.clone());

        scenes.push(Scene {
            index: i,
            t0,
            t1,
            starts_with: bd.reason.clone(),
            axes,
            dominant,
            dominance,
            stem_axes,
            palette,
            behavior,
            look,
            layout,
            screen_material,
            shape_by_stem,
            material_by_stem,
            text,
            word_times,
        });
    }
    Ok(scenes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use signals::FeelSettings;
    use std::sync::Arc;

    fn setup() -> (Arc<Track>, FeelModel, Vocabulary, Direction) {
        let t = Arc::new(Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json")).unwrap());
        let feel = FeelModel::fit(&t, FeelSettings::from_json(include_str!("../../../vocab/feel.json")).unwrap());
        let vocab = Vocabulary::parse(include_str!("../../../vocab/vocabulary.json"), &t.stem_names()).unwrap();
        let d = Direction::parse(include_str!("../../../vocab/direction.json")).unwrap();
        (t, feel, vocab, d)
    }

    #[test]
    fn scenes_tile_the_recording_and_respect_the_minimum_length() {
        let (t, f, v, d) = setup();
        let s = derive(&t, &f, &v, &d).unwrap();
        assert_eq!(s[0].t0, 0.0);
        assert_eq!(s.last().unwrap().t1, t.duration_s);
        assert!(s.windows(2).all(|w| w[0].t1 == w[1].t0));
        let min = d.min_scene_bars * t.bar_s;
        // The final scene after the impact may be shorter: the impact is kept for its own sake.
        assert!(s[..s.len() - 1].iter().all(|x| x.duration() >= min - 1e-3), "{:?}", s.iter().map(|x| x.duration()).collect::<Vec<_>>());
    }

    #[test]
    fn the_drop_and_the_impact_are_scene_starts_within_a_beat_of_their_times() {
        let (t, f, v, d) = setup();
        let s = derive(&t, &f, &v, &d).unwrap();
        let beat = 60.0 / t.bpm;
        for target in [t.drop_t, t.impact_t] {
            assert!(s.iter().any(|x| (x.t0 - target).abs() <= 0.5 * beat + 1e-3), "no scene starts near {target}");
        }
    }

    #[test]
    fn cuts_land_on_a_drum_onset() {
        let (t, f, v, d) = setup();
        let s = derive(&t, &f, &v, &d).unwrap();
        let drums: Vec<f32> = d
            .snap_to
            .iter()
            .flat_map(|n| t.stem(n).unwrap().notes.iter().map(|n| n.onset))
            .collect();
        let on_drum = s[1..].iter().filter(|x| drums.iter().any(|o| (o - x.t0).abs() < 1e-4)).count();
        assert!(on_drum >= (s.len() - 1) / 2, "{on_drum} of {} cuts on a drum", s.len() - 1);
    }

    #[test]
    fn the_intro_is_led_by_the_drums_and_the_bass_entering_is_a_scene_start() {
        let (t, f, v, d) = setup();
        let s = derive(&t, &f, &v, &d).unwrap();
        assert!(["kick", "hat", "snare"].contains(&s[0].dominant.as_str()), "{}", s[0].dominant);
        let drop = s.iter().find(|x| (x.t0 - t.drop_t).abs() < 0.3).unwrap();
        assert!(drop.starts_with.contains("drop") || drop.starts_with.contains("bass"), "{}", drop.starts_with);
    }

    #[test]
    fn muting_the_bass_removes_the_scene_it_defined() {
        let (t, f, v, d) = setup();
        let full = derive(&t, &f, &v, &d).unwrap();
        let muted_track = Arc::new(t.without_stem("bass"));
        let muted = derive(&muted_track, &f, &v, &d).unwrap();
        let bass_entrances = |s: &[Scene]| s.iter().filter(|x| x.starts_with.contains("bass enters") || x.starts_with.contains("bass leaves")).count();
        assert!(bass_entrances(&full) > 0 || full.iter().any(|x| x.dominant == "bass"));
        assert_eq!(bass_entrances(&muted), 0);
        assert!(muted.iter().all(|x| x.dominant != "bass"));
        // Only structure the bass owned goes: the rest of the scene list is still there.
        assert!(muted.len() <= full.len() && muted.len() + 4 >= full.len(), "{} vs {}", muted.len(), full.len());
    }

    fn direction_with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Direction, String> {
        let mut v: serde_json::Value = serde_json::from_str(include_str!("../../../vocab/direction.json")).unwrap();
        edit(&mut v);
        Direction::parse(&v.to_string())
    }

    #[test]
    fn the_chrome_band_is_read_and_its_ranges_are_checked() {
        let d = direction_with(|_| {}).unwrap();
        assert_eq!((d.chrome.band_y, d.chrome.band_h, d.chrome.fft_bands), (836.0, 124.0, 48));
        assert!(d.stage.y + d.stage.h <= d.chrome.band_y, "stage must end above the chrome band");
        assert!(direction_with(|v| v["chrome"]["band_h"] = 0.0.into()).is_err());
        assert!(direction_with(|v| v["chrome"]["fft_bands"] = 0.into()).is_err());
        assert!(direction_with(|v| v["chrome"]["fft_lo_hz"] = 16000.into()).is_err());
        assert!(direction_with(|v| v["chrome"]["fft_db_floor"] = 48.0.into()).is_err());
        assert!(direction_with(|v| v["chrome"]["fft_hz"] = 1.into()).is_err(), "unknown key");
        assert!(direction_with(|v| { v.as_object_mut().unwrap().remove("chrome"); }).is_err(), "chrome is required");
    }

    #[test]
    fn neighbouring_scenes_differ_unless_every_alternative_is_farther_than_the_penalty() {
        use crate::vocab::{At, Placed};
        fn check<T: Placed>(kind: &str, items: &[T], scenes: &[Scene], pick: impl Fn(&Scene) -> usize, penalty: f32) {
            for w in scenes.windows(2) {
                let (here, i) = (At { edge: w[1].axes.edge, energy: w[1].axes.energy }, pick(&w[1]));
                if i == pick(&w[0]) {
                    let own = items[i].at().distance(here);
                    let best_other = (0..items.len()).filter(|k| *k != i).map(|k| items[k].at().distance(here)).fold(f32::MAX, f32::min);
                    assert!(best_other - own >= penalty, "{kind} repeats at {} though another lies within the penalty", w[1].t0);
                }
            }
        }
        let (t, f, v, d) = setup();
        let s = derive(&t, &f, &v, &d).unwrap();
        check("palette", &v.palettes, &s, |x| x.palette, d.repeat_penalty);
        check("behavior", &v.behaviors, &s, |x| x.behavior, d.repeat_penalty);
    }
}
