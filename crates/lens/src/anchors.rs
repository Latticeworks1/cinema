//! Recognition squares on the motifs found in the collage images (see direct::anchors), drawn
//! sharp over the filmed screen in the same hand as the note tracker: corner brackets, a small
//! label out of the way under the square, and thin links between neighbouring squares with a dot
//! running along each. One square is locked at a time, chosen by the register of the leading
//! stem's newest note, and a link runs to it from the tracker's own point.

use direct::{FrameState, Plan, register};
use fframes::Svgr;

const RW: f32 = super::WIDTH as f32;
const RH: f32 = super::HEIGHT as f32;

struct Screen {
    label: String,
    c: (f32, f32),
    half: (f32, f32),
    alpha: f32,
}

pub fn marks<'a>(plan: &'a Plan, s: &FrameState, from: Option<(f32, f32)>) -> Vec<Svgr<'a>> {
    let st = &plan.dir.anchors;
    if !st.on || plan.anchors.images.is_empty() {
        return vec![];
    }
    let focal = plan.dir.camera.focal;
    let mut on: Vec<(f32, Screen)> = vec![];
    for m in plan.anchor_marks(s) {
        let Some(c) = direct::project(&s.camera, m.center, RW, RH, focal) else { continue };
        // pixels per world unit at this point, from a step along each axis
        let (Some(px), Some(py)) =
            (direct::project(&s.camera, (m.center.0 + 1.0, m.center.1), RW, RH, focal), direct::project(&s.camera, (m.center.0, m.center.1 + 1.0), RW, RH, focal))
        else {
            continue;
        };
        let (kx, ky) = ((px.0 - c.0).hypot(px.1 - c.1), (py.0 - c.0).hypot(py.1 - c.1));
        let half = (m.half.0 * kx, m.half.1 * ky);
        if 2.0 * half.0.max(half.1) < st.min_px || c.0 < -half.0 || c.0 > RW + half.0 || c.1 < -half.1 || c.1 > RH + half.1 {
            continue;
        }
        on.push((m.score * m.alpha, Screen { label: m.label, c, half, alpha: m.alpha.sqrt() }));
    }
    on.sort_by(|a, b| b.0.total_cmp(&a.0));
    on.truncate(st.cap);
    if on.is_empty() {
        return vec![];
    }
    let marks: Vec<Screen> = on.into_iter().map(|(_, m)| m).collect();

    // The locked square: the leading stem's newest note, by pitch register, picks one from left to right.
    let scene = &plan.scenes[s.scene];
    let tk = &plan.dir.tracker;
    let lock = plan.track().stem(&scene.dominant).and_then(|stem| {
        let last = stem.last_onset_at(s.t)?;
        let n = stem.notes[last];
        if s.t - n.onset >= tk.hold_s {
            return None;
        }
        let mut order: Vec<usize> = (0..marks.len()).collect();
        order.sort_by(|&a, &b| marks[a].c.0.total_cmp(&marks[b].c.0));
        Some(order[((register(stem, n.pitch) * order.len() as f32) as usize).min(order.len() - 1)])
    });

    let ink = plan.dir.ink.lens_ink.as_str();
    let hot = plan.dir.ink.lens_lock.as_str();
    let font = plan.dir.font.clone();
    let mut items: Vec<Svgr> = vec![];

    // Links: each square to its nearest neighbours, once per pair.
    let mut pairs: Vec<(usize, usize)> = vec![];
    for i in 0..marks.len() {
        let mut near: Vec<usize> = (0..marks.len()).filter(|&j| j != i).collect();
        near.sort_by(|&a, &b| dist(&marks[i], &marks[a]).total_cmp(&dist(&marks[i], &marks[b])));
        for &j in near.iter().take(st.links) {
            let pair = (i.min(j), i.max(j));
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }
    for (k, &(i, j)) in pairs.iter().enumerate() {
        let (a, b) = (&marks[i], &marks[j]);
        let o = st.link_opacity * a.alpha.min(b.alpha);
        items.push(fframes::svgr!(<line x1={a.c.0} y1={a.c.1} x2={b.c.0} y2={b.c.1} stroke={ink} stroke-width="1.1" opacity={o} />));
        let p = (s.index as f32 * 0.016 + k as f32 * 0.37).fract();
        items.push(fframes::svgr!(<circle cx={a.c.0 + (b.c.0 - a.c.0) * p} cy={a.c.1 + (b.c.1 - a.c.1) * p} r="2.4" fill={ink} opacity={o * 2.0} />));
    }
    if let (Some(f), Some(l)) = (from, lock) {
        let m = &marks[l];
        items.push(fframes::svgr!(<line x1={f.0} y1={f.1} x2={m.c.0} y2={m.c.1} stroke={hot} stroke-width="1.4" opacity="0.8" />));
    }

    for (i, m) in marks.iter().enumerate() {
        let locked = lock == Some(i);
        let color = if locked { hot } else { ink };
        let (hw, hh) = (m.half.0 + if locked { 5.0 } else { 0.0 }, m.half.1 + if locked { 5.0 } else { 0.0 });
        let (x0, y0, x1, y1) = (m.c.0 - hw, m.c.1 - hh, m.c.0 + hw, m.c.1 + hh);
        let arm = (hw.min(hh) * 2.0 * st.arm).clamp(st.arm_min, st.arm_max).min(hw).min(hh);
        let corner = |x: f32, y: f32, sx: f32, sy: f32| format!("M {:.1} {:.1} L {x:.1} {y:.1} L {:.1} {:.1}", x + sx * arm, y, x, y + sy * arm);
        let d = [corner(x0, y0, 1.0, 1.0), corner(x1, y0, -1.0, 1.0), corner(x1, y1, -1.0, -1.0), corner(x0, y1, 1.0, -1.0)].join(" ");
        let w = if locked { "2.0" } else { "1.3" };
        items.push(fframes::svgr!(<path d={d} fill="none" stroke={color} stroke-width={w} opacity={st.opacity * m.alpha} />));
        if 2.0 * hw.max(hh) >= st.label_min_px {
            let text = if locked { format!("LOCK {}", m.label) } else { m.label.clone() };
            items.push(fframes::svgr!(<text x={x0} y={y1 + st.label_px + 3.0} font-family={font.clone()} font-size={st.label_px} fill={color} opacity={0.7 * m.alpha}>{text}</text>));
        }
    }
    items
}

fn dist(a: &Screen, b: &Screen) -> f32 {
    (a.c.0 - b.c.0).hypot(a.c.1 - b.c.1)
}
