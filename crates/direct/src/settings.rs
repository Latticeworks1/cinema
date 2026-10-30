//! Authored numbers that shape the stage, the camera's limits, the boundaries between scenes and
//! the marks drawn in the flat pass. They are the groups of `vocab/direction.json`.

use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

/// How the camera's point of attention follows the leading stem's recent notes.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attention {
    /// How far ahead of a note's onset the camera starts to turn towards it, seconds.
    pub lead_s: f32,
    /// How long after its onset a note keeps pulling the camera, seconds.
    pub fall_s: f32,
    /// How many of the latest notes are blended.
    pub notes: usize,
}

/// The area of the 1920x1080 screen the panels share, and how a layout rule divides it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub gap: f32,
    /// How much taller a lane of the leading stem is than the others.
    pub lane_emphasis: f32,
    /// Share of the stage width the leading stem takes in the hero layout.
    pub hero_share: f32,
    pub grid_cols: usize,
    /// Margin inside a panel before the first note position.
    pub pad: f32,
}

impl Stage {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraLimits {
    /// Share of the half-view kept inside the stage when the camera is tilted.
    pub stage_slack: f32,
    pub max_zoom: f32,
    /// Margins, as shares of the frame, inside which the subject of attention must stay.
    pub safe_x: f32,
    pub safe_y: f32,
    /// Focal length shared by the CPU projection (`camera::project`) and the lens shader's own
    /// projection (`uFocal` in lens.sksl); the two must always read the same resolved value.
    pub focal: f32,
}

/// Priorities of the boundary kinds; the thinning keeps higher ones.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Priority {
    pub start: u8,
    pub impact: u8,
    pub drop: u8,
    pub phrase: u8,
    pub enter: u8,
    pub exit: u8,
}

/// Sizes and levels of the marks the flat pass draws, in logical pixels and 0..1.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Marks {
    pub note_h: f32,
    pub note_min_len: f32,
    /// Brightness of notes that have not reached the playhead yet.
    pub ahead: f32,
    /// Brightness of the beat grid.
    pub grid: f32,
    pub ring_stagger_s: f32,
}

/// The chrome band under the stage: the time ruler, the waveform, the spectrum, the loudness
/// meter and the status line. Each part can be switched off; the band geometry is in screen
/// pixels of the 1920x1080 frame.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Chrome {
    pub ruler: bool,
    pub waveform: bool,
    pub fft: bool,
    pub meter: bool,
    pub status: bool,
    /// Top edge of the band.
    pub band_y: f32,
    pub band_h: f32,
    /// Width of the spectrum panel at the right of the band.
    pub fft_w: f32,
    /// Number of log-spaced spectrum bars.
    pub fft_bands: usize,
    pub fft_lo_hz: f32,
    pub fft_hi_hz: f32,
    /// Level mapped to an empty bar and to a full bar, dBFS.
    pub fft_db_floor: f32,
    pub fft_db_ceil: f32,
    /// Seconds across the band's ruler and waveform, as a multiple of a panel's span.
    pub band_span_panels: f32,
    /// Waveform columns closer together than this many logical pixels are merged.
    pub merge_px: f32,
    /// Loudness mapped to an empty and a full level meter, dBFS.
    pub meter_floor_db: f32,
    pub meter_ceil_db: f32,
    /// Row of the beat/bar ruler, below the top of the band.
    pub ruler_row: f32,
    /// Tick height of a beat and of a bar on the ruler.
    pub tick_beat: f32,
    pub tick_bar: f32,
}

/// Position and size of the level meter composited sharp over the lens pass (see
/// `lens::overlay::hud`). HUD layout, not tracker behaviour, so it is its own group rather than
/// folded into `Tracker`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Colors of the flat pass's own marks and of the lens overlay composited sharp over the shaded
/// image. Kept independently configurable even where today's values happen to agree: the two
/// layers play different rendering roles (`flat_*` participates in the filmed SVG and goes
/// through the lens shader's palette/LED model; `lens_*` is composited afterward for legibility,
/// see the comment at the end of `flat::chrome`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ink {
    pub flat_lit: String,
    pub flat_dim: String,
    pub flat_rule: String,
    pub flat_label: String,
    pub flat_meter_fill: String,
    pub flat_lock: String,
    pub lens_ink: String,
    pub lens_lock: String,
    pub lens_hud_text: String,
    pub lens_hud_secondary: String,
    pub lens_hud_highlight: String,
    pub lens_hud_muted: String,
    pub lens_rule: String,
    pub lens_label: String,
}

/// The camera's own tracker drawn over the filmed screen.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tracker {
    /// Notes joined by links behind the newest one.
    pub trail: usize,
    pub arm: f32,
    pub grow: f32,
    /// Seconds a box on a played note stays on screen.
    pub hold_s: f32,
    /// Most boxes drawn at once.
    pub box_cap: usize,
    pub box_w: f32,
    pub box_h: f32,
}

