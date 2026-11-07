//! Recognizable motifs found in the plate images (python/image_anchors.py), and where they fall
//! on the stage once a plate is placed.
//!
//! `packet prepare` writes `<run>/anchors.json`: for each image, boxes in image-normalized
//! coordinates with a label and a detector score. [`Anchors::load`] reads it strictly, and
//! [`Plan::anchor_marks`] places every box of every plate that is on screen in the flat's world
//! coordinates, so the lens can draw a recognition square on each.

use crate::{Direction, FrameState, Rect, settings::Plate};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub id: String,
    pub label: String,
    pub score: f32,
    #[serde(rename = "box")]
    pub rect: [f32; 4],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAnchors {
    /// Pixel size of the image, which fixes how a plate's `fit` maps it onto the plate.
    pub size: [u32; 2],
    pub boxes: Vec<Anchor>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Anchors {
    pub schema: u32,
    pub images: BTreeMap<String, ImageAnchors>,
}

impl Anchors {
    pub fn load(path: impl AsRef<Path>) -> Result<Anchors, String> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let a: Anchors = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if a.schema != SCHEMA {
            return Err(format!("{}: schema {} but this build reads {SCHEMA}", path.display(), a.schema));
        }
        for (name, im) in &a.images {
            let ok = im.size[0] > 0
                && im.size[1] > 0
                && im.boxes.iter().all(|b| {
                    let r = b.rect;
                    (0.0..=1.0).contains(&r[0]) && (0.0..=1.0).contains(&r[1]) && r[2] <= 1.0 && r[3] <= 1.0 && r[0] < r[2] && r[1] < r[3]
                });
            if !ok {
                return Err(format!("{}: {name} has a degenerate size or box", path.display()));
            }
        }
        Ok(a)
    }
}

/// One anchor placed in the flat's world coordinates.
#[derive(Clone, Debug)]
pub struct Mark {
    pub label: String,
    pub score: f32,
    /// Centre and half extents on the stage, before the plate's rotation is applied to the centre.
    pub center: (f32, f32),
    pub half: (f32, f32),
    /// The plate's own strength this frame, 0..1.
    pub alpha: f32,
}

/// Where a plate sits at one instant: centre, size, rotation in degrees and strength.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
    pub alpha: f32,
}

/// The rectangle the camera sees, in stage units: a zoom about its aim point.
pub fn view_rect(st: &FrameState) -> Rect {
    let z = st.camera.zoom.max(0.01);
    let (vw, vh) = (crate::geometry::W / z, crate::geometry::H / z);
    Rect::new(st.camera.aim.0 - 0.5 * vw, st.camera.aim.1 - 0.5 * vh, vw, vh)
}

impl Plate {
    /// The plate's place at this frame, or none when it is not on screen.
    pub fn placed(&self, dir: &Direction, st: &FrameState) -> Option<Placed> {
        let into = st.t - self.t0;
        let left = self.t1.map_or(f32::INFINITY, |t1| t1 - st.t);
        if into < 0.0 || left < 0.0 {
            return None;
        }
        let lyric_dim = if st.lyric.is_some() { dir.lyric.dim } else { 1.0 };
        let edge = if self.fade_s > 0.0 { (into / self.fade_s).min(left / self.fade_s).min(1.0) } else { 1.0 };
        let alpha = self.opacity * edge * (1.0 - self.lyric_dim * (1.0 - lyric_dim));
        if alpha <= 0.0 {
            return None;
        }
        let stage = if self.space == "view" { view_rect(st) } else { dir.stage.rect() };
        let swell = 1.0 + self.pulse * st.pulse;
        Some(Placed {
            cx: stage.x + (self.x + self.w * 0.5 + self.drift.0 * into) * stage.w,
            cy: stage.y + (self.y + self.h * 0.5 + self.drift.1 * into) * stage.h,
            w: self.w * stage.w * swell,
            h: self.h * stage.h * swell,
            rot: self.rot,
            alpha,
        })
    }

