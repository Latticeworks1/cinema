//! The chrome of the flat pass: a context band under the stage (time ruler, master waveform,
//! live spectrum), a header with the loudness meter, and a caption with the status line.
//!
//! The band shows a window of the track centred on the playhead, so the ruler and the waveform
//! scroll under a fixed playhead the same way the stem panels do. Every part is switched on or
//! off by the `chrome` section of direction.json. White and grey on black only.

use crate::draw::gray;
use direct::{FrameState, Plan};
use fframes::{FFramesContext, Frame, SampleSize, VisualizeFrameInput, WindowFunction};
use std::fmt::Write;
use track::Track;

/// The soundtrack as the lens pass names it; the packet copies it into the pass directory.
pub const MUSIC: &str = "music.wav";

/// The band's time window spans this many panel spans, so it shows more of the track than a
/// single panel does.
/// The FFT is taken over this many samples, from the start of each frame.
const FFT_SIZE: usize = 1024;
/// Smoothing of the spectrum across neighbouring frames, in frames on each side.
const FFT_SMOOTH: usize = 1;
/// Added to a band's magnitude before the logarithm, so silence maps to a finite level.
const FFT_EPS: f32 = 1e-6;
/// Columns of the waveform are merged into groups once they are thinner than this many pixels.
/// Display range of the loudness meter in dBFS.

/// Band rows, in logical pixels below the top of the band.
const RULER_ROW: f32 = 22.0;
const TICK_BEAT: f32 = 8.0;
const TICK_BAR: f32 = 16.0;

const LIT: &str = "#e6e6e6";
const DIM: &str = "#666666";
const RULE: &str = "#333333";
const LABEL: &str = "#9a9a9a";
const METER_FILL: &str = "#9a9a9a";

/// The time window of the band, mapped to logical x.
struct Window {
    /// Left and right edge of the scrolling area.
    x0: f32,
    x1: f32,
    /// Seconds at the left edge.
    t_lo: f32,
    /// Seconds across the scrolling area.
    span: f32,
    /// Logical pixels per second.
    px_per_s: f32,
}

impl Window {
    fn new(plan: &Plan, st: &FrameState, scroll_w: f32) -> Window {
        let stage = &plan.dir.stage;
        let span = plan.dir.panel_span_s * plan.dir.chrome.band_span_panels;
        let now_frac = plan.timeline.now_frac;
        Window { x0: stage.x, x1: stage.x + scroll_w, t_lo: st.t - now_frac * span, span, px_per_s: scroll_w / span }
    }

    fn x_of(&self, t: f32) -> f32 {
        self.x0 + (t - self.t_lo) * self.px_per_s
    }

    fn t_hi(&self) -> f32 {
        self.t_lo + self.span
    }
}

/// Whether the FFT strip is drawn for this frame: the flag is on and the track has audio.
pub fn fft_shown(plan: &Plan) -> bool {
    plan.dir.chrome.fft && plan.track().has_audio()
}

/// Band levels 0..1 of the spectrum at `frame`, or None when the strip is not drawn. A track
/// with audio whose soundtrack is missing from the media is an error, not a silent blank strip.
pub fn spectrum(plan: &Plan, frame: &Frame, ctx: &FFramesContext) -> Option<Vec<f32>> {
    if !fft_shown(plan) {
        return None;
    }
    let audio = ctx
        .get_audio(MUSIC)
        .unwrap_or_else(|| panic!("{MUSIC} is not in the flat media: run `packet prepare` for this packet"));
    let rate = audio.sample_rate();
    assert!(rate > 0, "{MUSIC} decoded with no sample rate");
    let mags = frame.visualize_audio_frame(VisualizeFrameInput {
        audio,
        sample_size: SampleSize::S1024,
        smooth_level: FFT_SMOOTH,
        window: Some(WindowFunction::Hann),
    });
    let c = &plan.dir.chrome;
    let bin_hz = rate as f32 / FFT_SIZE as f32;
    let edge = |i: usize| c.fft_lo_hz * (c.fft_hi_hz / c.fft_lo_hz).powf(i as f32 / c.fft_bands as f32);
    let levels = (0..c.fft_bands)
        .map(|b| {
            let lo = ((edge(b) / bin_hz).floor() as usize).max(1);
            let hi = ((edge(b + 1) / bin_hz).ceil() as usize).max(lo + 1).min(FFT_SIZE / 2);
            let m = mags
                .get(lo..hi.min(mags.len()))
                .map(|s| s.iter().cloned().fold(0.0, f32::max))
                .unwrap_or(0.0);
            let v = (20.0 * (m + FFT_EPS).log10() - c.fft_db_floor) / (c.fft_db_ceil - c.fft_db_floor);
            if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
        })
        .collect();
    Some(levels)
}

/// The context band: ruler, waveform, spectrum and the playhead, between the stage and the caption.
pub fn band(s: &mut String, plan: &Plan, st: &FrameState, bands: Option<&[f32]>) {
    let c = &plan.dir.chrome;
    if !(c.ruler || c.waveform || c.fft) {
        return;
    }
    let tr = plan.track();
    let stage = &plan.dir.stage;
    let fft = bands.filter(|_| fft_shown(plan));
    let fft_w = if fft.is_some() { c.fft_w } else { 0.0 };
    let win = Window::new(plan, st, stage.w - fft_w);
    let (top, h) = (c.band_y, c.band_h);
    let bottom = top + h;
    let now_x = win.x0 + win.px_per_s * (st.t - win.t_lo);

    s.push_str("<g id='band'>");
    if c.waveform {
        waveform(s, tr, st, &win, top + RULER_ROW + 4.0, bottom, c.merge_px);
    }
    if c.ruler {
        ruler(s, plan, &win, top);
    }
    if let Some(levels) = fft {
        spectrum_strip(s, levels, stage.x + stage.w - fft_w, fft_w, top, bottom);
    }
    write!(
        s,
        "<line x1='{now_x:.1}' y1='{top:.1}' x2='{now_x:.1}' y2='{bottom:.1}' stroke='{}' stroke-width='1.5'/>",
        gray(0.85)
    )
    .unwrap();
    s.push_str("</g>");
}

