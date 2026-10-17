//! The visual vocabulary: every shape, palette, camera behaviour, look, display material and
//! layout the director may choose, each placed on the feel axes (edge, energy).
//!
//! The director picks the entry nearest to a scene's measured feel. Nothing here is chosen by
//! time or by name in code, so a new entry is data and is picked up wherever its position fits.

use crate::matrix::{N_PARAMS, Param};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Position on the feel axes.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct At {
    pub edge: f32,
    pub energy: f32,
}

impl At {
    pub fn distance(self, other: At) -> f32 {
        ((self.edge - other.edge).powi(2) + (self.energy - other.energy).powi(2)).sqrt()
    }
}

pub trait Placed {
    fn name(&self) -> &str;
    fn at(&self) -> At;
}

/// Index of the entry nearest to `to`; `avoid` is penalised so neighbouring scenes differ.
pub fn nearest<T: Placed>(items: &[T], to: At, avoid: Option<usize>, penalty: f32) -> usize {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| (i, it.at().distance(to) + if Some(i) == avoid { penalty } else { 0.0 }))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .expect("vocabulary list is not empty")
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StemDef {
    pub name: String,
    /// Brightness of the stem's marks in the flat pass; the palette maps it to colour.
    pub ink: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EmitKind {
    Dot,
    Shard,
    Ring,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Emit {
    pub kind: EmitKind,
    /// Particles per note at emission 1.
    pub count: u32,
    pub speed: f32,
    pub life_s: f32,
    pub size: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub name: String,
    pub at: At,
    /// 0 = sharp corners, 1 = fully rounded ends.
    pub corner: f32,
    pub emit: Emit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPalette {
    name: String,
    at: At,
    stops: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub name: String,
    pub at: At,
    /// Five colours from black (0) to white (1) of the flat pass, sRGB 0..1.
    pub stops: [[f32; 3]; 5],
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// The panel of the scene's leading stem fills the frame.
    Dominant,
    /// The whole stage.
    Wide,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BehaviorKind {
    /// A still frame.
    Hold { zoom: f32 },
    /// A slow move in or out over the scene.
    Push { from: f32, to: f32 },
    /// Tilt that circles once per phrase.
    Orbit { zoom: f32, tilt: f32, roll: f32 },
    /// Arrive from the previous scene's subject in a quick swing.
    Whip { zoom: f32 },
    /// Handheld wander, `amp` logical pixels.
    Drift { zoom: f32, amp: f32 },
    /// Aim tracks the newest note of the leading stem.
    Follow { zoom: f32 },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Behavior {
    pub name: String,
    pub at: At,
    pub framing: Framing,
    pub kind: BehaviorKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLook {
    name: String,
    at: At,
    #[serde(default)]
    base: std::collections::HashMap<String, f32>,
    #[serde(default)]
    scale: std::collections::HashMap<String, f32>,
}

/// Resting values of the look parameters and how strongly the matrix may move each of them.
#[derive(Clone, Debug)]
pub struct Look {
    pub name: String,
    pub at: At,
    pub base: [f32; N_PARAMS],
    pub scale: [f32; N_PARAMS],
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayLayout {
    /// Vertical RGB stripes in rounded rows.
    Stripe,
    /// Round dots on a square grid.
    Dot,
    /// Rows offset by half a cell with a triad per cell.
    Delta,
    /// Hard square cells with a dark gap.
    Block,
}

/// The physical display a region of the screen is filmed on.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub name: String,
    pub at: At,
    pub layout: DisplayLayout,
    /// Cell size in logical pixels.
    pub cell: f32,
    /// Dark share of a cell, 0..0.6.
    pub gap: f32,
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    Lanes,
    Hero,
    Grid,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Slider {
    /// Time runs left to right past a fixed playhead.
    Linear,
    /// Time runs around a ring like a clock face.
    Ring,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutDef {
    pub name: String,
    pub at: At,
    pub rule: Rule,
    pub slider: Slider,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVocabulary {
    stems: Vec<StemDef>,
    shapes: Vec<Shape>,
    palettes: Vec<RawPalette>,
    behaviors: Vec<Behavior>,
    looks: Vec<RawLook>,
    materials: Vec<Material>,
    layouts: Vec<LayoutDef>,
}

#[derive(Clone, Debug)]
pub struct Vocabulary {
    /// Display order, top to bottom.
    pub stems: Vec<StemDef>,
    pub shapes: Vec<Shape>,
    pub palettes: Vec<Palette>,
    pub behaviors: Vec<Behavior>,
    pub looks: Vec<Look>,
    pub materials: Vec<Material>,
    pub layouts: Vec<LayoutDef>,
}

macro_rules! placed {
    ($($t:ty),*) => {$(impl Placed for $t {
        fn name(&self) -> &str { &self.name }
        fn at(&self) -> At { self.at }
    })*};
}
placed!(Shape, Palette, Behavior, Look, Material, LayoutDef);

fn hex(s: &str) -> Result<[f32; 3], String> {
    let h = s.strip_prefix('#').filter(|h| h.len() == 6).ok_or_else(|| format!("`{s}` is not #rrggbb"))?;
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("`{s}` is not #rrggbb"))?;
    Ok([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

fn unique<T: Placed>(kind: &str, items: &[T]) -> Result<(), String> {
    if items.is_empty() {
        return Err(format!("vocabulary has no {kind}"));
    }
    let mut seen = HashSet::new();
    for it in items {
        if !seen.insert(it.name().to_string()) {
            return Err(format!("{kind}: duplicate name `{}`", it.name()));
        }
        let at = it.at();
        if !(at.edge.abs() <= 1.0 && at.energy.abs() <= 1.0) {
            return Err(format!("{kind} `{}`: position must lie in -1..1", it.name()));
        }
    }
    Ok(())
}

impl Vocabulary {
    /// Parse and check. `track_stems` must all be listed, so no stem is left without a panel.
    pub fn parse(json: &str, track_stems: &[&str]) -> Result<Vocabulary, String> {
        let raw: RawVocabulary = serde_json::from_str(json).map_err(|e| format!("vocabulary: {e}"))?;
        let listed: HashSet<&str> = raw.stems.iter().map(|s| s.name.as_str()).collect();
        if listed.len() != raw.stems.len() {
            return Err("vocabulary: duplicate stem".into());
        }
        if let Some(missing) = track_stems.iter().find(|s| !listed.contains(**s)) {
            return Err(format!("vocabulary: the track has a stem `{missing}` that the vocabulary does not list"));
        }
        let palettes = raw
            .palettes
            .into_iter()
            .map(|p| {
                if p.stops.len() != 5 {
                    return Err(format!("palette `{}` needs five stops", p.name));
                }
                let c: Result<Vec<[f32; 3]>, String> = p.stops.iter().map(|s| hex(s)).collect();
                let c = c.map_err(|e| format!("palette `{}`: {e}", p.name))?;
                Ok(Palette { name: p.name, at: p.at, stops: [c[0], c[1], c[2], c[3], c[4]] })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let looks = raw
            .looks
            .into_iter()
            .map(|l| {
                let mut base = [0.0; N_PARAMS];
                let mut scale = [1.0; N_PARAMS];
                for (k, v) in &l.base {
                    base[Param::parse(k).ok_or_else(|| format!("look `{}`: no parameter `{k}`", l.name))?.index()] = *v;
                }
                for (k, v) in &l.scale {
                    scale[Param::parse(k).ok_or_else(|| format!("look `{}`: no parameter `{k}`", l.name))?.index()] = *v;
                }
                Ok(Look { name: l.name, at: l.at, base, scale })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for s in &raw.stems {
            if !(0.0..=1.0).contains(&s.ink) {
                return Err(format!("stem `{}`: ink must lie in 0..1", s.name));
            }
        }
        for m in &raw.materials {
            if !(m.cell >= 1.0 && (0.0..=0.6).contains(&m.gap)) {
                return Err(format!("material `{}`: cell must be at least 1 and gap within 0..0.6", m.name));
            }
        }
        unique("shapes", &raw.shapes)?;
        unique("palettes", &palettes)?;
        unique("behaviors", &raw.behaviors)?;
        unique("looks", &looks)?;
        unique("materials", &raw.materials)?;
        unique("layouts", &raw.layouts)?;
        Ok(Vocabulary {
            stems: raw.stems,
            shapes: raw.shapes,
            palettes,
            behaviors: raw.behaviors,
            looks,
            materials: raw.materials,
            layouts: raw.layouts,
        })
    }

    pub fn declared_stems(&self) -> HashSet<String> {
        self.stems.iter().map(|s| s.name.clone()).collect()
    }

    pub fn stem_index(&self, name: &str) -> Option<usize> {
        self.stems.iter().position(|s| s.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const TRACK_STEMS: [&str; 5] = ["bass", "hat", "kick", "piano", "snare"];

    pub fn load() -> Vocabulary {
        Vocabulary::parse(include_str!("../../../vocab/vocabulary.json"), &TRACK_STEMS).unwrap()
    }

    #[test]
    fn the_shipped_vocabulary_loads_and_lists_every_stem() {
        let v = load();
        assert_eq!(v.stems.len(), 5);
        assert!(v.palettes.iter().all(|p| p.stops.iter().flatten().all(|c| (0.0..=1.0).contains(c))));
    }

    #[test]
    fn a_stem_the_vocabulary_omits_is_refused() {
        let mut stems = TRACK_STEMS.to_vec();
        stems.push("choir");
        let err = Vocabulary::parse(include_str!("../../../vocab/vocabulary.json"), &stems).unwrap_err();
        assert!(err.contains("choir"), "{err}");
    }

    #[test]
    fn nearest_picks_by_distance_and_penalises_repeats() {
        let v = load();
        let to = v.palettes[1].at;
        assert_eq!(nearest(&v.palettes, to, None, 0.5), 1);
        assert_ne!(nearest(&v.palettes, to, Some(1), 10.0), 1);
    }

    #[test]
    fn bad_colours_and_parameters_are_refused() {
        let text = include_str!("../../../vocab/vocabulary.json");
        assert!(Vocabulary::parse(&text.replacen("#", "x", 1), &TRACK_STEMS).is_err());
        assert!(Vocabulary::parse(&text.replacen("\"bloom\"", "\"blooom\"", 1), &TRACK_STEMS).is_err());
    }
}
