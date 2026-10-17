use crate::chrome;
use direct::{EmitKind, FrameState, Panel, Plan, Slider, register};
use std::fmt::Write;
use track::{FPS, Stem, note_name};

const SCALE: f32 = 2.0;
const RED: &str = "#ff0000";
const EMIT_STREAM: u64 = 20;

pub(crate) fn gray(v: f32) -> String {
    let c = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{c:02x}{c:02x}{c:02x}")
}

/// One frame as SVG. `bands` is the spectrum of the soundtrack at this frame, when there is one;
/// the spectrum strip is drawn only when it is given and the flags and the track allow it.
pub fn artwork(plan: &Plan, index: usize, bands: Option<&[f32]>) -> String {
    let st = plan.frame(index);
    let mono = &plan.dir.font;
    let mut s = String::with_capacity(48_000);
    write!(
        s,
        "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}'><rect width='100%' height='100%' fill='#000000'/><g transform='scale({SCALE})' font-family='{mono}'>",
        super::WIDTH,
        super::HEIGHT
    )
    .unwrap();
    chrome::header(&mut s, plan, &st);
    chrome::band(&mut s, plan, &st, bands);
    for panel in &st.panels {
        draw_panel(&mut s, plan, &st, panel);
    }
    chrome::caption(&mut s, plan, &st);
    if st.look.lock {
        lock_marker(&mut s, plan, &st);
    }
    s.push_str("</g></svg>");
    s
}

fn draw_panel(s: &mut String, plan: &Plan, st: &FrameState, panel: &Panel) {
    let vocab = &plan.vocab;
    let si = vocab.stem_index(&panel.stem).expect("declared stem");
    let ink = vocab.stems[si].ink;
    let scene = &plan.scenes[st.scene];
    let shape = &vocab.shapes[scene.shape_by_stem[si]];
    let stem = plan.track().stem(&panel.stem).expect("present stem");
    let slider = plan.layouts[st.scene].slider;
    let tl = &plan.timeline;
    let leading = panel.stem == scene.dominant;
    let r = panel.rect;

    write!(
        s,
        "<rect x='{:.1}' y='{:.1}' width='{:.1}' height='{:.1}' fill='none' stroke='{}' stroke-width='1'/>",
        r.x,
        r.y,
        r.w,
        r.h,
        gray(if leading { 0.42 } else { 0.2 })
    )
    .unwrap();
    // Everything inside the panel is clipped to it: long notes run off the edge.
    write!(
        s,
        "<clipPath id='c{si}'><rect x='{:.1}' y='{:.1}' width='{:.1}' height='{:.1}'/></clipPath><g clip-path='url(#c{si})'>",
        r.x, r.y, r.w, r.h
    )
    .unwrap();
    grid(s, plan, st, panel, slider);

    // Notes and their emitters.
    let reach = tl.span_s * 1.2;
    let a = stem.notes.partition_point(|n| n.offset < st.t - reach);
    let b = stem.notes.partition_point(|n| n.onset <= st.t + reach);
    let pulse = 1.0 + st.pulse;
    for (k, n) in stem.notes[a..b].iter().enumerate() {
        let idx = a + k;
        let p = panel.note_pos(slider, tl, st.t, n.onset, register(stem, n.pitch));
        if !r.inset(-0.5 * panel.inner().w).contains(p) {
            continue;
        }
        let ahead = n.onset > st.t;
        let sounding = n.onset <= st.t && st.t < n.offset.max(n.onset + 0.06);
        let v = ink * if ahead { plan.dir.marks.ahead } else if sounding { 1.0 } else { 0.7 };
        let len = (n.duration() / tl.span_s * panel.inner().w).max(plan.dir.marks.note_min_len);
        let (dx, dy) = match slider {
            Slider::Linear => (1.0, 0.0),
            Slider::Ring => {
                let c = panel.inner().center();
                let (rx, ry) = (p.0 - c.0, p.1 - c.1);
                let l = (rx * rx + ry * ry).sqrt().max(1e-3);
                (-ry / l, rx / l)
            }
        };
        let thick = plan.dir.marks.note_h * if sounding { pulse } else { 1.0 };
        let cap = if shape.corner >= 0.6 { "round" } else { "butt" };
        let len = if cap == "round" { (len - thick).max(0.5) } else { len };
        write!(
            s,
            "<line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='{}' stroke-width='{:.1}' stroke-linecap='{cap}'/>",
            p.0,
            p.1,
            p.0 + dx * len,
            p.1 + dy * len,
            gray(v),
            thick
        )
        .unwrap();
        if !ahead {
            emit(s, plan, st, panel, si, stem, idx, p, v);
        }
    }

    s.push_str("</g>");

    // Label: stem, the newest pitch, and for the lead the chord of the bar.
    let mut label = panel.stem.clone();
    if let Some(i) = stem.last_onset_at(st.t) {
        write!(label, " {}", note_name(stem.notes[i].pitch)).unwrap();
    }
    if leading {
        if let Some(c) = plan.track().chord_at(st.t) {
            write!(label, " · {}", c.name).unwrap();
        }
    }
    write!(
        s,
        "<text x='{:.1}' y='{:.1}' font-size='15' fill='{}'>{label}</text>",
        r.x + 8.0,
        r.y + 20.0,
        gray(if leading { 0.95 } else { 0.5 })
    )
    .unwrap();
}

