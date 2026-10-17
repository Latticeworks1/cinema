//! Where each stem's panel sits on the stage, and where a note lands inside its panel.
//!
//! Every panel is a window onto one stem's notes. In a linear panel time runs left to right
//! past a fixed playhead and height is register. In a ring panel time runs around a clock
//! face and register is the distance from the centre. The camera frames panels, so a layout
//! change moves the subjects the camera cuts between.

use crate::geometry::Rect;
use crate::vocab::{Rule, Slider};

/// The area of the 1920x1080 screen the panels share.
pub const STAGE: Rect = Rect { x: 90.0, y: 120.0, w: 1740.0, h: 840.0 };
const GAP: f32 = 12.0;
/// How much taller a lane of the leading stem is than the others.
const LANE_EMPHASIS: f32 = 2.2;
/// Share of the stage width the leading stem takes in the hero layout.
const HERO_SHARE: f32 = 0.62;
const GRID_COLS: usize = 3;
/// Margin inside a panel before the first note position.
const PAD: f32 = 14.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub stem: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub panels: Vec<Panel>,
    pub slider: Slider,
}

/// How notes are placed in time inside a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    /// Seconds across a panel (linear) or around half a ring.
    pub span_s: f32,
    /// Where the playhead sits across a linear panel, 0..1.
    pub now_frac: f32,
}

impl Layout {
    /// Arrange `stems` (display order) with `dominant` given the larger share.
    pub fn arrange(rule: Rule, slider: Slider, stems: &[&str], dominant: &str) -> Layout {
        let n = stems.len();
        let rects: Vec<Rect> = if n <= 1 {
            vec![STAGE; n]
        } else {
            match rule {
                Rule::Lanes => Layout::lanes(stems, dominant),
                Rule::Hero => Layout::hero(stems, dominant),
                Rule::Grid => Layout::grid(stems, dominant),
            }
        };
        Layout {
            panels: stems.iter().zip(rects).map(|(s, rect)| Panel { stem: s.to_string(), rect }).collect(),
            slider,
        }
    }

    fn lanes(stems: &[&str], dominant: &str) -> Vec<Rect> {
        let weights: Vec<f32> = stems.iter().map(|s| if *s == dominant { LANE_EMPHASIS } else { 1.0 }).collect();
        let total: f32 = weights.iter().sum();
        let free = STAGE.h - GAP * (stems.len() - 1) as f32;
        let mut y = STAGE.y;
        weights
            .iter()
            .map(|w| {
                let h = free * w / total;
                let r = Rect::new(STAGE.x, y, STAGE.w, h);
                y += h + GAP;
                r
            })
            .collect()
    }

    fn hero(stems: &[&str], dominant: &str) -> Vec<Rect> {
        let left = Rect::new(STAGE.x, STAGE.y, STAGE.w * HERO_SHARE - GAP * 0.5, STAGE.h);
        let right_x = left.right() + GAP;
        let right_w = STAGE.right() - right_x;
        let others = stems.len() - 1;
        let h = (STAGE.h - GAP * (others - 1) as f32) / others as f32;
        let mut k = 0;
        stems
            .iter()
            .map(|s| {
                if *s == dominant {
                    left
                } else {
                    let r = Rect::new(right_x, STAGE.y + k as f32 * (h + GAP), right_w, h);
                    k += 1;
                    r
                }
            })
            .collect()
    }

    fn grid(stems: &[&str], dominant: &str) -> Vec<Rect> {
        let rows = (stems.len() + 1).div_ceil(GRID_COLS);
        let cw = (STAGE.w - GAP * (GRID_COLS - 1) as f32) / GRID_COLS as f32;
        let ch = (STAGE.h - GAP * (rows - 1) as f32) / rows as f32;
        let cell = |c: usize, r: usize, span: usize| {
            Rect::new(
                STAGE.x + c as f32 * (cw + GAP),
                STAGE.y + r as f32 * (ch + GAP),
                cw * span as f32 + GAP * (span - 1) as f32,
                ch,
            )
        };
        let mut free = (0..rows).flat_map(|r| (0..GRID_COLS).map(move |c| (c, r))).filter(|&(c, r)| !(r == 0 && c < 2));
        stems
            .iter()
            .map(|s| {
                if *s == dominant {
                    cell(0, 0, 2)
                } else {
                    let (c, r) = free.next().expect("grid has a cell for every stem");
                    cell(c, r, 1)
                }
            })
            .collect()
    }

    pub fn panel(&self, stem: &str) -> Option<&Panel> {
        self.panels.iter().find(|p| p.stem == stem)
    }
}

impl Panel {
    pub fn inner(&self) -> Rect {
        self.rect.inset(PAD)
    }

