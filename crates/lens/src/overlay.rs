//! The camera's own tracker, drawn sharp over the filmed screen: brackets and a label on the
//! newest note of the leading stem, and links through its last few notes.

use direct::{FrameState, Plan, project, register};
use fframes::Svgr;
use track::note_name;

const RW: f32 = super::WIDTH as f32;
const RH: f32 = super::HEIGHT as f32;
const INK: &str = "#ffffff";
const LOCK_RED: &str = "#ff3b30";

fn hz(pitch: u8) -> f32 {
    440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0)
}

pub fn tracker<'a>(plan: &Plan, s: &FrameState) -> Svgr<'a> {
    let scene = &plan.scenes[s.scene];
    let (Some(stem), Some(panel)) = (plan.track().stem(&scene.dominant), plan.lead_panel(s.scene)) else {
        return Svgr::empty();
    };
    let Some(last) = stem.last_onset_at(s.t) else { return Svgr::empty() };
    let slider = plan.layouts[s.scene].slider;
    let (arm, font) = (plan.dir.tracker.arm, plan.dir.font.clone());
    let color = if s.look.lock { LOCK_RED } else { INK };
    let at = |i: usize| {
        let n = stem.notes[i];
        let q = panel.note_pos(slider, &plan.timeline, s.t, n.onset, register(stem, n.pitch));
        project(&s.camera, q, RW, RH)
    };

    let mut items: Vec<Svgr> = vec![fframes::svgr!(
        <defs>
            <pattern id="tracker-hatch" width="8" height="8" patternTransform="rotate(45)" patternUnits="userSpaceOnUse">
                <line x1="0" y1="0" x2="0" y2="8" stroke="#ffffff" stroke-width="1.5" />
            </pattern>
        </defs>
    )];
    let trail: Vec<(f32, f32)> = (0..=last).rev().take(plan.dir.tracker.trail + 1).filter_map(at).collect();
    for (k, w) in trail.windows(2).enumerate() {
        items.push(fframes::svgr!(<line x1={w[0].0} y1={w[0].1} x2={w[1].0} y2={w[1].1} stroke={INK} stroke-width="1.2" opacity="0.7" />));
        let p = ((s.index as f32 * 0.16 + k as f32 * 0.33)).fract();
        let mx = w[0].0 + (w[1].0 - w[0].0) * p;
        let my = w[0].1 + (w[1].1 - w[0].1) * p;
        items.push(fframes::svgr!(<circle cx={mx} cy={my} r="3.2" fill={INK} />));
    }
    for q in &trail {
        items.push(fframes::svgr!(<circle cx={q.0} cy={q.1} r="3" fill={INK} />));
    }

    if let Some(c) = at(last) {
        // Radiating camera tails from target to frame edge (from ff-tracking)
        let (dx, dy) = (c.0 - RW * 0.5, c.1 - RH * 0.5);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let ex = c.0 + dx / len * 2500.0;
        let ey = c.1 + dy / len * 2500.0;
        items.push(fframes::svgr!(<line x1={c.0} y1={c.1} x2={ex} y2={ey} stroke={INK} stroke-width="1.0" opacity="0.35" stroke-dasharray="4,8" />));

        let grow = plan.dir.tracker.grow + if s.look.lock { 6.0 } else { 0.0 };
        let (x0, y0, x1, y1) = (c.0 - 34.0 - grow, c.1 - 20.0 - grow, c.0 + 34.0 + grow, c.1 + 20.0 + grow);
        let corner = |x: f32, y: f32, sx: f32, sy: f32| {
            format!("M {:.1} {:.1} L {x:.1} {y:.1} L {:.1} {:.1}", x + sx * arm, y, x, y + sy * arm)
        };
        let d = [corner(x0, y0, 1.0, 1.0), corner(x1, y0, -1.0, 1.0), corner(x1, y1, -1.0, -1.0), corner(x0, y1, 1.0, -1.0)].join(" ");
        items.push(fframes::svgr!(<path d={d} fill="none" stroke={color} stroke-width="2.4" />));
        let n = stem.notes[last];
        let label = format!(
            "{}{} {} {:.0} Hz [x:{:.0} y:{:.0}]",
            if s.look.lock { "LOCK " } else { "" },
            scene.dominant,
            note_name(n.pitch),
            hz(n.pitch),
            c.0,
            c.1
        );
        items.push(fframes::svgr!(<text x={x0} y={y0 - 9.0} font-family={font} font-size="18" fill={color}>{label}</text>));
    }

    // Text stream tracker in camera space (combining ff-tracking with ff-unmute)
    if let Some(ref txt) = scene.text {
        let elapsed = (s.t - scene.t0).max(0.0);
        let type_span = (scene.duration() * 0.75).max(0.6);
        let n_chars = txt.chars().count();
        let count = ((elapsed / type_span) * n_chars as f32).ceil().min(n_chars as f32) as usize;
        let typed: String = txt.chars().take(count).collect();
        let prompt_x = 340.0;
        let char_w = 10.5;
        let last_space = typed.rfind(' ').map_or(0, |idx| idx + 1);
        let word_len = count.saturating_sub(last_space).max(1);
        let wx = prompt_x + 22.0 + last_space as f32 * char_w;
        let ww = word_len as f32 * char_w;
        let word_pos = (wx + ww * 0.5, 77.0);

        if let Some(tp) = project(&s.camera, word_pos, RW, RH) {
            let tcolor = if s.look.lock { LOCK_RED } else { INK };
            let (tx0, ty0, tx1, ty1) = (tp.0 - ww * 0.6 * s.camera.zoom - 8.0, tp.1 - 16.0, tp.0 + ww * 0.6 * s.camera.zoom + 8.0, tp.1 + 16.0);
            let corner = |x: f32, y: f32, sx: f32, sy: f32| {
                format!("M {:.1} {:.1} L {x:.1} {y:.1} L {:.1} {:.1}", x + sx * 10.0, y, x, y + sy * 10.0)
            };
            let td = [corner(tx0, ty0, 1.0, 1.0), corner(tx1, ty0, -1.0, 1.0), corner(tx1, ty1, -1.0, -1.0), corner(tx0, ty1, 1.0, -1.0)].join(" ");
            items.push(fframes::svgr!(<path d={td} fill="none" stroke={tcolor} stroke-width="1.8" opacity="0.8" />));
            items.push(fframes::svgr!(<circle cx={tp.0} cy={tp.1} r="3" fill={tcolor} opacity="0.75" />));
            let tlabel = format!("TOKEN [x:{:.0} y:{:.0}]", tp.0, tp.1);
            items.push(fframes::svgr!(<text x={tx0} y={ty0 - 6.0} font-family={plan.dir.font.clone()} font-size="14" fill={tcolor}>{tlabel}</text>));

            // Dynamic link line from active leading note to active streaming text
            if let Some(c) = at(last) {
                let pp = ((s.index as f32 * 0.22)).fract();
                let pmx = c.0 + (tp.0 - c.0) * pp;
                let pmy = c.1 + (tp.1 - c.1) * pp;
                items.push(fframes::svgr!(<g opacity="0.5">
                    <line x1={c.0} y1={c.1} x2={tp.0} y2={tp.1} stroke={tcolor} stroke-width="1.0" stroke-dasharray="3,5" />
                    <circle cx={pmx} cy={pmy} r="2.8" fill={tcolor} />
                </g>));
            }
        }
    }

    items.extend(boxes(plan, s));
    fframes::svgr!(<g opacity={0.92 * (1.0 - s.look.out)}>{items}</g>)
}