/// Beat ticks from the drop, a longer tick and a label on every bar.
fn ruler(s: &mut String, plan: &Plan, win: &Window, top: f32) {
    let tr = plan.track();
    let beat = 60.0 / tr.bpm;
    let bpb = beats_per_bar(tr);
    let base = top + RULER_ROW;
    write!(
        s,
        "<g id='ruler'><line x1='{:.1}' y1='{base:.1}' x2='{:.1}' y2='{base:.1}' stroke='{RULE}' stroke-width='1.5'/>",
        win.x0, win.x1
    )
    .unwrap();
    let k_lo = ((win.t_lo - tr.drop_t) / beat).floor() as i64;
    let k_hi = ((win.t_hi() - tr.drop_t) / beat).ceil() as i64;
    for k in k_lo..=k_hi {
        let t = tr.drop_t + k as f32 * beat;
        if t < 0.0 || t > tr.duration_s {
            continue;
        }
        let x = win.x_of(t);
        if x < win.x0 || x > win.x1 {
            continue;
        }
        let bar = k.rem_euclid(bpb as i64) == 0;
        let len = if bar { TICK_BAR } else { TICK_BEAT };
        write!(
            s,
            "<line x1='{x:.1}' y1='{:.1}' x2='{x:.1}' y2='{base:.1}' stroke='{}' stroke-width='1.5'/>",
            base - len,
            gray(if bar { 0.55 } else { 0.36 })
        )
        .unwrap();
        let bar_no = k.div_euclid(bpb as i64) + 1;
        if bar && bar_no >= 1 {
            write!(
                s,
                "<text x='{:.1}' y='{:.1}' font-size='15' fill='{LABEL}'>bar {bar_no}   {t:.1} s</text>",
                x + 6.0,
                base - 4.0
            )
            .unwrap();
        }
    }
    s.push_str("</g>");
}

/// Master waveform. Columns closer together than `merge_px` are merged into groups, each drawn at
/// its peak, so the shape stays solid at every zoom. Columns before the playhead are lit.
fn waveform(s: &mut String, tr: &Track, st: &FrameState, win: &Window, y0: f32, y1: f32, merge_px: f32) {
    let cps = tr.waveform_cols_per_s;
    let n = tr.waveform.len();
    if n == 0 || !(cps > 0.0) {
        return;
    }
    let mid = 0.5 * (y0 + y1);
    let half = 0.5 * (y1 - y0) - 4.0;
    let col_px = win.px_per_s / cps;
    let stride = ((merge_px / col_px).ceil().max(1.0)) as usize;
    let first = ((win.t_lo * cps).floor().max(0.0) as usize) / stride * stride;
    let last = ((win.t_hi() * cps).ceil().max(0.0) as usize).min(n);
    let w = col_px * stride as f32;
    write!(s, "<g id='wave'><rect x='{:.1}' y='{mid:.1}' width='{:.1}' height='1' fill='{RULE}'/>", win.x0, win.x1 - win.x0).unwrap();
    for g0 in (first..last).step_by(stride) {
        let t = g0 as f32 / cps;
        let x = win.x_of(t);
        if x + w < win.x0 || x > win.x1 {
            continue;
        }
        let peak = tr.waveform[g0..(g0 + stride).min(n)].iter().cloned().fold(0.0f32, f32::max);
        let h = 0.5 + peak.clamp(0.0, 1.0) * half;
        let fill = if t <= st.t { LIT } else { DIM };
        write!(
            s,
            "<rect x='{x:.1}' y='{:.1}' width='{:.1}' height='{:.1}' fill='{fill}'/>",
            mid - h,
            (w - 0.6).max(0.8),
            2.0 * h
        )
        .unwrap();
    }
    s.push_str("</g>");
}

/// The spectrum, one bar per band stacked from the bottom of the band, growing to the right.
fn spectrum_strip(s: &mut String, levels: &[f32], x0: f32, w: f32, top: f32, bottom: f32) {
    let n = levels.len().max(1);
    let step = (bottom - top) / n as f32;
    write!(s, "<g id='fft'>").unwrap();
    for (i, &v) in levels.iter().enumerate() {
        let y = bottom - (i + 1) as f32 * step + 1.5;
        write!(
            s,
            "<rect x='{x0:.1}' y='{y:.1}' width='{:.1}' height='{:.1}' fill='{METER_FILL}'/>",
            (v * w).max(0.5),
            (step - 3.0).max(0.5)
        )
        .unwrap();
    }
    s.push_str("</g>");
}

/// Bars per bar of the track, from its measured bar length and tempo.
pub fn beats_per_bar(tr: &Track) -> usize {
    (tr.bar_s * tr.bpm / 60.0).round().max(1.0) as usize
}

// The scene counter, time, BPM, bar, loudness meter and caption line used to be drawn here and
// go through the LED-degraded flat pass with everything else. They are information that must
// stay legible, the same category as the camera's own tracker labels, so they are now composited
// sharp over the shaded image instead: see `lens::overlay::hud`, next to `overlay::tracker`.
