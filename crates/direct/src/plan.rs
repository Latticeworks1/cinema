//! The plan: scenes derived from a track, and the per-frame state both render passes read.
//!
//! `Plan::frame` is a pure function of the frame index. It places the camera for the scene the
//! frame falls in, adds the matrix's music-driven motion, resolves the look and palette, and
//! lists the display material of each panel.

use crate::camera::{Camera, Setting, behave, keep_in_stage, keep_in_view};
use crate::geometry::{Rect, lerp, smoothstep};
use crate::layout::{Layout, Panel, Timeline};
use crate::matrix::{Matrix, Param};
use crate::scenes::{Direction, Scene, derive};
use crate::vocab::{Framing, Vocabulary};
use signals::{Bus, FeelModel, FeelSettings};
use std::{fs, path::Path, sync::Arc};
use track::{FPS, Stem, Track};

pub struct Sources<'a> {
    pub feel: &'a str,
    pub vocabulary: &'a str,
    pub matrix: &'a str,
    pub direction: &'a str,
}

pub struct Plan {
    track: Arc<Track>,
    pub bus: Bus,
    pub vocab: Vocabulary,
    pub matrix: Matrix,
    pub dir: Direction,
    pub timeline: Timeline,
    pub scenes: Vec<Scene>,
    pub layouts: Vec<Layout>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookState {
    pub bloom: f32,
    pub led: f32,
    pub aberration: f32,
    pub grain: f32,
    pub vignette: f32,
    pub blur: f32,
    pub tear: f32,
    pub punch: f32,
    /// Exposure flash on a cut and at the impact.
    pub cut: f32,
    /// Fade to black at the end.
    pub out: f32,
    /// The leading stem has just played a note.
    pub lock: bool,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub stem: String,
    pub rect: Rect,
    /// Index into the vocabulary's materials.
    pub material: usize,
}

#[derive(Clone, Debug)]
pub struct FrameState {
    pub index: usize,
    pub t: f32,
    pub scene: usize,
    pub camera: Camera,
    pub focus: (f32, f32),
    pub look: LookState,
    pub stops: [[f32; 3]; 5],
    pub pulse: f32,
    pub emit: f32,
    pub regions: Vec<Region>,
    pub screen_material: usize,
}

/// How far ahead of a note's onset the camera starts to turn towards it, seconds.
const ATTENTION_LEAD_S: f32 = 0.15;
/// How long after its onset a note keeps pulling the camera, seconds.
const ATTENTION_FALL_S: f32 = 0.4;
const ATTENTION_NOTES: usize = 8;

/// Register of `pitch` inside the stem's own range, 0 lowest and 1 highest.
pub fn register(stem: &Stem, pitch: u8) -> f32 {
    let (lo, hi) = stem.pitch_range();
    if hi == lo { 0.5 } else { (pitch as f32 - lo as f32) / (hi - lo) as f32 }
}

impl Plan {
    /// Read `analysis` and the four authored files (`feel`, `vocabulary`, `matrix`,
    /// `direction`, each `.json`) from `vocab_dir`.
    pub fn load(analysis: impl AsRef<Path>, vocab_dir: impl AsRef<Path>) -> Result<Plan, String> {
        let dir = vocab_dir.as_ref();
        let read = |name: &str| fs::read_to_string(dir.join(name)).map_err(|e| format!("{}: {e}", dir.join(name).display()));
        let (feel, vocabulary, matrix, direction) =
            (read("feel.json")?, read("vocabulary.json")?, read("matrix.json")?, read("direction.json")?);
        let track = Track::load(analysis).map_err(|e| e.to_string())?;
        Plan::build(track, &Sources { feel: &feel, vocabulary: &vocabulary, matrix: &matrix, direction: &direction })
    }

