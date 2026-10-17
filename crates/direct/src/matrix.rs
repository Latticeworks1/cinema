//! The modulation matrix: every parameter of the picture as a weighted sum of bus signals.
//!
//! A route reads one source, shapes it with a curve, optionally averages it over a short
//! trailing window (`smooth_s`) and reads it ahead of the frame (`lead_s`, which a score
//! allows and a live signal does not), then adds `gain * shaped` to one parameter. One
//! source may feed many parameters and one parameter may take many sources.

use crate::geometry::smoothstep;
use serde::Deserialize;
use signals::{Bus, MAX_OSCILLATORS, Source};
use std::collections::HashSet;

/// Samples across a smoothing window.
const SMOOTH_TAPS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    Zoom,
    AimX,
    AimY,
    Roll,
    TiltX,
    TiltY,
    Blur,
    Tear,
    Punch,
    Bloom,
    Aberration,
    Grain,
    Led,
    Vignette,
    /// Scale pulse of the objects in the flat pass.
    Pulse,
    /// Particle emission multiplier in the flat pass.
    Emit,
}

pub const N_PARAMS: usize = 16;

impl Param {
    pub const ALL: [Param; N_PARAMS] = [
        Param::Zoom,
        Param::AimX,
        Param::AimY,
        Param::Roll,
        Param::TiltX,
        Param::TiltY,
        Param::Blur,
        Param::Tear,
        Param::Punch,
        Param::Bloom,
        Param::Aberration,
        Param::Grain,
        Param::Led,
        Param::Vignette,
        Param::Pulse,
        Param::Emit,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Param::Zoom => "zoom",
            Param::AimX => "aim_x",
            Param::AimY => "aim_y",
            Param::Roll => "roll",
            Param::TiltX => "tilt_x",
            Param::TiltY => "tilt_y",
            Param::Blur => "blur",
            Param::Tear => "tear",
            Param::Punch => "punch",
            Param::Bloom => "bloom",
            Param::Aberration => "aberration",
            Param::Grain => "grain",
            Param::Led => "led",
            Param::Vignette => "vignette",
            Param::Pulse => "pulse",
            Param::Emit => "emit",
        }
    }

    pub fn parse(name: &str) -> Option<Param> {
        Param::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// Summed route outputs, indexed by [`Param::index`].
pub type Modulation = [f32; N_PARAMS];

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Linear,
    Square,
    Sqrt,
    /// One sine period over the source's 0..1 range.
    Sine,
    Smooth,
}

