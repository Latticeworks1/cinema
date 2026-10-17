//! The camera's own tracker, drawn sharp over the filmed screen: brackets and a label on the
//! newest note of the leading stem, and links through its last few notes.

use direct::{FrameState, Plan, project, register};
use fframes::Svgr;
use track::note_name;

const RW: f32 = super::WIDTH as f32;
const RH: f32 = super::HEIGHT as f32;
const MONO: &str = "JetBrains Mono";
const INK: &str = "#ffffff";
const LOCK_RED: &str = "#ff3b30";
/// Notes joined by links behind the newest one.
const TRAIL: usize = 4;
const ARM: f32 = 18.0;
const GROW: f32 = 8.0;

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
    let color = if s.look.lock { LOCK_RED } else { INK };
    let at = |i: usize| {
        let n = stem.notes[i];
        let q = panel.note_pos(slider, &plan.timeline, s.t, n.onset, register(stem, n.pitch));
        project(&s.camera, q, RW, RH)
    };

    let mut items: Vec<Svgr> = vec![];
    let trail: Vec<(f32, f32)> = (0..=last).rev().take(TRAIL + 1).filter_map(at).collect();
    for w in trail.windows(2) {
        items.push(fframes::svgr!(<line x1={w[0].0} y1={w[0].1} x2={w[1].0} y2={w[1].1} stroke={INK} stroke-width="1.2" opacity="0.7" />));
    }
    for q in &trail {
        items.push(fframes::svgr!(<circle cx={q.0} cy={q.1} r="3" fill={INK} />));
    }

    if let Some(c) = at(last) {
        let grow = GROW + if s.look.lock { 6.0 } else { 0.0 };
        let (x0, y0, x1, y1) = (c.0 - 34.0 - grow, c.1 - 20.0 - grow, c.0 + 34.0 + grow, c.1 + 20.0 + grow);
        let corner = |x: f32, y: f32, sx: f32, sy: f32| {
            format!("M {:.1} {:.1} L {x:.1} {y:.1} L {:.1} {:.1}", x + sx * ARM, y, x, y + sy * ARM)
        };
        let d = [corner(x0, y0, 1.0, 1.0), corner(x1, y0, -1.0, 1.0), corner(x1, y1, -1.0, -1.0), corner(x0, y1, 1.0, -1.0)].join(" ");
        items.push(fframes::svgr!(<path d={d} fill="none" stroke={color} stroke-width="2.4" />));
        let n = stem.notes[last];
        let label = format!(
            "{}{} {} {:.1} Hz",
            if s.look.lock { "LOCK " } else { "" },
            scene.dominant,
            note_name(n.pitch),
            hz(n.pitch)
        );
        items.push(fframes::svgr!(<text x={x0} y={y0 - 9.0} font-family={MONO} font-size="19" fill={color}>{label}</text>));
    }
    fframes::svgr!(<g opacity={0.92 * (1.0 - s.look.out)}>{items}</g>)
}
