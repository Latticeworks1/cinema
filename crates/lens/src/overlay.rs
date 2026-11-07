//! The camera's own tracker, drawn sharp over the filmed screen: brackets and a label on the
//! newest note of the leading stem, and links through its last few notes.

use direct::{FrameState, LyricState, Plan, project, register};
use fframes::Svgr;
use track::note_name;

const RW: f32 = super::WIDTH as f32;
const RH: f32 = super::HEIGHT as f32;

fn hz(pitch: u8) -> f32 {
    440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0)
}

pub fn tracker<'a>(plan: &'a Plan, s: &FrameState) -> Svgr<'a> {
    if let Some(ly) = &s.lyric {
        return lyric_tracker(plan, s, ly);
    }
    let scene = &plan.scenes[s.scene];
    let (Some(stem), Some(panel)) = (plan.track().stem(&scene.dominant), plan.lead_panel(s.scene)) else {
        return fframes::svgr!(<g opacity={1.0 - s.look.out}>{super::anchors::marks(plan, s, None)}</g>);
    };
    let Some(last) = stem.last_onset_at(s.t) else {
        return fframes::svgr!(<g opacity={1.0 - s.look.out}>{super::anchors::marks(plan, s, None)}</g>);
    };
    let slider = plan.layouts[s.scene].slider;
    let (arm, font) = (plan.dir.tracker.arm, plan.dir.font.clone());
    let ink = plan.dir.ink.lens_ink.as_str();
    let color = if s.look.lock { plan.dir.ink.lens_lock.as_str() } else { ink };
    let focal = plan.dir.camera.focal;
    let at = |i: usize| {
        let n = stem.notes[i];
        let q = panel.note_pos(slider, &plan.timeline, s.t, n.onset, register(stem, n.pitch));
        project(&s.camera, q, RW, RH, focal)
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
        items.push(fframes::svgr!(<line x1={w[0].0} y1={w[0].1} x2={w[1].0} y2={w[1].1} stroke={ink} stroke-width="1.2" opacity="0.7" />));
        let p = ((s.index as f32 * 0.16 + k as f32 * 0.33)).fract();
        let mx = w[0].0 + (w[1].0 - w[0].0) * p;
        let my = w[0].1 + (w[1].1 - w[0].1) * p;
        items.push(fframes::svgr!(<circle cx={mx} cy={my} r="3.2" fill={ink} />));
    }
    for q in &trail {
        items.push(fframes::svgr!(<circle cx={q.0} cy={q.1} r="3" fill={ink} />));
    }

    if let Some(c) = at(last) {
        // Radiating camera tails from target to frame edge (from ff-tracking)
        let (dx, dy) = (c.0 - RW * 0.5, c.1 - RH * 0.5);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let ex = c.0 + dx / len * 2500.0;
        let ey = c.1 + dy / len * 2500.0;
        items.push(fframes::svgr!(<line x1={c.0} y1={c.1} x2={ex} y2={ey} stroke={ink} stroke-width="1.0" opacity="0.35" stroke-dasharray="4,8" />));

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

    items.extend(super::anchors::marks(plan, s, at(last)));
    items.extend(boxes(plan, s));
    fframes::svgr!(<g opacity={0.92 * (1.0 - s.look.out)}>{items}</g>)
}