    pub fn build(track: Track, src: &Sources) -> Result<Plan, String> {
        let track = Arc::new(track);
        let settings = FeelSettings::from_json(src.feel)?;
        let feel = FeelModel::fit(&track, settings);
        let vocab = Vocabulary::parse(src.vocabulary, &track.stem_names())?;
        let matrix = Matrix::parse(src.matrix, &vocab.declared_stems())?;
        let dir = Direction::parse(src.direction)?;
        let scenes = derive(&track, &feel, &vocab, &dir)?;
        let present: Vec<&str> = vocab.stems.iter().map(|s| s.name.as_str()).filter(|n| track.stem(n).is_some()).collect();
        let layouts = scenes
            .iter()
            .map(|s| {
                let def = &vocab.layouts[s.layout];
                Layout::arrange(def.rule, def.slider, &present, &s.dominant)
            })
            .collect();
        let timeline = Timeline { span_s: dir.panel_span_s, now_frac: dir.now_frac };
        let bus = Bus::new(track.clone());
        Ok(Plan { track, bus, vocab, matrix, dir, timeline, scenes, layouts })
    }

    pub fn track(&self) -> &Track {
        &self.track
    }

    pub fn frames(&self) -> usize {
        self.track.frames()
    }

    pub fn scene_at(&self, t: f32) -> usize {
        self.scenes.partition_point(|s| s.t0 <= t + 1e-4).saturating_sub(1)
    }

    /// The panel of the scene's leading stem.
    pub fn lead_panel(&self, scene: usize) -> Option<&Panel> {
        self.layouts[scene].panel(&self.scenes[scene].dominant)
    }

    fn subject(&self, scene: usize) -> Rect {
        match self.vocab.behaviors[self.scenes[scene].behavior].framing {
            Framing::Wide => Rect::SCREEN,
            Framing::Dominant => self.lead_panel(scene).map_or(Rect::SCREEN, |p| p.rect),
        }
    }

    /// Where the newest note of `stem` sits in its panel at time `t`.
    pub fn newest_note_pos(&self, scene: usize, stem: &str, t: f32) -> Option<(f32, f32)> {
        let s = self.track.stem(stem)?;
        let n = s.notes[s.last_onset_at(t)?];
        let panel = self.layouts[scene].panel(stem)?;
        Some(panel.note_pos(self.layouts[scene].slider, &self.timeline, t, n.onset, register(s, n.pitch)))
    }

    /// A smooth point of attention on `stem`: its recent notes blended, each fading in over the
    /// `ATTENTION_LEAD_S` before its onset and out over `ATTENTION_FALL_S` after it, so the camera
    /// and the focus plane glide to a note instead of snapping.
    pub fn attention(&self, scene: usize, stem: &str, t: f32) -> Option<(f32, f32)> {
        let s = self.track.stem(stem)?;
        let panel = self.layouts[scene].panel(stem)?;
        let slider = self.layouts[scene].slider;
        let last = s.last_onset_at(t + ATTENTION_LEAD_S)?;
        let (mut sum, mut wsum) = ((0.0, 0.0), 0.0);
        for n in s.notes[..=last].iter().rev().take(ATTENTION_NOTES) {
            let arrive = smoothstep((t + ATTENTION_LEAD_S - n.onset) / ATTENTION_LEAD_S);
            let w = arrive * (-(t - n.onset).max(0.0) / ATTENTION_FALL_S).exp();
            let q = panel.note_pos(slider, &self.timeline, t, n.onset, register(s, n.pitch));
            sum = (sum.0 + w * q.0, sum.1 + w * q.1);
            wsum += w;
        }
        (wsum > 1e-4).then(|| (sum.0 / wsum, sum.1 / wsum))
    }

    fn locked(&self, scene: usize, f: usize) -> bool {
        let Some(s) = self.track.stem(&self.scenes[scene].dominant) else { return false };
        let probe = (f as f32 + 0.5) / FPS as f32;
        s.last_onset_at(probe).is_some_and(|i| {
            let fo = (s.notes[i].onset * FPS as f32).round() as usize;
            f >= fo && f < fo + self.dir.lock_frames
        })
    }

