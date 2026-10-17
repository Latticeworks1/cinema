//! Everything the signals measure with, as data. The shipped values are `vocab/signals.json`;
//! a packet overrides any of them for one run.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusSettings {
    /// Notes whose onsets differ by less than this are one event (a chord).
    pub same_onset_s: f32,
    /// How many notes back the held-note and silence searches look.
    pub lookback: usize,
    /// The smallest and largest decay of a stem's note impulse, seconds.
    pub decay_min_s: f32,
    pub decay_max_s: f32,
    /// Percentiles of the loudness curve that map to 0 and 1.
    pub level_lo_pct: f32,
    pub level_hi_pct: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RhythmSettings {
    /// Notes starting within this of each other are one event.
    pub same_onset_s: f64,
    /// Width of a lag bin; onsets in the transcription are multiples of 10 ms, so not finer.
    pub bin_s: f64,
    pub min_lag_s: f64,
    /// A period must fit this many times into the span of the events.
    pub min_cycles: f64,
    pub surrogates: usize,
    /// Half-width of the uniform displacement in a surrogate.
    pub jitter_s: f64,
    /// Family-wise false-positive rate over all lags.
    pub alpha: f64,
    pub harmonics: usize,
    pub template_bins: usize,
    pub window_cycles: f64,
    pub min_window_s: f64,
    pub min_events: usize,
    pub refine_span_s: f64,
    pub refine_step_s: f64,
    /// Relative distance within which two periods are the same, or one a multiple of the other.
    pub period_tol: f64,
    pub seed: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalSettings {
    pub bus: BusSettings,
    pub rhythm: RhythmSettings,
}

impl SignalSettings {
    pub fn from_json(text: &str) -> Result<SignalSettings, String> {
        let s: SignalSettings = serde_json::from_str(text).map_err(|e| format!("signals settings: {e}"))?;
        let (b, r) = (&s.bus, &s.rhythm);
        let ok = b.same_onset_s >= 0.0
            && b.lookback > 0
            && 0.0 < b.decay_min_s
            && b.decay_min_s <= b.decay_max_s
            && 0.0 <= b.level_lo_pct
            && b.level_lo_pct < b.level_hi_pct
            && b.level_hi_pct <= 1.0
            && r.same_onset_s >= 0.0
            && r.bin_s > 0.0
            && r.min_lag_s >= r.bin_s
            && r.min_cycles >= 2.0
            && r.surrogates >= 20
            && r.jitter_s > r.bin_s
            && 0.0 < r.alpha
            && r.alpha < 0.5
            && r.harmonics >= 1
            && r.template_bins >= 16
            && r.window_cycles > 0.0
            && r.min_window_s > 0.0
            && r.min_events >= 4
            && r.refine_span_s >= 0.0
            && r.refine_step_s > 0.0
            && r.period_tol > 0.0;
        if ok { Ok(s) } else { Err("signals settings: a value is out of range".into()) }
    }

    /// The values shipped in `vocab/signals.json`, for callers that have no packet.
    pub fn shipped() -> SignalSettings {
        SignalSettings::from_json(include_str!("../../../vocab/signals.json")).expect("shipped signals.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_parses_and_typos_and_bad_ranges_are_refused() {
        SignalSettings::shipped();
        let text = include_str!("../../../vocab/signals.json");
        assert!(SignalSettings::from_json(&text.replace("\"alpha\"", "\"alfa\"")).is_err());
        assert!(SignalSettings::from_json(&text.replace("\"surrogates\": 200", "\"surrogates\": 3")).is_err());
        assert!(SignalSettings::from_json(&text.replace("\"harmonics\": 4", "\"harmonics\": 0")).is_err());
    }
}
