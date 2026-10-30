//! Lyric shots: every line of a scene's text is its own shot, typed in the monospace font and
//! filmed with the line as the subject.
//!
//! A line opens part-typed, finishes typing by a share of the shot, and the newest word then
//! locks. All of it is a closed form of the time, so any frame can be evaluated alone.

use crate::camera::Camera;
use crate::geometry::{W, ease_out, rnd};
use crate::scenes::{Direction, Scene};
use track::Track;

#[derive(Clone, Debug)]
pub struct LyricLine {
    pub scene: usize,
    pub text: String,
    pub t0: f32,
    pub t1: f32,
    /// Real onset of every word of `text`, in order, when the source cue has one; see
    /// `Cue::word_times`. `None` reveals the line at a uniform typing speed instead.
    pub word_times: Option<Vec<f32>>,
}

/// One line at one frame.
#[derive(Clone, Debug)]
pub struct LyricState {
    pub line: usize,
    pub text: String,
    /// Glyphs drawn so far and in the whole line.
    pub typed: usize,
    pub len: usize,
    pub size: f32,
    /// Resolved `direction.lyric.advance`: glyph advance of the monospace font as a share of `size`.
    pub advance_ratio: f32,
    pub x0: f32,
    pub baseline: f32,
    /// Seconds into the shot, and 0..1 through it.
    pub age: f32,
    pub u: f32,
    /// Columns (start, length) of every word typed so far, the newest last.
    pub words: Vec<(usize, usize)>,
    /// The newest word is locked on this frame.
    pub locked: bool,
    /// 0..1 growth of the box over the newest word once the line is typed.
    pub grow: f32,
}

impl LyricState {
    pub fn advance(&self) -> f32 {
        self.size * self.advance_ratio
    }

    /// Centre of a word in logical pixels.
    pub fn word_center(&self, (start, len): (usize, usize)) -> (f32, f32) {
        (self.x0 + (start as f32 + len as f32 * 0.5) * self.advance(), self.baseline - 0.35 * self.size)
    }

    pub fn newest(&self) -> Option<(usize, usize)> {
        self.words.last().copied()
    }

    pub fn line_center(&self) -> (f32, f32) {
        (self.x0 + self.len as f32 * self.advance() * 0.5, self.baseline - 0.35 * self.size)
    }
}

/// Split a cue's text into lines at " / " and give every line a share of its scene, with the
/// inner boundaries moved onto the beat grid and no shot shorter than `min_s`.
pub fn lines(scenes: &[Scene], track: &Track, d: &Direction) -> Vec<LyricLine> {
    let beat = 60.0 / track.bpm;
    let mut out = vec![];
    for sc in scenes {
        let Some(text) = &sc.text else { continue };
        let parts: Vec<&str> = text.split(" / ").map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }
        let n = parts.len();
        // Slice the scene's flat per-word onsets into one run per subline, in the same reading
        // order `scenes::cue_words` lists them in, before placing the cuts: `Direction::parse`
        // already guarantees the total count matches, so each subline's words consume exactly
        // its share, and the first real word of a subline is the truest cut point there is.
        let mut rest = sc.word_times.as_deref();
        let word_runs: Vec<Option<Vec<f32>>> = parts
            .iter()
            .map(|p| {
                let count = p.split_whitespace().count();
                rest.map(|wt| {
                    let (head, tail) = wt.split_at(count);
                    rest = Some(tail);
                    head.to_vec()
                })
            })
            .collect();
        let mut cuts = vec![sc.t0];
        for k in 1..n {
            let lo = cuts[k - 1] + d.lyric.min_s;
            let hi = sc.t1 - d.lyric.min_s * (n - k) as f32;
            let raw = match word_runs[k].as_ref().and_then(|wt| wt.first()) {
                // A real onset for this subline's first word: no need to guess or snap to a
                // beat, it already is one.
                Some(&onset) => onset,
                None => {
                    let guess = sc.t0 + (sc.t1 - sc.t0) * k as f32 / n as f32;
                    track.drop_t + ((guess - track.drop_t) / beat).round() * beat
                }
            };
            cuts.push(if lo <= hi { raw.clamp(lo, hi) } else { raw });
        }
        cuts.push(sc.t1);
        for (k, (p, word_times)) in parts.iter().zip(word_runs).enumerate() {
            out.push(LyricLine { scene: sc.index, text: (*p).to_string(), t0: cuts[k], t1: cuts[k + 1], word_times });
        }
    }
    out
}

