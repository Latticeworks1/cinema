//! Authored numbers that shape the stage, the camera's limits, the boundaries between scenes and
//! the marks drawn in the flat pass. They are the groups of `vocab/direction.json`.

use crate::geometry::Rect;
use serde::Deserialize;

/// How the camera's point of attention follows the leading stem's recent notes.
#[derive(Clone, Copy, Debug, Deserialize)]
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
#[derive(Clone, Copy, Debug, Deserialize)]
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

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraLimits {
    /// Share of the half-view kept inside the stage when the camera is tilted.
    pub stage_slack: f32,
    pub max_zoom: f32,
    /// Margins, as shares of the frame, inside which the subject of attention must stay.
    pub safe_x: f32,
    pub safe_y: f32,
}

/// Priorities of the boundary kinds; the thinning keeps higher ones.
#[derive(Clone, Copy, Debug, Deserialize)]
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
#[derive(Clone, Copy, Debug, Deserialize)]
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
#[derive(Clone, Copy, Debug, Deserialize)]
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
}

/// The camera's own tracker drawn over the filmed screen.
#[derive(Clone, Copy, Debug, Deserialize)]
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
#[derive(Clone, Copy, Debug, Deserialize)]
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
