//! Prints the oscillators inferred from a track. Usage: `cargo run --release -p signals --example rhythm [analysis.json]`.

use signals::{Rhythm, RhythmSignal, SignalSettings};
use track::{FPS, Track};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "data/analysis.json".into());
    let track = Track::load(&path).expect("analysis");
    let rhythm = Rhythm::fit(&track, &SignalSettings::shipped().rhythm);
    println!("authored beat {:.3} s, bar {:.3} s (not read by the fit)", 60.0 / track.bpm, track.bar_s);
    println!("rank  period_s  /prev   z      conf   strength  mean_amp");
    let mut prev = None;
    for (k, o) in rhythm.oscillators().iter().enumerate() {
        let mean_amp: f32 = (0..track.frames()).map(|f| rhythm.value(k, RhythmSignal::Amp, f as f32 / FPS as f32)).sum::<f32>() / track.frames() as f32;
        let ratio = prev.map_or("   -".to_string(), |p: f64| format!("{:5.2}", o.period_s / p));
        println!("{k:>4}  {:>8.4}  {ratio}  {:>5.1}  {:.3}  {:>8.3}  {mean_amp:>8.3}", o.period_s, o.z, o.confidence, o.strength);
        prev = Some(o.period_s);
    }
}
