//! The plan: scenes derived from a track, and the per-frame state both render passes read.
//!
//! `Plan::frame` is a pure function of the frame index. It places the camera for the scene the
//! frame falls in, adds the matrix's music-driven motion, resolves the look and palette, and
//! lists the display material of each panel.

use crate::camera::{Camera, Setting, behave, fit_zoom, keep_in_stage, keep_in_view};
use crate::geometry::{Rect, ease_in_out, ease_out, lerp, rnd, smoothstep};
use crate::layout::{Layout, Panel, Timeline};
use crate::lyric::{self, LyricLine, LyricState};
use crate::matrix::{Matrix, Param};
use crate::scenes::{Direction, Scene, derive};
use crate::vocab::{Framing, Vocabulary};
use packet::{Packet, Section};
use crate::anchors::{Anchors, Mark};
use signals::{Bus, FeelModel, FeelSettings, SignalSettings};
use std::{fs, path::Path, sync::Arc};
use track::{FPS, Stem, Track};

pub struct Sources<'a> {
    pub feel: &'a str,
    pub vocabulary: &'a str,
    pub matrix: &'a str,
    pub direction: &'a str,
    pub signals: &'a str,
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
    pub lyrics: Vec<LyricLine>,
    /// Motifs found in the plate images; empty until `packet prepare` has detected them.
    pub anchors: Anchors,
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
    pub panels: Vec<Panel>,
    /// The lyric line being shot, when the frame is in one.
    pub lyric: Option<LyricState>,
}

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
        let (feel, vocabulary, matrix, direction, signals) =
            (read("feel.json")?, read("vocabulary.json")?, read("matrix.json")?, read("direction.json")?, read("signals.json")?);
        let track = Track::load(analysis).map_err(|e| e.to_string())?;
        Plan::build(track, &Sources { feel: &feel, vocabulary: &vocabulary, matrix: &matrix, direction: &direction, signals: &signals })
    }

    /// Read the packet's analysis and its authored files, with the packet's overrides applied.
    pub fn from_packet(p: &Packet) -> Result<Plan, String> {
        let track = Track::load(p.analysis_path()).map_err(|e| e.to_string())?;
        let (feel, vocabulary, matrix, direction, signals) = (
            p.section(Section::Feel)?,
            p.section(Section::Vocabulary)?,
            p.section(Section::Matrix)?,
            p.section(Section::Direction)?,
            p.section(Section::Signals)?,
        );
        let mut plan = Plan::build(track, &Sources { feel: &feel, vocabulary: &vocabulary, matrix: &matrix, direction: &direction, signals: &signals })?;
        if p.anchors_path().exists() {
            plan.anchors = Anchors::load(p.anchors_path())?;
        }
        Ok(plan)
    }

    /// Every anchor of every plate on screen at this frame, in the flat's world coordinates.
    pub fn anchor_marks(&self, st: &FrameState) -> Vec<Mark> {
        let mut out = vec![];
        for p in &self.dir.collage {
            let (Some(im), Some(at)) = (self.anchors.images.get(&p.image), p.placed(&self.dir, st)) else { continue };
            out.extend(p.marks(&at, im));
        }
        out
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
                Layout::arrange(def.rule, def.slider, &present, &s.dominant, &dir.stage)
            })
            .collect();
        let timeline = Timeline { span_s: dir.panel_span_s, now_frac: dir.now_frac };
        let bus = Bus::with_settings(track.clone(), &SignalSettings::from_json(src.signals)?);
        let lyrics = lyric::lines(&scenes, &track, &dir);
        Ok(Plan { track, bus, vocab, matrix, dir, timeline, scenes, layouts, lyrics, anchors: Anchors::default() })
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

    /// Morphs panels smoothly from the previous scene's layout during scene transitions.
    pub fn morphed_panels(&self, si: usize, t: f32) -> Vec<Panel> {
        let cur = &self.layouts[si];
        if si == 0 {
            return cur.panels.clone();
        }
        let scene = &self.scenes[si];
        let morph_s = self.dir.whip_s.max(0.35);
        let u = smoothstep((t - scene.t0) / morph_s);
        if u >= 1.0 {
            return cur.panels.clone();
        }
        let prev = &self.layouts[si - 1];
        cur.panels
            .iter()
            .map(|p| {
                if let Some(pp) = prev.panels.iter().find(|pp| pp.stem == p.stem) {
                    Panel {
                        stem: p.stem.clone(),
                        rect: Rect::lerp(pp.rect, p.rect, u),
                        pad: lerp(pp.pad, p.pad, u),
                    }
                } else {
                    let inset = p.rect.inset(p.rect.w.min(p.rect.h) * 0.25 * (1.0 - u));
                    Panel {
                        stem: p.stem.clone(),
                        rect: inset,
                        pad: p.pad,
                    }
                }
            })
            .collect()
    }

    /// Where the newest note of `stem` sits in its panel at time `t`.
    pub fn newest_note_pos(&self, scene: usize, stem: &str, t: f32) -> Option<(f32, f32)> {
        let s = self.track.stem(stem)?;
        let n = s.notes[s.last_onset_at(t)?];
        let panel = self.layouts[scene].panel(stem)?;
        Some(panel.note_pos(self.layouts[scene].slider, &self.timeline, t, n.onset, register(s, n.pitch)))
    }

    /// A smooth point of attention on `stem`: its recent notes blended, each fading in over the
    /// `self.dir.attention.lead_s` before its onset and out over `self.dir.attention.fall_s` after it, so the camera
    /// and the focus plane glide to a note instead of snapping.
    pub fn attention(&self, scene: usize, stem: &str, t: f32) -> Option<(f32, f32)> {
        let s = self.track.stem(stem)?;
        let panel = self.layouts[scene].panel(stem)?;
        let slider = self.layouts[scene].slider;
        let last = s.last_onset_at(t + self.dir.attention.lead_s)?;
        let (mut sum, mut wsum) = ((0.0, 0.0), 0.0);
        for n in s.notes[..=last].iter().rev().take(self.dir.attention.notes) {
            let arrive = smoothstep((t + self.dir.attention.lead_s - n.onset) / self.dir.attention.lead_s);
            let w = arrive * (-(t - n.onset).max(0.0) / self.dir.attention.fall_s).exp();
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
            let fo = track::output::nearest_frame(s.notes[i].onset);
            f >= fo && f < fo + self.dir.lock_frames
        })
    }

    /// Centre, zoom and tilt direction of shot `k` in scene `si`: a panel (or the whole stage)
    /// picked by keyed noise so any frame can be evaluated alone.
    fn shot_target(&self, si: usize, k: i64) -> ((f32, f32), f32, f32) {
        let sh = &self.dir.shots;
        let panels = &self.layouts[si].panels;
        let key = k as u64;
        let wide = panels.is_empty() || rnd(self.dir.seed, 7001, key) < sh.wide;
        let rect = if wide { Rect::SCREEN } else { panels[(rnd(self.dir.seed, 7002, key) * panels.len() as f32) as usize % panels.len()].rect };
        let zoom = if wide { 1.0 } else { fit_zoom(rect, self.dir.fit).max(1.0) * sh.zoom };
        let sign = if rnd(self.dir.seed, 7003, key) < 0.5 { -1.0 } else { 1.0 };
        (rect.center(), zoom, sign)
    }

    /// Blend the scene's behaviour with the shot layer after the drop.
    fn shot(&self, si: usize, t: f32, base: Camera) -> Camera {
        let sh = &self.dir.shots;
        if sh.mix <= 0.0 || t < self.track.drop_t {
            return base;
        }
        let len = sh.beats * 60.0 / self.track.bpm;
        let k = ((t - self.track.drop_t) / len).floor() as i64;
        let a = t - (self.track.drop_t + k as f32 * len);
        let u = (a / len).clamp(0.0, 1.0);
        let (c, z, sign) = self.shot_target(si, k);
        let (pc, pz, _) = self.shot_target(si, k - 1);
        let w = (a / sh.whip_s).clamp(0.0, 1.0);
        let e = ease_out(w);
        let push = 1.0 + sh.push * u;
        let shot = Camera {
            aim: (lerp(pc.0, c.0, e), lerp(pc.1, c.1, e)),
            zoom: lerp(pz * (1.0 + sh.push), z * push, e),
            tilt_x: sh.tilt * sign * (w * std::f32::consts::PI).sin(),
            tilt_y: 0.0,
            roll: 0.5 * sh.tilt * sign * (w * std::f32::consts::PI).sin(),
            blur: sh.blur * (w * std::f32::consts::PI).sin(),
        };
        let m = sh.mix;
        Camera {
            aim: (lerp(base.aim.0, shot.aim.0, m), lerp(base.aim.1, shot.aim.1, m)),
            zoom: lerp(base.zoom, shot.zoom, m),
            tilt_x: base.tilt_x + shot.tilt_x * m,
            tilt_y: base.tilt_y,
            roll: base.roll + shot.roll * m,
            blur: base.blur.max(shot.blur * m),
        }
    }

    pub fn frame(&self, index: usize) -> FrameState {
        let t = track::output::seconds(index);
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
        let phrase_phase = ((t - self.track.drop_t) / phrase).rem_euclid(1.0);
        let setting = Setting {
            subject,
            from,
            age: t - scene.t0,
            length: scene.duration(),
            phrase_phase,
            follow,
            scene_index: si,
        };
        let mut cam = behave(self.vocab.behaviors[scene.behavior].kind, &setting, &self.dir);
        cam = self.shot(si, t, cam);
        let lyric = lyric::state(&self.lyrics, &self.dir, index);
        if let Some(ls) = &lyric {
            // aim and zoom stay owned by the lyric camera: the word being typed must stay framed
            // and legible. But the scene's own chosen personality (Orbit's sweep, Drift's wander,
            // Follow's tracking) is real motion, not noise, so its tilt and roll ride along on
            // top of the line's own instead of being discarded outright.
            let behave_cam = cam;
            cam = lyric::camera(ls, &self.dir);
            cam.tilt_x += behave_cam.tilt_x;
            cam.tilt_y += behave_cam.tilt_y;
            cam.roll += behave_cam.roll;
        }

        let motion = &self.dir.motion;
        let intro = motion.intro;
        // Pre-drop cinematic arc
        if intro.enabled && t < self.track.drop_t {
            let drop_t = self.track.drop_t.max(0.1);
            let u_pre = (t / drop_t).clamp(0.0, 1.0);
            let arc_phi = u_pre * std::f32::consts::PI;
            cam.roll += intro.roll * arc_phi.sin();
            cam.tilt_y += intro.tilt_y * (1.0 - u_pre);
            cam.tilt_x += intro.tilt_x * (arc_phi * 2.0).sin();
            cam.zoom *= intro.zoom_start + intro.zoom_rise * ease_in_out(u_pre);
            let bar_s = self.track.bar_s * intro.riser_bars;
            if t > drop_t - bar_s {
                let riser = ((t - (drop_t - bar_s)) / bar_s).clamp(0.0, 1.0);
                cam.zoom *= 1.0 + intro.riser_zoom * riser * riser;
                cam.blur = cam.blur.max(intro.riser_blur * riser);
            }
        }

        // Phrase-start stutter: rapid rhythmic camera micro-oscillation on the 1 of every phrase
        let time_in_phrase = phrase_phase * phrase;
        let stutter = motion.phrase_stutter;
        let stutter_s = stutter.duration_s;
        let mut stutter_tear = 0.0;
        let mut stutter_punch = 0.0;
        if stutter.enabled && t >= self.track.drop_t && time_in_phrase < stutter_s {
            let stutter_u = 1.0 - (time_in_phrase / stutter_s);
            let stutter_osc = (time_in_phrase * stutter.hz_x * std::f32::consts::TAU).sin();
            cam.aim.0 += stutter_osc * stutter.aim_x * stutter_u;
            cam.aim.1 += (time_in_phrase * stutter.hz_y * std::f32::consts::TAU).cos() * stutter.aim_y * stutter_u;
            cam.zoom *= 1.0 + stutter_osc * stutter.zoom * stutter_u;
            cam.roll += stutter_osc * stutter.roll * stutter_u;
            stutter_tear = stutter_u * stutter.tear * if (index % 2) == 0 { 1.0 } else { 0.0 };
            stutter_punch = stutter_u * stutter.punch;
        }

        // Zoom-out ending: pull back smoothly to frame the entire stage after impact
        let ending = motion.ending;
        let ending_t0 = self.track.impact_t.min(self.track.duration_s - ending.lead_bars * self.track.bar_s).max(0.0);
        if ending.enabled && t > ending_t0 {
            let u_ending = ease_in_out(((t - ending_t0) / (self.track.duration_s - ending_t0)).clamp(0.0, 1.0));
            let stage_center = self.dir.stage.rect().center();
            let wide_zoom = fit_zoom(self.dir.stage.rect(), self.dir.fit);
            cam.zoom = lerp(cam.zoom, wide_zoom, u_ending * ending.zoom_mix);
            cam.aim.0 = lerp(cam.aim.0, stage_center.0, u_ending * ending.aim_mix);
            cam.aim.1 = lerp(cam.aim.1, stage_center.1, u_ending * ending.aim_mix);
            cam.tilt_x *= 1.0 - u_ending * ending.tilt_decay;
            cam.tilt_y *= 1.0 - u_ending * ending.tilt_decay;
            cam.roll *= 1.0 - u_ending * ending.roll_decay;
        }

        let whip = cam.blur;
        cam.zoom *= 1.0 + moved(Param::Zoom);
        cam.aim.0 += moved(Param::AimX);
        cam.aim.1 += moved(Param::AimY);
        cam.tilt_x += moved(Param::TiltX);
        cam.tilt_y += moved(Param::TiltY);
        cam.roll += moved(Param::Roll);
        let focus = match &lyric {
            Some(ls) => ls.newest().map_or(ls.line_center(), |w| ls.word_center(w)),
            None => follow,
        };
        keep_in_view(&mut cam, focus, &self.dir.camera);
        keep_in_stage(&mut cam, &self.dir.camera);

        let since_start = index as isize - track::output::nearest_frame(scene.t0) as isize;
        let flash = if (0..3).contains(&since_start) { self.dir.cut_flash[since_start as usize] } else { 0.0 };
        let accents = motion.event_accents;
        let impact = (-(t - self.track.impact_t).max(0.0) / accents.impact_decay_s).exp() * f32::from(accents.impact_enabled && t >= self.track.impact_t);
        let fade = self.dir.outro_fade_s;

        // Shot transient kicks (rapid micro-cut kicks from ff-tracking)
        let transient = motion.shot_transient;
        let shot_kick = if transient.enabled && t >= self.track.drop_t && self.dir.shots.mix > 0.0 {
            let len = self.dir.shots.beats * 60.0 / self.track.bpm;
            let a = (t - self.track.drop_t).rem_euclid(len);
            if a < transient.duration_s { (1.0 - a / transient.duration_s) * transient.strength } else { 0.0 }
        } else {
            0.0
        };
        let is_locked = lyric.as_ref().map_or_else(|| self.locked(si, index), |ls| ls.locked);
        let lock_tear = if accents.lock_enabled && is_locked { accents.lock_tear } else { 0.0 };
        let lock_aberr = if accents.lock_enabled && is_locked { accents.lock_aberration } else { 0.0 };

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

        let panels = self.morphed_panels(si, t);
        let regions = panels
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
            focus,
            look: LookState {
                bloom: value(Param::Bloom).clamp(0.0, 1.5),
                led: value(Param::Led).clamp(0.0, 1.0),
                aberration: (value(Param::Aberration) + lock_aberr).clamp(0.0, 1.5),
                grain: value(Param::Grain).clamp(0.0, 1.0),
                vignette: value(Param::Vignette).clamp(0.0, 1.0),
                blur: (value(Param::Blur) + whip).clamp(0.0, 2.0),
                tear: (value(Param::Tear) + stutter_tear + shot_kick * transient.tear_gain + lock_tear).clamp(0.0, 1.5),
                punch: (value(Param::Punch) + stutter_punch + shot_kick * transient.punch_gain).clamp(0.0, 1.5),
                cut: flash.max(impact).max(shot_kick),
                out: ((t - (self.track.duration_s - fade)) / fade).clamp(0.0, 1.0),
                lock: is_locked,
            },
            stops,
            pulse: moved(Param::Pulse).max(0.0),
            emit: (1.0 + m[Param::Emit.index()]).clamp(0.2, 3.0),
            regions,
            screen_material: scene.screen_material,
            panels,
            lyric,
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
                "text": s.text,
            })).collect::<Vec<_>>(),
            "lyrics": self.lyrics.iter().map(|l| serde_json::json!({
                "scene": l.scene, "text": l.text, "t0": l.t0, "t1": l.t1, "word_times": l.word_times,
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
    fn disabled_motion_does_not_leak_into_frames() {
        let mut a = plan();
        a.dir.motion.intro.enabled = false;
        a.dir.motion.phrase_stutter.enabled = false;
        a.dir.motion.ending.enabled = false;
        a.dir.motion.shot_transient.enabled = false;
        a.dir.motion.event_accents.impact_enabled = false;
        a.dir.motion.event_accents.lock_enabled = false;
        let mut b = plan();
        b.dir.motion = a.dir.motion;
        b.dir.motion.intro.zoom_start = 4.0;
        b.dir.motion.phrase_stutter.aim_x = 900.0;
        b.dir.motion.ending.aim_mix = 0.0;
        b.dir.motion.shot_transient.strength = 9.0;
        b.dir.motion.event_accents.impact_decay_s = 9.0;
        b.dir.motion.event_accents.lock_tear = 9.0;
        for f in 0..a.frames() {
            let x = a.frame(f);
            let y = b.frame(f);
            assert_eq!(x.camera, y.camera, "frame {f}");
            assert_eq!(x.look, y.look, "frame {f}");
        }
    }

    #[test]
    fn motion_configuration_rejects_invalid_values_and_typos() {
        let base: serde_json::Value = serde_json::from_str(include_str!("../../../vocab/direction.json")).unwrap();
        for (group, key, value) in [
            ("phrase_stutter", "duration_s", serde_json::json!(0)),
            ("ending", "aim_mix", serde_json::json!(1.1)),
            ("intro", "enabeld", serde_json::json!(false)),
        ] {
            let mut v = base.clone();
            v["motion"][group][key] = value;
            assert!(crate::scenes::Direction::parse(&v.to_string()).is_err());
        }
    }

    #[test]
    fn direction_defaults_preserve_frame_states() {
        use std::hash::{Hash, Hasher};
        let p = plan();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for f in 0..p.frames() {
            let s = p.frame(f);
            format!("{:?}{:?}", s.camera, s.look).hash(&mut hash);
        }
        assert_eq!(hash.finish(), 7079847075976594544, "default camera/look state changed");
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
            let q = project(&c, s.focus, W, H, p.dir.camera.focal).expect("focus is in front of the camera");
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
            &Sources { feel: &text("feel"), vocabulary: &text("vocabulary"), matrix: &text("matrix"), direction: &text("direction"), signals: &text("signals") },
        )
        .unwrap();
        assert!(muted.layouts.iter().all(|l| l.panel("piano").is_none()));
        assert!(full.layouts.iter().all(|l| l.panel("piano").is_some()));
        let note = full.track().stem("piano").unwrap().notes[20];
        let f = (note.onset * FPS as f32).round() as usize;
        assert!(full.frame(f).pulse > muted.frame(f).pulse || full.frame(f).look.bloom > muted.frame(f).look.bloom);
    }

    #[test]
    fn layout_morphing_interpolates_smoothly() {
        let p = plan();
        let s1 = &p.scenes[1];
        let t0 = s1.t0;
        let p_start = p.morphed_panels(1, t0);
        let p_mid = p.morphed_panels(1, t0 + 0.15);
        let p_end = p.morphed_panels(1, t0 + 0.5);
        assert_eq!(p_start[0].rect, p.layouts[0].panels[0].rect);
        assert_eq!(p_end[0].rect, p.layouts[1].panels[0].rect);
        assert!(p_mid[0].rect.x >= p_start[0].rect.x.min(p_end[0].rect.x));
        assert!(p_mid[0].rect.x <= p_start[0].rect.x.max(p_end[0].rect.x));
    }

    #[test]
    fn predrop_arc_and_zoom_out_ending() {
        let p = plan();
        let f0 = p.frame(0);
        let f_pre = p.frame((p.track().drop_t * 0.5 * FPS as f32).round() as usize);
        assert!(f0.camera.tilt_y > 0.0);
        assert!(f_pre.camera.roll != 0.0);
        let f_last = p.frame(p.frames() - 1);
        let stage_center = p.dir.stage.rect().center();
        assert!((f_last.camera.aim.0 - stage_center.0).abs() < 100.0);
    }
}