/// Beat and bar guides, and the playhead.
fn grid(s: &mut String, plan: &Plan, st: &FrameState, panel: &Panel, slider: Slider) {
    let tr = plan.track();
    let tl = &plan.timeline;
    let inner = panel.inner();
    let beat = 60.0 / tr.bpm;
    let first = ((st.t - tl.span_s * tl.now_frac - tr.drop_t) / beat).floor() as i32;
    let last = ((st.t + tl.span_s * (1.0 - tl.now_frac) - tr.drop_t) / beat).ceil() as i32;
    match slider {
        Slider::Linear => {
            for k in first..=last {
                let t = tr.drop_t + k as f32 * beat;
                let x = inner.x + inner.w * (tl.now_frac + (t - st.t) / tl.span_s);
                if x < inner.x || x > inner.right() {
                    continue;
                }
                let bar = k.rem_euclid(4) == 0;
                write!(
                    s,
                    "<line x1='{x:.1}' y1='{:.1}' x2='{x:.1}' y2='{:.1}' stroke='{}' stroke-width='1'/>",
                    inner.y,
                    inner.bottom(),
                    gray(if bar { plan.dir.marks.grid * 1.6 } else { plan.dir.marks.grid * 0.7 })
                )
                .unwrap();
            }
            let x = panel.playhead_x(tl);
            write!(
                s,
                "<line x1='{x:.1}' y1='{:.1}' x2='{x:.1}' y2='{:.1}' stroke='{}' stroke-width='1.5'/>",
                inner.y,
                inner.bottom(),
                gray(0.85)
            )
            .unwrap();
        }
        Slider::Ring => {
            let (cx, cy) = inner.center();
            let rmax = 0.5 * inner.w.min(inner.h);
            for f in [0.3, 0.65, 1.0] {
                write!(
                    s,
                    "<circle cx='{cx:.1}' cy='{cy:.1}' r='{:.1}' fill='none' stroke='{}' stroke-width='1'/>",
                    rmax * f,
                    gray(plan.dir.marks.grid)
                )
                .unwrap();
            }
            write!(
                s,
                "<line x1='{cx:.1}' y1='{cy:.1}' x2='{cx:.1}' y2='{:.1}' stroke='{}' stroke-width='1.5'/>",
                cy - rmax,
                gray(0.85)
            )
            .unwrap();
        }
    }
}