    pub fn frame(&self, index: usize) -> FrameState {
        let t = index as f32 / FPS as f32;
        let si = self.scene_at(t);
        let scene = &self.scenes[si];
        let look = &self.vocab.looks[scene.look];
        let m = self.matrix.evaluate(&self.bus, t);
        let moved = |p: Param| m[p.index()] * look.scale[p.index()];
        let value = |p: Param| look.base[p.index()] + moved(p);

        let subject = self.subject(si);
        let from = if si > 0 { self.subject(si - 1).center() } else { subject.center() };
        let follow = self.attention(si, &scene.dominant, t).unwrap_or(subject.center());
        let phrase = self.dir.phrase_bars as f32 * self.track.bar_s;
        let setting = Setting {
            subject,
            from,
            age: t - scene.t0,
            length: scene.duration(),
            phrase_phase: ((t - self.track.drop_t) / phrase).rem_euclid(1.0),
            follow,
            scene_index: si,
        };
        let mut cam = behave(self.vocab.behaviors[scene.behavior].kind, &setting, &self.dir);
        let whip = cam.blur;
        cam.zoom *= 1.0 + moved(Param::Zoom);
        cam.aim.0 += moved(Param::AimX);
        cam.aim.1 += moved(Param::AimY);
        cam.tilt_x += moved(Param::TiltX);
        cam.tilt_y += moved(Param::TiltY);
        cam.roll += moved(Param::Roll);
        keep_in_view(&mut cam, follow);
        keep_in_stage(&mut cam);

        let since_start = index as isize - (scene.t0 * FPS as f32).round() as isize;
        let flash = if (0..3).contains(&since_start) { self.dir.cut_flash[since_start as usize] } else { 0.0 };
        let impact = (-(t - self.track.impact_t).max(0.0) / 0.18).exp() * f32::from(t >= self.track.impact_t);
        let fade = self.dir.outro_fade_s;

        let stops = {
            let cur = self.vocab.palettes[scene.palette].stops;
            let u = smoothstep((t - scene.t0) / self.dir.palette_blend_s);
            if si == 0 || u >= 1.0 {
                cur
            } else {
                let prev = self.vocab.palettes[self.scenes[si - 1].palette].stops;
                std::array::from_fn(|k| std::array::from_fn(|c| lerp(prev[k][c], cur[k][c], u)))
            }
        };

        let regions = self.layouts[si]
            .panels
            .iter()
            .map(|p| Region {
                stem: p.stem.clone(),
                rect: p.rect,
                material: scene.material_by_stem[self.vocab.stem_index(&p.stem).expect("panel stems are declared")],
            })
            .collect();

        FrameState {
            index,
            t,
            scene: si,
            camera: cam,
            focus: follow,
            look: LookState {
                bloom: value(Param::Bloom).clamp(0.0, 1.5),
                led: value(Param::Led).clamp(0.0, 1.0),
                aberration: value(Param::Aberration).clamp(0.0, 1.5),
                grain: value(Param::Grain).clamp(0.0, 1.0),
                vignette: value(Param::Vignette).clamp(0.0, 1.0),
                blur: (value(Param::Blur) + whip).clamp(0.0, 2.0),
                tear: value(Param::Tear).clamp(0.0, 1.5),
                punch: value(Param::Punch).clamp(0.0, 1.5),
                cut: flash.max(impact),
                out: ((t - (self.track.duration_s - fade)) / fade).clamp(0.0, 1.0),
                lock: self.locked(si, index),
            },
            stops,
            pulse: moved(Param::Pulse).max(0.0),
            emit: (1.0 + m[Param::Emit.index()]).clamp(0.2, 3.0),
            regions,
            screen_material: scene.screen_material,
        }
    }

