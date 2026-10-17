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

/// Header meter position, in logical pixels.
const METER_X: f32 = 1290.0;
const METER_Y: f32 = 70.0;
const METER_W: f32 = 300.0;
const METER_H: f32 = 14.0;

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

/// Loudness at `frame` in dBFS, held inside the meter range. A missing or non-finite value reads
/// as the floor of the range.
pub fn level_db(tr: &Track, frame: usize, c: &direct::Chrome) -> f32 {
    match tr.level_db.get(frame) {
        Some(&db) if db.is_finite() => db.clamp(c.meter_floor_db, c.meter_ceil_db),
        _ => c.meter_floor_db,
    }
}

/// Loudness meter in the header, filled from the floor to the ceiling of its range.
pub fn meter(s: &mut String, plan: &Plan, st: &FrameState) {
    if !plan.dir.chrome.meter {
        return;
    }
    let c = &plan.dir.chrome;
    let db = level_db(plan.track(), st.index, c);
    let frac = (db - c.meter_floor_db) / (c.meter_ceil_db - c.meter_floor_db);
    write!(
        s,
        "<g id='meter'><rect x='{METER_X}' y='{METER_Y}' width='{METER_W}' height='{METER_H}' fill='none' stroke='{RULE}' stroke-width='1.5'/><rect x='{METER_X}' y='{METER_Y}' width='{:.1}' height='{METER_H}' fill='{METER_FILL}'/></g>",
        (frac * METER_W).max(0.5)
    )
    .unwrap();
}

/// Header: where we are in the scene list and in the track, plus the live streaming text tracking overlay.
pub fn header(s: &mut String, plan: &Plan, st: &FrameState) {
    let sc = &plan.scenes[st.scene];
    let tr = plan.track();
    let bar = ((st.t - tr.drop_t) / tr.bar_s).floor() as i32 + 1;
    write!(
        s,
        "<text x='90' y='84' font-size='20' letter-spacing='4' fill='{}'>{:02}.{:02} {:05.2} s</text>",
        gray(0.9),
        sc.index + 1,
        plan.scenes.len(),
        st.t
    )
    .unwrap();

    // Streaming text tracking terminal overlay (from ff-tracking & ff-unmute)
    if let Some(ref txt) = sc.text {
        let elapsed = (st.t - sc.t0).max(0.0);
        let type_span = (sc.duration() * 0.75).max(0.6);
        let n_chars = txt.chars().count();
        let count = ((elapsed / type_span) * n_chars as f32).ceil().min(n_chars as f32) as usize;
        let typed: String = txt.chars().take(count).collect();
        let prompt_x = 340.0;
        let char_w = 10.5;
        let cur_x = prompt_x + 24.0 + count as f32 * char_w;
        let typing = count < n_chars;
        let cursor_opacity = if typing || (st.index / 12) % 2 == 0 { "0.9" } else { "0.0" };

        write!(
            s,
            "<g id='text-stream'><text x='{prompt_x}' y='84' font-size='16' letter-spacing='2' fill='#ff0000'>› </text><text x='{:.1}' y='84' font-size='16' letter-spacing='1' fill='{}'>{}</text><rect x='{:.1}' y='68' width='7' height='18' fill='#dadada' opacity='{cursor_opacity}'/></g>",
            prompt_x + 22.0,
            gray(0.92),
            typed,
            cur_x
        )
        .unwrap();

        // Hatched tracker box over the latest active word
        if count > 0 {
            let last_space = typed.rfind(' ').map_or(0, |idx| idx + 1);
            let word_len = count - last_space;
            if word_len > 0 {
                let wx = prompt_x + 22.0 + last_space as f32 * char_w - 3.0;
                let ww = word_len as f32 * char_w + 6.0;
                write!(
                    s,
                    "<rect x='{:.1}' y='66' width='{:.1}' height='22' fill='none' stroke='{}' stroke-width='1.2' stroke-dasharray='3,3'/>",
                    wx, ww, if st.look.lock { "#ff0000" } else { "#ffffff" }
                ).unwrap();
            }
        }
    }

    write!(
        s,
        "<text x='1830' y='84' font-size='16' text-anchor='end' fill='{}'>{:.1} BPM · bar {}</text>",
        gray(0.6),
        tr.bpm,
        bar.max(0)
    )
    .unwrap();
    meter(s, plan, st);
}

/// Caption: the scene's vocabulary on the left. The right side carries the status line when it
/// is on, and the edge and energy axes otherwise.
pub fn caption(s: &mut String, plan: &Plan, st: &FrameState) {
    let sc = &plan.scenes[st.scene];
    let v = &plan.vocab;
    let label = match &sc.text {
        Some(txt) => format!("\"{txt}\"  ·  {} ({})", sc.dominant, v.palettes[sc.palette].name),
        None => format!(
            "{} leads · {} · {} · {} · {}",
            sc.dominant,
            v.layouts[sc.layout].name,
            v.behaviors[sc.behavior].name,
            v.looks[sc.look].name,
            v.palettes[sc.palette].name
        ),
    };
    write!(
        s,
        "<text x='90' y='1008' font-size='14' letter-spacing='2' fill='{}'>{}</text>",
        if sc.text.is_some() { gray(0.85) } else { gray(0.55) },
        label
    )
    .unwrap();
    if plan.dir.chrome.status {
        status(s, plan, st);
    } else {
        write!(
            s,
            "<text x='1830' y='1008' font-size='14' text-anchor='end' fill='{}'>edge {:+.2} · energy {:+.2}</text>",
            gray(0.55),
            sc.axes.edge,
            sc.axes.energy
        )
        .unwrap();
    }
}

/// BPM, bar within the track, beat within the bar, time, level and frame.
fn status(s: &mut String, plan: &Plan, st: &FrameState) {
    let tr = plan.track();
    let beat = 60.0 / tr.bpm;
    let bpb = beats_per_bar(tr) as i64;
    let k = ((st.t - tr.drop_t) / beat).floor() as i64;
    let bar = k.div_euclid(bpb) + 1;
    let beat_in_bar = k.rem_euclid(bpb) + 1;
    let total_bars = (tr.duration_s / tr.bar_s).ceil().max(1.0) as i64;
    let text = format!(
        "{:.1} BPM  ·  bar {}/{}  ·  beat {}  ·  {:06.3} s  ·  {:>6.1} dBFS  ·  frame {}/{}",
        tr.bpm,
        bar.max(1),
        total_bars,
        beat_in_bar,
        st.t,
        level_db(tr, st.index, &plan.dir.chrome),
        st.index + 1,
        plan.frames()
    );
    write!(
        s,
        "<text id='status' x='1830' y='1008' font-size='14' letter-spacing='1' text-anchor='end' fill='{}'>{text}</text>",
        gray(0.55)
    )
    .unwrap();
}