/// A box on every hit of every stem in the scene's layout, held for `tracker.hold_s` while it
/// shrinks onto the note, with the stem, the pitch and a measured peak when the track has one.
fn boxes<'a>(plan: &'a Plan, s: &FrameState) -> Vec<Svgr<'a>> {
    let tk = &plan.dir.tracker;
    let layout = &plan.layouts[s.scene];
    let mut hits: Vec<(f32, &str, u8, (f32, f32))> = vec![];
    for panel in &s.panels {
        let Some(stem) = plan.track().stem(&panel.stem) else { continue };
        let Some(last) = stem.last_onset_at(s.t) else { continue };
        for n in stem.notes[..=last].iter().rev().take_while(|n| s.t - n.onset < tk.hold_s) {
            let q = panel.note_pos(layout.slider, &plan.timeline, s.t, n.onset, register(stem, n.pitch));
            if let Some(p) = project(&s.camera, q, RW, RH, plan.dir.camera.focal) {
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
        let color = if fresh { plan.dir.ink.lens_lock.as_str() } else { plan.dir.ink.lens_ink.as_str() };
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

/// The camera's tracker on a lyric line: links through the newest words in reading order with a
/// dot running along each, brackets and a labelled coordinate on the newest word, and the boxes
/// of the notes under it.
fn lyric_tracker<'a>(plan: &'a Plan, s: &FrameState, ly: &LyricState) -> Svgr<'a> {
    let cfg = &plan.dir.lyric;
    let ink = plan.dir.ink.lens_ink.as_str();
    let color = if s.look.lock { plan.dir.ink.lens_lock.as_str() } else { ink };
    let font = plan.dir.font.clone();
    let pts: Vec<(String, (f32, f32), f32)> = ly
        .words
        .iter()
        .rev()
        .take(cfg.trail + 1)
        .rev()
        .filter_map(|&(start, len)| {
            let c = ly.word_center((start, len));
            project(&s.camera, c, RW, RH, plan.dir.camera.focal).map(|p| {
                let word: String = ly.text.chars().skip(start).take(len).collect();
                (word, p, len as f32 * ly.advance() * s.camera.zoom)
            })
        })
        .collect();
    let mut items: Vec<Svgr> = vec![fframes::svgr!(
        <defs>
            <pattern id="tracker-hatch" width="8" height="8" patternTransform="rotate(45)" patternUnits="userSpaceOnUse">
                <line x1="0" y1="0" x2="0" y2="8" stroke="#ffffff" stroke-width="1.5" />
            </pattern>
        </defs>
    )];
    for (k, w) in pts.windows(2).enumerate() {
        let (a, b) = (w[0].1, w[1].1);
        items.push(fframes::svgr!(<line x1={a.0} y1={a.1} x2={b.0} y2={b.1} stroke={ink} stroke-width="1.4" opacity="0.8" />));
        let p = (s.index as f32 * 0.16 + k as f32 * 0.33).fract();
        items.push(fframes::svgr!(<circle cx={a.0 + (b.0 - a.0) * p} cy={a.1 + (b.1 - a.1) * p} r="3.4" fill={ink} />));
    }
    for (_, p, _) in &pts {
        items.push(fframes::svgr!(<circle cx={p.0} cy={p.1} r="3" fill={ink} />));
    }
    if let Some((word, c, w)) = pts.last() {
        let (dx, dy) = (c.0 - RW * 0.5, c.1 - RH * 0.5);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        items.push(fframes::svgr!(<line x1={c.0} y1={c.1} x2={c.0 + dx / len * 2500.0} y2={c.1 + dy / len * 2500.0} stroke={ink} stroke-width="1.0" opacity="0.35" stroke-dasharray="4,8" />));
        let pad = plan.dir.tracker.grow + if s.look.lock { 6.0 } else { 0.0 };
        let (hw, hh) = (w * 0.5 + pad + 8.0, ly.size * 0.55 * s.camera.zoom + pad);
        let (x0, y0, x1, y1) = (c.0 - hw, c.1 - hh, c.0 + hw, c.1 + hh);
        let arm = plan.dir.tracker.arm.min(hw).min(hh);
        let corner = |x: f32, y: f32, sx: f32, sy: f32| format!("M {:.1} {:.1} L {x:.1} {y:.1} L {:.1} {:.1}", x + sx * arm, y, x, y + sy * arm);
        let d = [corner(x0, y0, 1.0, 1.0), corner(x1, y0, -1.0, 1.0), corner(x1, y1, -1.0, -1.0), corner(x0, y1, 1.0, -1.0)].join(" ");
        items.push(fframes::svgr!(<path d={d} fill="none" stroke={color} stroke-width="2.4" />));
        let label = format!("{}{} [x:{:.0} y:{:.0}]", if s.look.lock { "LOCK " } else { "" }, word, c.0, c.1);
        items.push(fframes::svgr!(<text x={x0} y={y0 - 10.0} font-family={font} font-size="18" fill={color}>{label}</text>));
    }
    items.extend(super::anchors::marks(plan, s, pts.last().map(|p| p.1)));
    items.extend(boxes(plan, s));
    fframes::svgr!(<g opacity={0.92 * (1.0 - s.look.out)}>{items}</g>)
}

/// The scene counter, time, BPM, bar, loudness meter and caption line: composited sharp over the
/// shaded image, the same way as the tracker, since this is information that must stay legible
/// rather than texture that goes through the display's LED model. It used to be drawn in the flat
/// pass and smeared along with everything else once a scene's camera zoomed out far enough to
/// shrink it below the display's subpixel cell size.
pub fn hud<'a>(plan: &'a Plan, s: &FrameState) -> Svgr<'a> {
    let sc = &plan.scenes[s.scene];
    let v = &plan.vocab;
    let tr = plan.track();
    let c = &plan.dir.chrome;
    let ink = &plan.dir.ink;
    let m = &plan.dir.meter;
    let font = plan.dir.font.clone();
    let bar = ((s.t - tr.drop_t) / tr.bar_s).floor() as i32 + 1;
    // Loudness in dBFS, held inside the meter range; a missing or non-finite value reads as the
    // floor of the range. Mirrors `flat::chrome::level_db`, which the lens pass has no need to
    // depend on the flat crate for.
    let db = match tr.level_db.get(s.index) {
        Some(&db) if db.is_finite() => db.clamp(c.meter_floor_db, c.meter_ceil_db),
        _ => c.meter_floor_db,
    };

    let mut items: Vec<Svgr> = vec![
        fframes::svgr!(<text x="90" y="84" font-family={font.clone()} font-size="20" letter-spacing="4" fill={ink.lens_hud_text.as_str()}>{format!("{:02}.{:02} {:05.2} s", sc.index + 1, plan.scenes.len(), s.t)}</text>),
        fframes::svgr!(<text x="1830" y="84" font-family={font.clone()} font-size="16" text-anchor="end" fill={ink.lens_hud_secondary.as_str()}>{format!("{:.1} BPM · bar {}", tr.bpm, bar.max(0))}</text>),
    ];

    if c.meter {
        let frac = (db - c.meter_floor_db) / (c.meter_ceil_db - c.meter_floor_db);
        items.push(fframes::svgr!(<rect x={m.x} y={m.y} width={m.w} height={m.h} fill="none" stroke={ink.lens_rule.as_str()} stroke-width="1.5" />));
        items.push(fframes::svgr!(<rect x={m.x} y={m.y} width={(frac * m.w).max(0.5)} height={m.h} fill={ink.lens_label.as_str()} />));
    }

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
    let label_fill = if sc.text.is_some() { ink.lens_hud_highlight.as_str() } else { ink.lens_hud_muted.as_str() };
    items.push(fframes::svgr!(<text x="90" y="1008" font-family={font.clone()} font-size="14" letter-spacing="2" fill={label_fill}>{label}</text>));

    if c.status {
        let beat = 60.0 / tr.bpm;
        let bpb = (tr.bar_s * tr.bpm / 60.0).round().max(1.0) as i64;
        let k = ((s.t - tr.drop_t) / beat).floor() as i64;
        let status_bar = k.div_euclid(bpb) + 1;
        let beat_in_bar = k.rem_euclid(bpb) + 1;
        let total_bars = (tr.duration_s / tr.bar_s).ceil().max(1.0) as i64;
        let text = format!(
            "{:.1} BPM  ·  bar {}/{}  ·  beat {}  ·  {:06.3} s  ·  {:>6.1} dBFS  ·  frame {}/{}",
            tr.bpm,
            status_bar.max(1),
            total_bars,
            beat_in_bar,
            s.t,
            db,
            s.index + 1,
            plan.frames()
        );
        items.push(fframes::svgr!(<text x="1830" y="1008" font-family={font} font-size="14" letter-spacing="1" text-anchor="end" fill={ink.lens_hud_muted.as_str()}>{text}</text>));
    } else {
        let axes_text = format!("edge {:+.2} · energy {:+.2}", sc.axes.edge, sc.axes.energy);
        items.push(fframes::svgr!(<text x="1830" y="1008" font-family={font} font-size="14" text-anchor="end" fill={ink.lens_hud_muted.as_str()}>{axes_text}</text>));
    }

    fframes::svgr!(<g>{items}</g>)
}
