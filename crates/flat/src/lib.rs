//! Pass 1: the stage, flat and colourless.
//!
//! Each stem is a panel (see `direct::layout`) showing its notes around the playhead. Notes are
//! marks whose ends are sharp or round according to the scene's shape for that stem, and each
//! onset throws the shape's particles. The only colour is pure red (#ff0000), which marks a
//! lock on the leading stem and survives the lens's palette.
//!
//! Drawn in the 1920x1080 logical space at 2x so the lens can magnify it.

mod draw;

use direct::Plan;
use fframes::{AudioMap, Color, Duration, FFramesContext, Frame, Svgr, Video};

pub const WIDTH: usize = 3840;
pub const HEIGHT: usize = 2160;

pub struct FlatVideo {
    plan: Plan,
}

impl FlatVideo {
    pub fn new(plan: Plan) -> Self {
        Self { plan }
    }

    /// The SVG of one frame, for inspection and tests.
    pub fn artwork(&self, frame: usize) -> String {
        draw::artwork(&self.plan, frame)
    }
}

impl Video for FlatVideo {
    const FPS: usize = track::FPS;
    const WIDTH: usize = WIDTH;
    const HEIGHT: usize = HEIGHT;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(self.plan.frames())
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, frame: Frame, _ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        Svgr::from(self.artwork(frame.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video() -> FlatVideo {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        FlatVideo::new(Plan::load(format!("{root}/data/analysis.json"), format!("{root}/vocab")).unwrap())
    }

    #[test]
    fn every_frame_is_wellformed_svg_without_non_finite_numbers() {
        let v = video();
        for f in (0..v.plan.frames()).step_by(3) {
            let s = v.artwork(f);
            assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "frame {f}");
            assert!(!s.contains("NaN") && !s.contains("inf"), "frame {f}");
        }
    }

    #[test]
    fn frames_are_deterministic() {
        let v = video();
        assert_eq!(v.artwork(401), v.artwork(401));
        assert_ne!(v.artwork(401), v.artwork(402));
    }

    #[test]
    fn a_lock_frame_carries_the_red_marker_and_a_quiet_frame_does_not() {
        let v = video();
        let lock = (0..v.plan.frames()).find(|&f| v.plan.frame(f).look.lock).expect("a lock frame");
        let quiet = (0..v.plan.frames()).find(|&f| !v.plan.frame(f).look.lock).unwrap();
        assert!(v.artwork(lock).contains("#ff0000"));
        assert!(!v.artwork(quiet).contains("#ff0000"));
    }

    #[test]
    fn every_stem_present_gets_a_panel_label() {
        let v = video();
        let s = v.artwork(300);
        for stem in v.plan.track().stem_names() {
            assert!(s.contains(&format!(">{stem}")), "no label for {stem}");
        }
    }
}