    /// Where a note starting at `t` with register `register` (0..1) sits when it is `now`.
    pub fn note_pos(&self, slider: Slider, tl: &Timeline, now: f32, t: f32, register: f32) -> (f32, f32) {
        let inner = self.inner();
        match slider {
            Slider::Linear => (
                inner.x + inner.w * (tl.now_frac + (t - now) / tl.span_s),
                inner.bottom() - register * inner.h,
            ),
            Slider::Ring => {
                let (cx, cy) = inner.center();
                let rmax = 0.5 * inner.w.min(inner.h);
                let angle = -std::f32::consts::FRAC_PI_2 + (t - now) / tl.span_s * std::f32::consts::PI;
                let r = rmax * (0.3 + 0.7 * register);
                (cx + r * angle.cos(), cy + r * angle.sin())
            }
        }
    }

    /// The panel's playhead position at height `y` of the panel (linear) or its centre (ring).
    pub fn playhead_x(&self, tl: &Timeline) -> f32 {
        let inner = self.inner();
        inner.x + inner.w * tl.now_frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEMS: [&str; 5] = ["piano", "bass", "snare", "kick", "hat"];

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() - 0.01 && b.x < a.right() - 0.01 && a.y < b.bottom() - 0.01 && b.y < a.bottom() - 0.01
    }

    #[test]
    fn every_rule_gives_each_stem_a_panel_inside_the_stage_without_overlap() {
        for rule in [Rule::Lanes, Rule::Hero, Rule::Grid] {
            for dominant in STEMS {
                let l = Layout::arrange(rule, Slider::Linear, &STEMS, dominant);
                assert_eq!(l.panels.len(), 5);
                for p in &l.panels {
                    assert!(p.rect.x >= STAGE.x - 0.01 && p.rect.right() <= STAGE.right() + 0.01, "{rule:?} {p:?}");
                    assert!(p.rect.y >= STAGE.y - 0.01 && p.rect.bottom() <= STAGE.bottom() + 0.01, "{rule:?} {p:?}");
                }
                for (i, a) in l.panels.iter().enumerate() {
                    for b in &l.panels[i + 1..] {
                        assert!(!overlaps(&a.rect, &b.rect), "{rule:?} {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_leading_stem_has_the_largest_panel() {
        for rule in [Rule::Lanes, Rule::Hero, Rule::Grid] {
            let l = Layout::arrange(rule, Slider::Linear, &STEMS, "bass");
            let area = |s: &str| {
                let r = l.panel(s).unwrap().rect;
                r.w * r.h
            };
            assert!(STEMS.iter().filter(|s| **s != "bass").all(|s| area("bass") > area(s)), "{rule:?}");
        }
    }

    #[test]
    fn fewer_stems_still_lay_out() {
        for n in 1..=5 {
            for rule in [Rule::Lanes, Rule::Hero, Rule::Grid] {
                let l = Layout::arrange(rule, Slider::Ring, &STEMS[..n], STEMS[0]);
                assert_eq!(l.panels.len(), n);
            }
        }
    }

    #[test]
    fn a_note_at_the_playhead_sits_on_it_and_later_notes_lie_to_the_right() {
        let l = Layout::arrange(Rule::Lanes, Slider::Linear, &STEMS, "piano");
        let p = l.panel("piano").unwrap();
        let tl = Timeline { span_s: 5.0, now_frac: 0.35 };
        let now = p.note_pos(Slider::Linear, &tl, 10.0, 10.0, 0.5);
        let later = p.note_pos(Slider::Linear, &tl, 10.0, 11.0, 0.5);
        assert!((now.0 - p.playhead_x(&tl)).abs() < 1e-3);
        assert!(later.0 > now.0);
        assert!(p.note_pos(Slider::Linear, &tl, 10.0, 10.0, 1.0).1 < p.note_pos(Slider::Linear, &tl, 10.0, 10.0, 0.0).1);
    }

    #[test]
    fn ring_notes_stay_inside_the_panel_and_circle_with_time() {
        let l = Layout::arrange(Rule::Hero, Slider::Ring, &STEMS, "piano");
        let p = l.panel("piano").unwrap();
        let tl = Timeline { span_s: 5.0, now_frac: 0.35 };
        for k in 0..=20 {
            let q = p.note_pos(Slider::Ring, &tl, 10.0, 10.0 + (k as f32 - 10.0) * 0.5, k as f32 / 20.0);
            assert!(p.rect.contains(q), "{q:?}");
        }
        let a = p.note_pos(Slider::Ring, &tl, 10.0, 10.0, 0.5);
        let b = p.note_pos(Slider::Ring, &tl, 10.0, 12.5, 0.5);
        assert!(a != b);
    }
}