    /// The scene list with every vocabulary choice by name, for inspection.
    pub fn report(&self) -> serde_json::Value {
        let v = &self.vocab;
        serde_json::json!({
            "recording": { "duration_s": self.track.duration_s, "bpm": self.track.bpm, "bar_s": self.track.bar_s,
                           "drop_t": self.track.drop_t, "impact_t": self.track.impact_t },
            "scenes": self.scenes.iter().map(|s| serde_json::json!({
                "index": s.index, "t0": s.t0, "t1": s.t1, "starts_with": s.starts_with,
                "axes": s.axes, "dominant": s.dominant, "dominance": s.dominance,
                "stem_axes": v.stems.iter().zip(&s.stem_axes).map(|(d, a)| (d.name.clone(), a)).collect::<std::collections::BTreeMap<_, _>>(),
                "palette": v.palettes[s.palette].name, "behavior": v.behaviors[s.behavior].name,
                "look": v.looks[s.look].name, "layout": v.layouts[s.layout].name,
                "screen_material": v.materials[s.screen_material].name,
                "shapes": v.stems.iter().zip(&s.shape_by_stem).map(|(d, i)| (d.name.clone(), v.shapes[*i].name.clone())).collect::<std::collections::BTreeMap<_, _>>(),
                "materials": v.stems.iter().zip(&s.material_by_stem).map(|(d, i)| (d.name.clone(), v.materials[*i].name.clone())).collect::<std::collections::BTreeMap<_, _>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::project;
    use crate::geometry::{H, W};

    fn plan() -> Plan {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        Plan::load(format!("{root}/data/analysis.json"), format!("{root}/vocab")).unwrap()
    }

    #[test]
    fn every_frame_has_finite_state_and_the_lead_note_stays_in_view() {
        let p = plan();
        for f in 0..p.frames() {
            let s = p.frame(f);
            let c = s.camera;
            assert!([c.aim.0, c.aim.1, c.zoom, c.tilt_x, c.tilt_y, c.roll, c.blur].iter().all(|v| v.is_finite()), "frame {f}");
            let l = s.look;
            assert!([l.bloom, l.led, l.aberration, l.grain, l.vignette, l.blur, l.tear, l.punch, l.cut, l.out].iter().all(|v| v.is_finite()), "frame {f}");
            let q = project(&c, s.focus, W, H).expect("focus is in front of the camera");
            assert!(q.0 > 0.0 && q.0 < W && q.1 > 0.0 && q.1 < H, "frame {f}: focus at {q:?}");
        }
    }

    #[test]
    fn frames_are_pure_functions_of_the_index() {
        let p = plan();
        for f in [0, 187, 400, 929] {
            let a = p.frame(f);
            let b = p.frame(f);
            assert_eq!(a.camera, b.camera);
            assert_eq!(a.look, b.look);
        }
    }

    #[test]
    fn a_scene_start_flashes_and_the_end_fades() {
        let p = plan();
        let s1 = &p.scenes[1];
        let f = (s1.t0 * FPS as f32).round() as usize;
        assert_eq!(p.frame(f).look.cut, p.dir.cut_flash[0]);
        assert!(p.frame(f + 3).look.cut < 0.2 || p.frame(f + 3).look.cut == 0.0 || p.frame(f + 3).t > p.track().impact_t);
        assert!(p.frame(p.frames() - 1).look.out > 0.9);
        assert_eq!(p.frame(0).look.out, 0.0);
    }

    #[test]
    fn a_kick_drives_the_punch_and_a_lock_follows_the_leading_stem() {
        let p = plan();
        let kicks = &p.track().stem("kick").unwrap().notes;
        let kick = kicks.windows(2).skip(20).find(|w| w[1].onset - w[0].onset > 0.35).map(|w| w[0].onset).unwrap();
        let f = (kick * FPS as f32).ceil() as usize;
        let (on, later) = (p.frame(f).look.punch, p.frame(f + 8).look.punch);
        assert!(on > later + 0.05, "punch {on} on the first frame of the kick and {later} eight frames later");
        let lead = &p.scenes[2].dominant;
        let n = p.track().stem(lead).unwrap().notes.iter().find(|n| n.onset > p.scenes[2].t0 + 0.2).unwrap();
        assert!(p.frame((n.onset * FPS as f32).round() as usize).look.lock);
    }

    #[test]
    fn muting_the_piano_removes_its_panel_and_its_modulation() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let full = plan();
        let text = |n: &str| std::fs::read_to_string(format!("{root}/vocab/{n}.json")).unwrap();
        let muted = Plan::build(
            full.track().without_stem("piano"),
            &Sources { feel: &text("feel"), vocabulary: &text("vocabulary"), matrix: &text("matrix"), direction: &text("direction") },
        )
        .unwrap();
        assert!(muted.layouts.iter().all(|l| l.panel("piano").is_none()));
        assert!(full.layouts.iter().all(|l| l.panel("piano").is_some()));
        let note = full.track().stem("piano").unwrap().notes[20];
        let f = (note.onset * FPS as f32).round() as usize;
        assert!(full.frame(f).pulse > muted.frame(f).pulse || full.frame(f).look.bloom > muted.frame(f).look.bloom);
    }
}
