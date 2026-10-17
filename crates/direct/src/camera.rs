//! The virtual camera: where it looks and how it moves in each scene.
//!
//! A behaviour places the camera on the scene's subject (a panel, or the whole stage) and moves
//! it in a closed form of the time since the scene began, so any frame can be evaluated alone.
//! The modulation matrix then adds the music-driven motion on top.

use crate::geometry::{H, Rect, W, ease_in_out, ease_out, lerp, wander};
use crate::scenes::Direction;
use crate::settings::CameraLimits;
use crate::vocab::BehaviorKind;

/// Must match `FOCAL` in lens.sksl.
pub const FOCAL: f32 = 1.7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub aim: (f32, f32),
    pub zoom: f32,
    pub tilt_x: f32,
    pub tilt_y: f32,
    pub roll: f32,
    /// Depth-of-field strength added by the move itself (a whip smears).
    pub blur: f32,
}

/// What a behaviour needs to know about its scene.
pub struct Setting {
    pub subject: Rect,
    /// Centre of the previous scene's subject, for arrivals.
    pub from: (f32, f32),
    /// Seconds into the scene and its length.
    pub age: f32,
    pub length: f32,
    /// Progress through the current phrase, 0..1.
    pub phrase_phase: f32,
    /// Newest note of the leading stem, for `Follow`.
    pub follow: (f32, f32),
    pub scene_index: usize,
}

/// Zoom at which `r` fills `fit` of the frame.
pub fn fit_zoom(r: Rect, fit: f32) -> f32 {
    (W / r.w).min(H / r.h) * fit
}

pub fn behave(kind: BehaviorKind, s: &Setting, d: &Direction) -> Camera {
    let fit = fit_zoom(s.subject, d.fit).max(1.0);
    let (cx, cy) = s.subject.center();
    let u = (s.age / s.length.max(1e-3)).clamp(0.0, 1.0);
    let still = Camera { aim: (cx, cy), zoom: fit, tilt_x: 0.0, tilt_y: 0.0, roll: 0.0, blur: 0.0 };
    match kind {
        BehaviorKind::Hold { zoom } => Camera { zoom: fit * zoom, ..still },
        BehaviorKind::Push { from, to } => Camera { zoom: fit * lerp(from, to, ease_in_out(u)), ..still },
        BehaviorKind::Orbit { zoom, tilt, roll } => {
            let phi = s.phrase_phase * std::f32::consts::TAU;
            Camera { zoom: fit * zoom, tilt_x: tilt * phi.sin(), tilt_y: tilt * phi.cos(), roll: roll * (2.0 * phi).sin(), ..still }
        }
        BehaviorKind::Whip { zoom } => {
            let w = (s.age / d.whip_s).clamp(0.0, 1.0);
            let k = ease_out(w);
            Camera {
                aim: (lerp(s.from.0, cx, k), lerp(s.from.1, cy, k)),
                zoom: fit * zoom * (1.0 - 0.3 * (1.0 - k)),
                blur: d.whip_blur * (w * std::f32::consts::PI).sin(),
                ..still
            }
        }
        BehaviorKind::Drift { zoom, amp } => {
            let (a, b, c) = (
                wander(d.seed, 10 + s.scene_index as u64, s.age, 0.7),
                wander(d.seed, 100 + s.scene_index as u64, s.age, 0.7),
                wander(d.seed, 1000 + s.scene_index as u64, s.age, 0.5),
            );
            Camera { aim: (cx + amp * a, cy + amp * b), zoom: fit * zoom, roll: 0.012 * c, ..still }
        }
        BehaviorKind::Follow { zoom } => {
            let inner = s.subject.inset(s.subject.h.min(s.subject.w) * 0.1);
            Camera {
                aim: (s.follow.0.clamp(inner.x, inner.right()), s.follow.1.clamp(inner.y, inner.bottom())),
                zoom: fit * zoom,
                ..still
            }
        }
    }
}