    /// Anchors of this plate's image in world coordinates. The image fills the plate (`slice`) or
    /// sits whole inside it (`meet`); a box whose centre falls outside the plate is dropped, and the
    /// rest are clipped to it. The plate's rotation turns each centre about the plate centre.
    pub fn marks(&self, at: &Placed, im: &ImageAnchors) -> Vec<Mark> {
        let (iw, ih) = (im.size[0] as f32, im.size[1] as f32);
        let k = if self.fit == "meet" { (at.w / iw).min(at.h / ih) } else { (at.w / iw).max(at.h / ih) };
        let (dw, dh) = (iw * k, ih * k);
        let (sin, cos) = at.rot.to_radians().sin_cos();
        let mut out = vec![];
        for b in &im.boxes {
            // plate-local coordinates, origin at the plate centre
            let (x0, x1) = ((b.rect[0] - 0.5) * dw, (b.rect[2] - 0.5) * dw);
            let (y0, y1) = ((b.rect[1] - 0.5) * dh, (b.rect[3] - 0.5) * dh);
            let (hw, hh) = (0.5 * at.w, 0.5 * at.h);
            let (cx, cy) = (0.5 * (x0 + x1), 0.5 * (y0 + y1));
            if cx.abs() > hw || cy.abs() > hh {
                continue;
            }
            let (x0, x1, y0, y1) = (x0.max(-hw), x1.min(hw), y0.max(-hh), y1.min(hh));
            let (cx, cy) = (0.5 * (x0 + x1), 0.5 * (y0 + y1));
            out.push(Mark {
                label: b.label.clone(),
                score: b.score,
                center: (at.cx + cx * cos - cy * sin, at.cy + cx * sin + cy * cos),
                half: (0.5 * (x1 - x0), 0.5 * (y1 - y0)),
                alpha: at.alpha,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(fit: &str, rot: f32) -> Plate {
        serde_json::from_str(&format!(r#"{{"image":"a.png","x":0,"y":0,"w":0.5,"h":0.5,"fit":"{fit}","rot":{rot}}}"#)).unwrap()
    }

    fn one(rect: [f32; 4]) -> ImageAnchors {
        ImageAnchors { size: [200, 100], boxes: vec![Anchor { id: "a0".into(), label: "x".into(), score: 0.9, rect }] }
    }

    #[test]
    fn a_box_in_the_middle_of_the_image_lands_on_the_plate_centre() {
        let at = Placed { cx: 500.0, cy: 300.0, w: 400.0, h: 400.0, rot: 0.0, alpha: 1.0 };
        let m = plate("meet", 0.0).marks(&at, &one([0.4, 0.4, 0.6, 0.6]));
        assert_eq!(m.len(), 1);
        assert!((m[0].center.0 - 500.0).abs() < 1e-3 && (m[0].center.1 - 300.0).abs() < 1e-3);
        // meet: 200x100 image scaled by 2 is 400x200; the box is 0.2 of the width = 80 wide
        assert!((m[0].half.0 - 40.0).abs() < 1e-3);
    }

    #[test]
    fn slice_crops_boxes_that_leave_the_plate_and_drops_those_outside() {
        let at = Placed { cx: 0.0, cy: 0.0, w: 100.0, h: 100.0, rot: 0.0, alpha: 1.0 };
        // slice scales the 200x100 image by 1.0 to cover it: 200 wide, of which the middle 100 is inside.
        // Local x runs 0..80 for this box, so the plate edge at 50 clips it to 0..50.
        let kept = plate("slice", 0.0).marks(&at, &one([0.5, 0.0, 0.9, 1.0]));
        assert_eq!(kept.len(), 1);
        assert!((kept[0].half.0 - 25.0).abs() < 1e-3 && (kept[0].center.0 - 25.0).abs() < 1e-3);
        assert!(plate("slice", 0.0).marks(&at, &one([0.0, 0.0, 0.1, 1.0])).is_empty());
    }

    #[test]
    fn rotation_turns_the_centre_about_the_plate_centre() {
        let at = Placed { cx: 0.0, cy: 0.0, w: 400.0, h: 400.0, rot: 90.0, alpha: 1.0 };
        let m = plate("meet", 90.0).marks(&at, &one([0.75, 0.45, 0.85, 0.55]));
        // local (+120, 0) turned 90 degrees clockwise on a y-down screen is (0, +120)
        assert!(m[0].center.0.abs() < 1e-3 && (m[0].center.1 - 120.0).abs() < 1e-3);
    }
}