/// Camera shots inside a scene: every `beats` beats after the drop the camera whips to another
/// panel and pushes in, with a tilt flick on the move.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shots {
    pub beats: f32,
    /// Share of the shot camera blended over the scene's behaviour, 0 turns shots off.
    pub mix: f32,
    pub whip_s: f32,
    pub zoom: f32,
    /// Extra push over a shot, as a share of its zoom.
    pub push: f32,
    pub tilt: f32,
    pub blur: f32,
    /// Chance a shot frames the whole stage instead of a panel.
    pub wide: f32,
}

/// Lyric shots: each line of a cue's text becomes its own shot, with the line as the subject.
/// `size` 0 turns the layer off and the scene's own camera is used.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Lyric {
    /// Font size of the line in logical pixels; its glyph advance is 0.6 of it.
    pub size: f32,
    /// Baseline of the line in logical pixels.
    pub y: f32,
    /// Share of the line already typed when its shot opens.
    pub pre: f32,
    /// Share of the shot by which the line is fully typed, and the longest that may take.
    pub type_frac: f32,
    pub type_cap_s: f32,
    /// Seconds from the last glyph to the lock on the newest word.
    pub lock_delay_s: f32,
    /// Share of the frame width a line fills when the whole line is framed.
    pub fit_w: f32,
    /// Chance a shot is close on the newest word instead of framing the line, and its extra zoom.
    pub close_p: f32,
    pub close_zoom: f32,
    /// Extra push over a shot, as a share of its zoom.
    pub push: f32,
    pub tilt: f32,
    pub roll: f32,
    /// Seconds of the whip into a shot.
    pub whip_s: f32,
    pub whip_blur: f32,
    /// Brightness of the note panels behind a line, and the opacity of the black under it.
    pub dim: f32,
    pub backing: f32,
    /// Typed words boxed and linked behind the newest one.
    pub trail: usize,
    /// Shortest a line's shot may be, seconds.
    pub min_s: f32,
    /// Seconds from the lock to the line's last glyph leaving (0 keeps it until the next shot).
    pub hold_s: f32,
    /// Glyph advance of the monospace font, as a share of its size.
    pub advance: f32,
}

impl Default for Lyric {
    fn default() -> Self {
        Lyric {
            size: 0.0,
            y: 470.0,
            pre: 0.35,
            type_frac: 0.5,
            type_cap_s: 1.5,
            lock_delay_s: 0.12,
            fit_w: 0.8,
            close_p: 0.45,
            close_zoom: 1.7,
            push: 0.2,
            tilt: 0.2,
            roll: 0.06,
            whip_s: 0.1,
            whip_blur: 0.65,
            dim: 0.3,
            backing: 0.55,
            trail: 3,
            min_s: 0.8,
            hold_s: 0.0,
            advance: 0.6,
        }
    }
}

/// Production-directed motion layered onto the selected camera behavior.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub intro: IntroMotion,
    pub phrase_stutter: PhraseStutter,
    pub ending: EndingMotion,
    pub shot_transient: ShotTransient,
    pub event_accents: EventAccents,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntroMotion {
    pub enabled: bool,
    pub roll: f32,
    pub tilt_y: f32,
    pub tilt_x: f32,
    pub zoom_start: f32,
    pub zoom_rise: f32,
    pub riser_bars: f32,
    pub riser_zoom: f32,
    pub riser_blur: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhraseStutter {
    pub enabled: bool,
    pub duration_s: f32,
    pub hz_x: f32,
    pub hz_y: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub zoom: f32,
    pub roll: f32,
    pub tear: f32,
    pub punch: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndingMotion {
    pub enabled: bool,
    pub lead_bars: f32,
    pub zoom_mix: f32,
    pub aim_mix: f32,
    pub tilt_decay: f32,
    pub roll_decay: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShotTransient {
    pub enabled: bool,
    pub duration_s: f32,
    pub strength: f32,
    pub tear_gain: f32,
    pub punch_gain: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventAccents {
    pub impact_enabled: bool,
    pub impact_decay_s: f32,
    pub lock_enabled: bool,
    pub lock_tear: f32,
    pub lock_aberration: f32,
}

impl Motion {
    pub fn valid(&self) -> bool {
        let i = self.intro;
        let s = self.phrase_stutter;
        let e = self.ending;
        let k = self.shot_transient;
        let a = self.event_accents;
        let nonnegative = [i.roll, i.tilt_y, i.tilt_x, i.zoom_rise, i.riser_zoom,
            i.riser_blur, s.hz_x, s.hz_y, s.aim_x, s.aim_y, s.zoom, s.roll,
            s.tear, s.punch, e.lead_bars, k.strength, k.tear_gain, k.punch_gain,
            a.lock_tear, a.lock_aberration];
        nonnegative.iter().all(|v| v.is_finite() && *v >= 0.0)
            && [i.zoom_start, i.riser_bars, s.duration_s, k.duration_s, a.impact_decay_s]
                .iter().all(|v| v.is_finite() && *v > 0.0)
            && [e.zoom_mix, e.aim_mix, e.tilt_decay, e.roll_decay]
                .iter().all(|v| (0.0..=1.0).contains(v))
    }
}