impl Curve {
    fn apply(self, v: f32) -> f32 {
        match self {
            Curve::Linear => v,
            Curve::Square => v * v,
            Curve::Sqrt => v.max(0.0).sqrt(),
            Curve::Sine => (v * std::f32::consts::TAU).sin(),
            Curve::Smooth => smoothstep(v),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoute {
    from: String,
    to: String,
    gain: f32,
    #[serde(default)]
    curve: Curve,
    #[serde(default)]
    smooth_s: f32,
    #[serde(default)]
    lead_s: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatrix {
    routes: Vec<RawRoute>,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub source: Source,
    pub to: Param,
    pub gain: f32,
    pub curve: Curve,
    pub smooth_s: f32,
    pub lead_s: f32,
}

#[derive(Clone, Debug)]
pub struct Matrix {
    pub routes: Vec<Route>,
}

impl Matrix {
    /// Parse a matrix, refusing unknown parameters, malformed sources and stems that the
    /// vocabulary does not declare.
    pub fn parse(json: &str, declared_stems: &HashSet<String>) -> Result<Matrix, String> {
        let raw: RawMatrix = serde_json::from_str(json).map_err(|e| format!("matrix: {e}"))?;
        let mut routes = vec![];
        for (i, r) in raw.routes.into_iter().enumerate() {
            let source = Source::parse(&r.from).map_err(|e| format!("matrix route {i}: {e}"))?;
            if let Source::Stem(name, _) = &source {
                if !declared_stems.contains(name) {
                    return Err(format!("matrix route {i}: `{}` names no declared stem", r.from));
                }
            }
            if let Source::Rhythm(rank, _) = &source {
                if *rank >= MAX_OSCILLATORS {
                    return Err(format!("matrix route {i}: `{}` names no oscillator rank (the bus has {MAX_OSCILLATORS})", r.from));
                }
            }
            let to = Param::parse(&r.to).ok_or_else(|| format!("matrix route {i}: no parameter `{}`", r.to))?;
            if !(r.gain.is_finite() && r.smooth_s >= 0.0 && r.lead_s.is_finite()) {
                return Err(format!("matrix route {i}: gain, smooth_s and lead_s must be finite, smooth_s non-negative"));
            }
            routes.push(Route { source, to, gain: r.gain, curve: r.curve, smooth_s: r.smooth_s, lead_s: r.lead_s });
        }
        Ok(Matrix { routes })
    }

    /// Output of one route at `t`.
    pub fn route_value(&self, route: &Route, bus: &Bus, t: f32) -> f32 {
        let at = t + route.lead_s;
        let raw = if route.smooth_s > 0.0 {
            (0..SMOOTH_TAPS)
                .map(|k| bus.value(&route.source, at - route.smooth_s * k as f32 / (SMOOTH_TAPS - 1) as f32))
                .sum::<f32>()
                / SMOOTH_TAPS as f32
        } else {
            bus.value(&route.source, at)
        };
        route.gain * route.curve.apply(raw)
    }

    pub fn evaluate(&self, bus: &Bus, t: f32) -> Modulation {
        let mut out = [0.0; N_PARAMS];
        for r in &self.routes {
            out[r.to.index()] += self.route_value(r, bus, t);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use track::Track;

    fn bus() -> Bus {
        Bus::new(Arc::new(
            Track::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/analysis.json")).unwrap(),
        ))
    }

    fn stems() -> HashSet<String> {
        ["kick", "snare", "hat", "bass", "piano"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parameters_round_trip_by_name() {
        for p in Param::ALL {
            assert_eq!(Param::parse(p.name()), Some(p));
            assert_eq!(Param::ALL[p.index()], p);
        }
    }

    #[test]
    fn one_source_fans_out_and_sums_into_a_parameter() {
        let m = Matrix::parse(
            r#"{"routes":[
                {"from":"kick.hit","to":"punch","gain":1.0},
                {"from":"kick.hit","to":"zoom","gain":0.05},
                {"from":"kick.hit","to":"punch","gain":0.5}]}"#,
            &stems(),
        )
        .unwrap();
        let b = bus();
        let onset = b.track().stem("kick").unwrap().notes[2].onset;
        let out = m.evaluate(&b, onset);
        assert!((out[Param::Punch.index()] - 1.5).abs() < 1e-5);
        assert!((out[Param::Zoom.index()] - 0.05).abs() < 1e-5);
    }

    #[test]
    fn a_lead_reads_the_score_ahead_of_the_frame() {
        let m = Matrix::parse(r#"{"routes":[{"from":"kick.hit","to":"punch","gain":1.0,"lead_s":0.1}]}"#, &stems()).unwrap();
        let b = bus();
        let onset = b.track().stem("kick").unwrap().notes[2].onset;
        assert!((m.evaluate(&b, onset - 0.1)[Param::Punch.index()] - 1.0).abs() < 1e-5);
        assert!(m.evaluate(&b, onset - 0.1)[Param::Punch.index()] > m.evaluate(&b, onset - 0.3)[Param::Punch.index()]);
    }

    #[test]
    fn smoothing_spreads_a_hit_backwards_in_time() {
        let sharp = Matrix::parse(r#"{"routes":[{"from":"kick.hit","to":"punch","gain":1.0}]}"#, &stems()).unwrap();
        let soft = Matrix::parse(r#"{"routes":[{"from":"kick.hit","to":"punch","gain":1.0,"smooth_s":0.2}]}"#, &stems()).unwrap();
        let b = bus();
        let onset = b.track().stem("kick").unwrap().notes[2].onset;
        assert!(soft.evaluate(&b, onset)[Param::Punch.index()] < sharp.evaluate(&b, onset)[Param::Punch.index()]);
    }

    #[test]
    fn typos_are_refused() {
        for bad in [
            r#"{"routes":[{"from":"kik.hit","to":"punch","gain":1.0}]}"#,
            r#"{"routes":[{"from":"kick.hit","to":"puncch","gain":1.0}]}"#,
            r#"{"routes":[{"from":"kick.hot","to":"punch","gain":1.0}]}"#,
            r#"{"routes":[{"from":"kick.hit","to":"punch","gain":1.0,"curv":"sine"}]}"#,
            r#"{"routes":[{"from":"rhythm.6.pulse","to":"punch","gain":1.0}]}"#,
        ] {
            assert!(Matrix::parse(bad, &stems()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_stem_missing_from_the_track_contributes_nothing() {
        let m = Matrix::parse(r#"{"routes":[{"from":"piano.hit","to":"bloom","gain":1.0}]}"#, &stems()).unwrap();
        let full = bus();
        let t = full.track().stem("piano").unwrap().notes[0].onset;
        assert!(m.evaluate(&full, t)[Param::Bloom.index()] > 0.9);
        let muted = Bus::new(Arc::new(full.track().without_stem("piano")));
        assert_eq!(m.evaluate(&muted, t)[Param::Bloom.index()], 0.0);
    }
}