/// Keep the view inside the stage so a long move never shows the black beyond the screen.
pub fn keep_in_stage(c: &mut Camera, lim: &CameraLimits) {
    c.zoom = c.zoom.clamp(1.0, lim.max_zoom);
    let (hx, hy) = (W * 0.5 / c.zoom * lim.stage_slack, H * 0.5 / c.zoom * lim.stage_slack);
    c.aim.0 = c.aim.0.clamp(hx, W - hx);
    c.aim.1 = c.aim.1.clamp(hy, H - hy);
}

/// Shift the aim just far enough that `point` projects inside the safe area of the frame.
/// Two passes absorb the small error that tilt and roll add to the first estimate.
pub fn keep_in_view(c: &mut Camera, point: (f32, f32), lim: &CameraLimits) {
    for _ in 0..2 {
        let Some(p) = project(c, point, W, H) else { return };
        let (lo_x, hi_x, lo_y, hi_y) = (W * lim.safe_x, W * (1.0 - lim.safe_x), H * lim.safe_y, H * (1.0 - lim.safe_y));
        let (dx, dy) = (p.0.clamp(lo_x, hi_x) - p.0, p.1.clamp(lo_y, hi_y) - p.1);
        // One logical pixel is `zoom` output pixels at the aim.
        c.aim.0 -= dx / c.zoom;
        c.aim.1 -= dy / c.zoom;
    }
}

type V3 = [f32; 3];

fn mat_mul_vec(cols: &[V3; 3], v: V3) -> V3 {
    [
        cols[0][0] * v[0] + cols[1][0] * v[1] + cols[2][0] * v[2],
        cols[0][1] * v[0] + cols[1][1] * v[1] + cols[2][1] * v[2],
        cols[0][2] * v[0] + cols[1][2] * v[1] + cols[2][2] * v[2],
    ]
}