/// A box on every hit of every stem in the scene's layout, held for `tracker.hold_s` while it
/// shrinks onto the note, with the stem, the pitch and a measured peak when the track has one.
fn boxes<'a>(plan: &Plan, s: &FrameState) -> Vec<Svgr<'a>> {
    let tk = &plan.dir.tracker;
    let layout = &plan.layouts[s.scene];
    let mut hits: Vec<(f32, &str, u8, (f32, f32))> = vec![];
    for panel in &s.panels {
        let Some(stem) = plan.track().stem(&panel.stem) else { continue };
        let Some(last) = stem.last_onset_at(s.t) else { continue };
        for n in stem.notes[..=last].iter().rev().take_while(|n| s.t - n.onset < tk.hold_s) {
            let q = panel.note_pos(layout.slider, &plan.timeline, s.t, n.onset, register(stem, n.pitch));
            if let Some(p) = project(&s.camera, q, RW, RH) {
                hits.push((s.t - n.onset, panel.stem.as_str(), n.pitch, p));
            }
        }
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.truncate(tk.box_cap);
    let font = plan.dir.font.clone();
    let mut out = vec![];
    for (age, name, pitch, p) in hits {
        let u = age / tk.hold_s;
        let fresh = u < 0.2;
        let color = if fresh { LOCK_RED } else { INK };
        let (hw, hh) = (tk.box_w * (1.0 + 0.8 * (1.0 - u)), tk.box_h * (1.0 + 0.8 * (1.0 - u)));
        let alpha = 0.85 * (1.0 - u * u);
        let label = if fresh {
            format!("LOCK {name} [x:{:.0} y:{:.0}]", p.0, p.1)
        } else {
            format!("{name} {} [x:{:.0} y:{:.0}]", note_name(pitch), p.0, p.1)
        };
        let (x, y) = (p.0 - hw, p.1 - hh);
        let w = 2.0 * hw;
        let h = 2.0 * hh;
        out.push(fframes::svgr!(<g opacity={alpha}>
            <rect x={x} y={y} width={w} height={h} fill="url(#tracker-hatch)" style="mix-blend-mode: difference" opacity={if fresh { "0.9" } else { "0.55" }} />
            <rect x={x} y={y} width={w} height={h} fill="none" stroke={color} stroke-width="1.6" />
            <text x={x} y={y - 5.0} font-family={font.clone()} font-size="13" fill={color}>{label}</text>
        </g>));
    }
    out
}