/// Particles of one note, as closed-form functions of the note and its age.
#[allow(clippy::too_many_arguments)]
fn emit(s: &mut String, plan: &Plan, st: &FrameState, panel: &Panel, si: usize, stem: &Stem, idx: usize, origin: (f32, f32), v: f32) {
    let shape = &plan.vocab.shapes[plan.scenes[st.scene].shape_by_stem[si]];
    let e = shape.emit;
    let n = &stem.notes[idx];
    let age = st.t - n.onset;
    if age < 0.0 || age > e.life_s * 1.6 {
        return;
    }
    let count = (e.count as f32 * st.emit * (0.6 + 0.4 * register(stem, n.pitch))).round() as u64;
    let seed = plan.dir.seed;
    for k in 0..count {
        let key = (idx as u64) * 64 + k;
        match e.kind {
            EmitKind::Ring => {
                let a = age - k as f32 * plan.dir.marks.ring_stagger_s;
                if !(0.0..e.life_s).contains(&a) {
                    continue;
                }
                let u = a / e.life_s;
                write!(
                    s,
                    "<circle cx='{:.1}' cy='{:.1}' r='{:.1}' fill='none' stroke='{}' stroke-width='{:.1}' opacity='{:.2}'/>",
                    origin.0,
                    origin.1,
                    e.size + e.speed * a * 0.5,
                    gray(v),
                    2.0 * (1.0 - u) + 0.5,
                    (1.0 - u) * (1.0 - u)
                )
                .unwrap();
            }
            EmitKind::Dot | EmitKind::Shard => {
                if age >= e.life_s {
                    continue;
                }
                let u = age / e.life_s;
                let ang = direct::geometry::rnd(seed, EMIT_STREAM + si as u64, key) * std::f32::consts::TAU;
                let speed = e.speed * (0.5 + direct::geometry::rnd(seed, EMIT_STREAM + 10 + si as u64, key));
                let travel = speed * age * (1.0 - 0.5 * u);
                let (px, py) = (origin.0 + ang.cos() * travel, origin.1 + ang.sin() * travel);
                if !panel.rect.contains((px, py)) {
                    continue;
                }
                let alpha = (1.0 - u) * (1.0 - u);
                if e.kind == EmitKind::Dot {
                    write!(
                        s,
                        "<circle cx='{px:.1}' cy='{py:.1}' r='{:.1}' fill='{}' opacity='{alpha:.2}'/>",
                        e.size * (1.0 - 0.6 * u) * 0.5,
                        gray(v)
                    )
                    .unwrap();
                } else {
                    let l = e.size * (1.0 - 0.5 * u);
                    write!(
                        s,
                        "<line x1='{px:.1}' y1='{py:.1}' x2='{:.1}' y2='{:.1}' stroke='{}' stroke-width='2' opacity='{alpha:.2}'/>",
                        px + ang.cos() * l,
                        py + ang.sin() * l,
                        gray(v)
                    )
                    .unwrap();
                }
            }
        }
    }
}

/// A red frame round the newest note of the leading stem, on the frames it lands.
fn lock_marker(s: &mut String, plan: &Plan, st: &FrameState) {
    let lead = &plan.scenes[st.scene].dominant;
    let Some(stem) = plan.track().stem(lead) else { return };
    let probe = (st.index as f32 + 0.5) / FPS as f32;
    let Some(i) = stem.last_onset_at(probe) else { return };
    let n = stem.notes[i];
    let Some(panel) = plan.lead_panel(st.scene) else { return };
    let p = panel.note_pos(plan.layouts[st.scene].slider, &plan.timeline, st.t, n.onset, register(stem, n.pitch));
    let h = plan.dir.marks.note_h * 1.8;
    write!(
        s,
        "<rect x='{:.1}' y='{:.1}' width='{:.1}' height='{:.1}' fill='none' stroke='{RED}' stroke-width='3'/>",
        p.0 - h,
        p.1 - h,
        h * 2.0 + 24.0,
        h * 2.0
    )
    .unwrap();
}