/// Same rotation as `rotation()` in lens.sksl (SkSL matrices are column-major): Rz * Ry * Rx.
fn rotate(c: &Camera, v: V3) -> V3 {
    let (cx, sx) = (c.tilt_x.cos(), c.tilt_x.sin());
    let (cy, sy) = (c.tilt_y.cos(), c.tilt_y.sin());
    let (cz, sz) = (c.roll.cos(), c.roll.sin());
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, sx], [0.0, -sx, cx]];
    let ry = [[cy, 0.0, -sy], [0.0, 1.0, 0.0], [sy, 0.0, cy]];
    let rz = [[cz, sz, 0.0], [-sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mat_mul_vec(&rz, mat_mul_vec(&ry, mat_mul_vec(&rx, v)))
}

/// Where a logical point of the flat screen lands in the lens output. `None` behind the camera.
pub fn project(c: &Camera, q: (f32, f32), res_w: f32, res_h: f32) -> Option<(f32, f32)> {
    let k = FOCAL * H / c.zoom;
    let (lx, ly) = ((q.0 - c.aim.0) / k, (q.1 - c.aim.1) / k);
    let eu = rotate(c, [1.0, 0.0, 0.0]);
    let ev = rotate(c, [0.0, 1.0, 0.0]);
    let p = [
        eu[0] * lx + ev[0] * ly,
        eu[1] * lx + ev[1] * ly,
        1.0 + eu[2] * lx + ev[2] * ly,
    ];
    if p[2] <= 0.05 {
        return None;
    }
    Some((FOCAL * p[0] / p[2] * res_h + 0.5 * res_w, FOCAL * p[1] / p[2] * res_h + 0.5 * res_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Direction {
        Direction::parse(include_str!("../../../vocab/direction.json")).unwrap()
    }

    fn setting(subject: Rect, age: f32) -> Setting {
        Setting { subject, from: (300.0, 300.0), age, length: 4.0, phrase_phase: 0.25, follow: (700.0, 500.0), scene_index: 3 }
    }

    #[test]
    fn a_hold_centres_the_subject_and_fills_the_frame() {
        let r = Rect::new(400.0, 300.0, 960.0, 540.0);
        let c = behave(BehaviorKind::Hold { zoom: 1.0 }, &setting(r, 1.0), &dir());
        assert_eq!(c.aim, r.center());
        let p0 = project(&c, (r.x, r.y), W, H).unwrap();
        let p1 = project(&c, (r.right(), r.bottom()), W, H).unwrap();
        assert!(p0.0 > 0.0 && p1.0 < W && p0.1 > 0.0 && p1.1 < H, "{p0:?} {p1:?}");
        assert!((p1.0 - p0.0) > 0.85 * W * 0.92 * 0.9, "{}", p1.0 - p0.0);
    }

    #[test]
    fn the_aim_projects_to_the_frame_centre() {
        let c = Camera { aim: (700.0, 400.0), zoom: 2.0, tilt_x: 0.2, tilt_y: -0.1, roll: 0.05, blur: 0.0 };
        let p = project(&c, c.aim, W, H).unwrap();
        assert!((p.0 - W * 0.5).abs() < 1e-2 && (p.1 - H * 0.5).abs() < 1e-2, "{p:?}");
    }

    #[test]
    fn a_whip_starts_on_the_previous_subject_and_arrives_on_the_new_one() {
        let r = Rect::new(900.0, 500.0, 600.0, 340.0);
        let d = dir();
        let start = behave(BehaviorKind::Whip { zoom: 1.3 }, &setting(r, 0.0), &d);
        let end = behave(BehaviorKind::Whip { zoom: 1.3 }, &setting(r, d.whip_s), &d);
        assert_eq!(start.aim, (300.0, 300.0));
        assert_eq!(end.aim, r.center());
        let mid = behave(BehaviorKind::Whip { zoom: 1.3 }, &setting(r, d.whip_s * 0.5), &d);
        assert!(mid.blur > 0.5 && start.blur == 0.0 && end.blur.abs() < 1e-4);
    }

    #[test]
    fn a_push_moves_monotonically_closer() {
        let r = Rect::new(90.0, 120.0, 1740.0, 840.0);
        let z = |age| behave(BehaviorKind::Push { from: 1.0, to: 1.3 }, &setting(r, age), &dir()).zoom;
        assert!(z(0.0) < z(2.0) && z(2.0) < z(4.0));
    }

    #[test]
    fn drift_is_deterministic_and_stays_within_its_amplitude() {
        let r = Rect::new(400.0, 300.0, 600.0, 340.0);
        let a = |age| behave(BehaviorKind::Drift { zoom: 1.0, amp: 16.0 }, &setting(r, age), &dir());
        assert_eq!(a(1.3), a(1.3));
        for k in 0..100 {
            let c = a(k as f32 * 0.1);
            assert!((c.aim.0 - r.center().0).abs() <= 16.0 * 1.3 && (c.aim.1 - r.center().1).abs() <= 16.0 * 1.3);
        }
    }

    #[test]
    fn keep_in_view_brings_a_point_inside_the_safe_area_and_leaves_one_already_there() {
        let mut c = Camera { aim: (960.0, 540.0), zoom: 2.0, tilt_x: 0.1, tilt_y: -0.1, roll: 0.0, blur: 0.0 };
        let before = c;
        keep_in_view(&mut c, (960.0, 540.0), &dir().camera);
        assert_eq!(c, before);
        keep_in_view(&mut c, (960.0, 120.0), &dir().camera);
        let p = project(&c, (960.0, 120.0), W, H).unwrap();
        assert!(p.1 >= H * dir().camera.safe_y - 2.0 && p.1 <= H * (1.0 - dir().camera.safe_y) + 2.0, "{p:?}");
    }

    #[test]
    fn keep_in_stage_clamps_the_view() {
        let mut c = Camera { aim: (5.0, 5.0), zoom: 3.0, tilt_x: 0.0, tilt_y: 0.0, roll: 0.0, blur: 0.0 };
        keep_in_stage(&mut c, &dir().camera);
        assert!(c.aim.0 >= W / 6.0 * 0.9 - 1e-3 && c.aim.1 >= H / 6.0 * 0.9 - 1e-3);
        c.zoom = 99.0;
        keep_in_stage(&mut c, &dir().camera);
        assert_eq!(c.zoom, dir().camera.max_zoom);
    }
}
