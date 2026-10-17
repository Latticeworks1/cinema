//! Screen space, easing, and the deterministic noise the camera and the emitters share.
//!
//! All positions are in the flat pass's logical space, `W` x `H`; the flat pass draws it at 2x.

use serde::Serialize;

pub const W: f32 = 1920.0;
pub const H: f32 = 1080.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: W, h: H };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn inset(&self, d: f32) -> Rect {
        Rect { x: self.x + d, y: self.y + d, w: (self.w - 2.0 * d).max(0.0), h: (self.h - 2.0 * d).max(0.0) }
    }

    pub fn contains(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }
}

pub fn lerp(a: f32, b: f32, u: f32) -> f32 {
    a + (b - a) * u
}

pub fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// Keyed noise in 0..1: independent streams for one seed, so adding a consumer of one
/// stream never changes another. (Mixing from ff-tracking's `seeded`, MIT.)
pub fn rnd(seed: u64, stream: u64, key: u64) -> f32 {
    let mut x = seed ^ stream.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ key.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((x ^ (x >> 31)) >> 40) as f32 / 16_777_216.0
}

/// Catmull-Rom between `b` and `c`, with `a` and `d` the neighbours. (From ff-unmute's hud, MIT.)
pub fn catmull(a: f32, b: f32, c: f32, d: f32, u: f32) -> f32 {
    0.5 * ((2.0 * b) + (-a + c) * u + (2.0 * a - 5.0 * b + 4.0 * c - d) * u * u + (-a + 3.0 * b - 3.0 * c + d) * u * u * u)
}

/// Smooth noise in about -1..1 that moves at `rate` keys per second: Catmull-Rom through keyed values.
pub fn wander(seed: u64, stream: u64, t: f32, rate: f32) -> f32 {
    let x = t * rate;
    let i = x.floor() as i64;
    let key = |j: i64| rnd(seed, stream, (j + 1_000_000) as u64) * 2.0 - 1.0;
    catmull(key(i - 1), key(i), key(i + 1), key(i + 2), x - i as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_noise_is_stable_and_streams_are_independent() {
        assert_eq!(rnd(1, 2, 3), rnd(1, 2, 3));
        assert_ne!(rnd(1, 2, 3), rnd(1, 3, 3));
        assert!((0..1000).all(|k| (0.0..1.0).contains(&rnd(7, 1, k))));
    }

    #[test]
    fn wander_is_continuous() {
        let mut prev = wander(1, 1, 0.0, 2.0);
        for k in 1..2000 {
            let v = wander(1, 1, k as f32 * 0.005, 2.0);
            assert!((v - prev).abs() < 0.1, "jump at {k}");
            prev = v;
        }
    }

    #[test]
    fn easing_hits_its_endpoints() {
        assert_eq!(ease_out(0.0), 0.0);
        assert!((ease_out(1.0) - 1.0).abs() < 1e-6);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-6);
    }
}
