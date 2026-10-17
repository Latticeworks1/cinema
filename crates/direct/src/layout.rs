//! Where each stem's panel sits on the stage, and where a note lands inside its panel.
//!
//! Every panel is a window onto one stem's notes. In a linear panel time runs left to right
//! past a fixed playhead and height is register. In a ring panel time runs around a clock
//! face and register is the distance from the centre. The camera frames panels, so a layout
//! change moves the subjects the camera cuts between.

use crate::geometry::Rect;
use crate::settings::Stage;
use crate::vocab::{Rule, Slider};

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub stem: String,
    pub rect: Rect,
    /// Margin inside the panel before the first note position.
    pub pad: f32,
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
    pub fn arrange(rule: Rule, slider: Slider, stems: &[&str], dominant: &str, st: &Stage) -> Layout {
        let n = stems.len();
        let rects: Vec<Rect> = if n <= 1 {
            vec![st.rect(); n]
        } else {
            match rule {
                Rule::Lanes => Layout::lanes(stems, dominant, st),
                Rule::Hero => Layout::hero(stems, dominant, st),
                Rule::Grid => Layout::grid(stems, dominant, st),
            }
        };
        Layout {
            panels: stems.iter().zip(rects).map(|(s, rect)| Panel { stem: s.to_string(), rect, pad: st.pad }).collect(),
            slider,
        }
    }

    fn lanes(stems: &[&str], dominant: &str, st: &Stage) -> Vec<Rect> {
        let stage = st.rect();
        let weights: Vec<f32> = stems.iter().map(|s| if *s == dominant { st.lane_emphasis } else { 1.0 }).collect();
        let total: f32 = weights.iter().sum();
        let free = stage.h - st.gap * (stems.len() - 1) as f32;
        let mut y = stage.y;
        weights
            .iter()
            .map(|w| {
                let h = free * w / total;
                let r = Rect::new(stage.x, y, stage.w, h);
                y += h + st.gap;
                r
            })
            .collect()
    }

    fn hero(stems: &[&str], dominant: &str, st: &Stage) -> Vec<Rect> {
        let stage = st.rect();
        let left = Rect::new(stage.x, stage.y, stage.w * st.hero_share - st.gap * 0.5, stage.h);
        let right_x = left.right() + st.gap;
        let right_w = stage.right() - right_x;
        let others = stems.len() - 1;
        let h = (stage.h - st.gap * (others - 1) as f32) / others as f32;
        let mut k = 0;
        stems
            .iter()
            .map(|s| {
                if *s == dominant {
                    left
                } else {
                    let r = Rect::new(right_x, stage.y + k as f32 * (h + st.gap), right_w, h);
                    k += 1;
                    r
                }
            })
            .collect()
    }

    fn grid(stems: &[&str], dominant: &str, st: &Stage) -> Vec<Rect> {
        let stage = st.rect();
        let rows = (stems.len() + 1).div_ceil(st.grid_cols);
        let cw = (stage.w - st.gap * (st.grid_cols - 1) as f32) / st.grid_cols as f32;
        let ch = (stage.h - st.gap * (rows - 1) as f32) / rows as f32;
        let cell = |c: usize, r: usize, span: usize| {
            Rect::new(
                stage.x + c as f32 * (cw + st.gap),
                stage.y + r as f32 * (ch + st.gap),
                cw * span as f32 + st.gap * (span - 1) as f32,
                ch,
            )
        };
        let mut free = (0..rows).flat_map(|r| (0..st.grid_cols).map(move |c| (c, r))).filter(|&(c, r)| !(r == 0 && c < 2));
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
        self.rect.inset(self.pad)
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
    use crate::scenes::Direction;

    fn st() -> Stage {
        Direction::parse(include_str!("../../../vocab/direction.json")).unwrap().stage
    }

    const STEMS: [&str; 5] = ["piano", "bass", "snare", "kick", "hat"];

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() - 0.01 && b.x < a.right() - 0.01 && a.y < b.bottom() - 0.01 && b.y < a.bottom() - 0.01
    }

    #[test]
    fn every_rule_gives_each_stem_a_panel_inside_the_stage_without_overlap() {
        let stage = st().rect();
        for rule in [Rule::Lanes, Rule::Hero, Rule::Grid] {
            for dominant in STEMS {
                let l = Layout::arrange(rule, Slider::Linear, &STEMS, dominant, &st());
                assert_eq!(l.panels.len(), 5);
                for p in &l.panels {
                    assert!(p.rect.x >= stage.x - 0.01 && p.rect.right() <= stage.right() + 0.01, "{rule:?} {p:?}");
                    assert!(p.rect.y >= stage.y - 0.01 && p.rect.bottom() <= stage.bottom() + 0.01, "{rule:?} {p:?}");
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
            let l = Layout::arrange(rule, Slider::Linear, &STEMS, "bass", &st());
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
                let l = Layout::arrange(rule, Slider::Ring, &STEMS[..n], STEMS[0], &st());
                assert_eq!(l.panels.len(), n);
            }
        }
    }

    #[test]
    fn a_note_at_the_playhead_sits_on_it_and_later_notes_lie_to_the_right() {
        let l = Layout::arrange(Rule::Lanes, Slider::Linear, &STEMS, "piano", &st());
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
        let l = Layout::arrange(Rule::Hero, Slider::Ring, &STEMS, "piano", &st());
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
