//! Pass 1: the stage, flat and colourless.
//!
//! Each stem is a panel (see `direct::layout`) showing its notes around the playhead. Notes are
//! marks whose ends are sharp or round according to the scene's shape for that stem, and each
//! onset throws the shape's particles. Below the stage a context band carries the time ruler,
//! the master waveform and the live spectrum (see `chrome`). The only colour is pure red
//! (#ff0000), which marks a lock on the leading stem and survives the lens's palette.
//!
//! Drawn in the 1920x1080 logical space at 2x so the lens can magnify it.

mod chrome;
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

    /// The SVG of one frame, for inspection and tests. `bands` is the spectrum at that frame
    /// (see `chrome::spectrum`); None draws no spectrum strip.
    pub fn artwork(&self, frame: usize, bands: Option<&[f32]>) -> String {
        draw::artwork(&self.plan, frame, bands)
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

    fn render_frame<'a>(&'a self, frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let bands = chrome::spectrum(&self.plan, &frame, ctx);
        Svgr::from(self.artwork(frame.index, bands.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use direct::Sources;
    use track::Track;

    fn root() -> &'static str {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../..")
    }

    fn video() -> FlatVideo {
        FlatVideo::new(Plan::load(format!("{}/data/analysis.json", root()), format!("{}/vocab", root())).unwrap())
    }

    /// The same plan with the track's soundtrack removed, as a track without audio.
    fn video_without_audio() -> FlatVideo {
        let text = std::fs::read_to_string(format!("{}/data/analysis.json", root())).unwrap();
        let text = text.replacen("\"audio_file\": \"data/audio.wav\"", "\"audio_file\": \"\"", 1);
        let track = Track::parse(&text).unwrap();
        assert!(!track.has_audio());
        let read = |name: &str| std::fs::read_to_string(format!("{}/vocab/{name}", root())).unwrap();
        let (feel, vocabulary, matrix, direction, signals) =
            (read("feel.json"), read("vocabulary.json"), read("matrix.json"), read("direction.json"), read("signals.json"));
        let src = Sources { feel: &feel, vocabulary: &vocabulary, matrix: &matrix, direction: &direction, signals: &signals };
        FlatVideo::new(Plan::build(track, &src).unwrap())
    }

    /// The text between an element with this id and its closing tag, empty when absent.
    fn group<'a>(s: &'a str, id: &str) -> &'a str {
        let Some(at) = s.find(&format!("id='{id}'")) else { return "" };
        let rest = &s[at..];
        &rest[..rest.find("</g>").map_or(rest.len(), |e| e + 4)]
    }

    fn bands(n: usize) -> Vec<f32> {
        (0..n).map(|i| i as f32 / n as f32).collect()
    }

    /// A frame a few bars after the drop, so the ruler and the waveform both have content.
    fn frame_after_drop(v: &FlatVideo) -> usize {
        let tr = v.plan.track();
        (tr.drop_t * track::FPS as f32) as usize + (3.0 * tr.bar_s * track::FPS as f32) as usize
    }

    #[test]
    fn every_frame_is_wellformed_svg_without_non_finite_numbers() {
        let v = video();
        let b = bands(v.plan.dir.chrome.fft_bands);
        for f in (0..v.plan.frames()).step_by(3) {
            for bs in [None, Some(b.as_slice())] {
                let s = v.artwork(f, bs);
                assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "frame {f}");
                assert!(!s.contains("NaN") && !s.contains("inf"), "frame {f}");
            }
        }
    }

    #[test]
    fn frames_are_deterministic() {
        let v = video();
        assert_eq!(v.artwork(401, None), v.artwork(401, None));
        assert_ne!(v.artwork(401, None), v.artwork(402, None));
    }

    #[test]
    fn a_lock_frame_carries_the_red_marker_and_a_quiet_frame_does_not() {
        let v = video();
        let lock = (0..v.plan.frames()).find(|&f| v.plan.frame(f).look.lock).expect("a lock frame");
        let quiet = (0..v.plan.frames()).find(|&f| !v.plan.frame(f).look.lock).unwrap();
        assert!(v.artwork(lock, None).contains("#ff0000"));
        assert!(!v.artwork(quiet, None).contains("#ff0000"));
    }

    #[test]
    fn every_stem_present_gets_a_panel_label() {
        let v = video();
        let s = v.artwork(300, None);
        for stem in v.plan.track().stem_names() {
            assert!(s.contains(&format!(">{stem}")), "no label for {stem}");
        }
    }

    #[test]
    fn the_ruler_labels_its_bars_after_the_drop() {
        let v = video();
        let s = v.artwork(frame_after_drop(&v), None);
        assert!(!group(&s, "ruler").is_empty(), "no ruler");
        assert!(group(&s, "ruler").contains(">bar "), "no bar label in the ruler");
    }

    #[test]
    fn the_waveform_is_lit_before_the_playhead_and_dim_after() {
        let v = video();
        let s = v.artwork(frame_after_drop(&v), None);
        let wave = group(&s, "wave");
        assert!(!wave.is_empty(), "no waveform");
        assert!(wave.contains("fill='#e6e6e6'"), "no lit column before the playhead");
        assert!(wave.contains("fill='#666666'"), "no dim column after the playhead");
    }

    #[test]
    fn the_meter_and_the_status_line_are_in_the_frame() {
        let v = video();
        let s = v.artwork(frame_after_drop(&v), None);
        assert!(!group(&s, "meter").is_empty(), "no meter");
        let status = group(&s, "status");
        assert!(status.contains("BPM") && status.contains("bar ") && status.contains("dBFS"), "{status}");
        assert!(status.contains("beat ") && status.contains("frame "), "{status}");
    }

    #[test]
    fn the_spectrum_strip_is_drawn_only_with_bands_and_audio() {
        let v = video();
        let f = frame_after_drop(&v);
        let b = bands(v.plan.dir.chrome.fft_bands);
        assert!(group(&v.artwork(f, Some(&b)), "fft").contains("<rect"), "no spectrum strip");
        assert!(group(&v.artwork(f, None), "fft").is_empty(), "strip drawn without bands");
    }

    #[test]
    fn the_spectrum_strip_is_not_drawn_without_audio() {
        let v = video_without_audio();
        let f = frame_after_drop(&v);
        let b = bands(v.plan.dir.chrome.fft_bands);
        assert!(!chrome::fft_shown(&v.plan));
        assert!(group(&v.artwork(f, Some(&b)), "fft").is_empty(), "strip drawn for a track without audio");
        assert!(group(&v.artwork(f, None), "ruler").len() > 0, "the rest of the band is still drawn");
    }

    #[test]
    fn switching_a_piece_off_removes_only_that_piece() {
        let mut v = video();
        let f = frame_after_drop(&v);
        let b = bands(v.plan.dir.chrome.fft_bands);

        v.plan.dir.chrome.ruler = false;
        let s = v.artwork(f, Some(&b));
        assert!(group(&s, "ruler").is_empty() && !group(&s, "wave").is_empty(), "ruler flag");
        v.plan.dir.chrome.ruler = true;

        v.plan.dir.chrome.waveform = false;
        let s = v.artwork(f, Some(&b));
        assert!(group(&s, "wave").is_empty() && !group(&s, "ruler").is_empty(), "waveform flag");
        v.plan.dir.chrome.waveform = true;

        v.plan.dir.chrome.fft = false;
        let s = v.artwork(f, Some(&b));
        assert!(group(&s, "fft").is_empty() && !group(&s, "wave").is_empty(), "fft flag");
        v.plan.dir.chrome.fft = true;

        v.plan.dir.chrome.meter = false;
        let s = v.artwork(f, None);
        assert!(group(&s, "meter").is_empty() && !group(&s, "status").is_empty(), "meter flag");
        v.plan.dir.chrome.meter = true;

        v.plan.dir.chrome.status = false;
        let s = v.artwork(f, None);
        assert!(group(&s, "status").is_empty() && s.contains("edge "), "status flag");
        v.plan.dir.chrome.status = true;
    }

    #[test]
    fn with_every_piece_off_the_band_is_gone() {
        let mut v = video();
        v.plan.dir.chrome.ruler = false;
        v.plan.dir.chrome.waveform = false;
        v.plan.dir.chrome.fft = false;
        let s = v.artwork(frame_after_drop(&v), None);
        assert!(!s.contains("id='band'"), "band drawn with every part off");
    }
}