pub fn line_at(lines: &[LyricLine], t: f32) -> Option<usize> {
    let i = lines.partition_point(|l| l.t0 <= t + 1e-4).checked_sub(1)?;
    (t < lines[i].t1).then_some(i)
}

fn words(text: &str, typed: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = None;
    for (i, c) in text.chars().take(typed).enumerate() {
        match (c == ' ', start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, i - s));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, typed - s));
    }
    out
}

/// Seconds into a shot at which its line is fully typed: the real onset of its last word when
/// the line has real timing, or a fixed share of the shot's length otherwise.
fn typed_by(l: &LyricLine, d: &Direction) -> f32 {
    match &l.word_times {
        Some(wt) => wt.last().map_or(0.0, |&onset| onset - l.t0),
        None => ((l.t1 - l.t0) * d.lyric.type_frac).min(d.lyric.type_cap_s),
    }
}

/// Characters revealed by real per-word onsets: a word appears the instant it is actually sung,
/// not at a uniform typing speed. The newest word is therefore the one being sung right now.
fn typed_from_onsets(text: &str, wt: &[f32], t: f32) -> usize {
    let bounds = words(text, text.chars().count());
    let n_done = wt.iter().filter(|&&onset| t >= onset).count().min(bounds.len());
    n_done.checked_sub(1).and_then(|i| bounds.get(i)).map_or(0, |&(s, wl)| s + wl)
}

pub fn state(lines: &[LyricLine], d: &Direction, index: usize) -> Option<LyricState> {
    let ly = &d.lyric;
    if ly.size <= 0.0 {
        return None;
    }
    let t = track::output::seconds(index);
    let i = line_at(lines, t)?;
    let l = &lines[i];
    let len = l.text.chars().count();
    let age = t - l.t0;
    let done = typed_by(l, d).max(1e-3);
    let typed = match &l.word_times {
        Some(wt) => typed_from_onsets(&l.text, wt, t),
        None => ((ly.pre + (1.0 - ly.pre) * (age / done).clamp(0.0, 1.0)) * len as f32).ceil().min(len as f32) as usize,
    };
    let lock_at = track::output::nearest_frame(l.t0 + done + ly.lock_delay_s);
    let locked = index >= lock_at && index < lock_at + d.lock_frames;
    let grow = ((age - done) / (ly.lock_delay_s + 0.2)).clamp(0.0, 1.0);
    let x0 = W * 0.5 - len as f32 * ly.size * ly.advance * 0.5;
    Some(LyricState {
        line: i,
        text: l.text.clone(),
        typed,
        len,
        size: ly.size,
        advance_ratio: ly.advance,
        x0,
        baseline: ly.y,
        age,
        u: (age / (l.t1 - l.t0)).clamp(0.0, 1.0),
        words: words(&l.text, typed),
        locked,
        grow,
    })
}

/// The camera for a lyric shot: it frames the line, or closes on the newest word, tilted
/// differently in every shot and whipped in at the cut.
pub fn camera(ls: &LyricState, d: &Direction) -> Camera {
    let ly = &d.lyric;
    let key = ls.line as u64;
    let width = ls.len as f32 * ls.advance();
    let frame_zoom = (W * ly.fit_w / width).clamp(1.0, d.camera.max_zoom);
    let close = rnd(d.seed, 8100, key) < ly.close_p;
    let word = ls.newest().map_or(ls.line_center(), |w| ls.word_center(w));
    let line = ls.line_center();
    let w = (ls.age / ly.whip_s).clamp(0.0, 1.0);
    let e = ease_out(w);
    let hump = (w * std::f32::consts::PI).sin();
    let sign = |stream: u64| if rnd(d.seed, stream, key) < 0.5 { -1.0 } else { 1.0 };
    let mag = |stream: u64| 0.55 + 0.45 * rnd(d.seed, stream, key);
    let zoom = frame_zoom * if close { ly.close_zoom } else { 1.0 } * (1.0 + ly.push * ls.u) * (0.75 + 0.25 * e);
    let aim = if close { word } else { (line.0 + (word.0 - line.0) * 0.25, line.1) };
    Camera {
        aim,
        zoom,
        tilt_x: ly.tilt * sign(8101) * mag(8104) + 0.3 * sign(8101) * hump,
        tilt_y: ly.tilt * 0.8 * sign(8102) * mag(8105),
        roll: ly.roll * sign(8103) * mag(8106) + 0.5 * ly.roll * sign(8103) * hump,
        blur: ly.whip_blur * hump,
    }
}
