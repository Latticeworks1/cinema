//! Print the feel profile of a track: every bar pooled, and each stem across the body of the track.
use signals::{FeelModel, FeelSettings, Selection};
use track::Track;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let track = Track::load(args.get(1).map_or("data/analysis.json", String::as_str)).expect("analysis");
    let settings = FeelSettings::from_json(&std::fs::read_to_string("vocab/feel.json").expect("vocab/feel.json")).expect("settings");
    let model = FeelModel::fit(&track, settings);
    let bar = track.bar_s;
    println!("bar  start   edge  energy   chord");
    let mut k = 0usize;
    while (k + 1) as f32 * bar <= track.duration_s + 0.01 {
        let (t0, t1) = (k as f32 * bar, ((k + 1) as f32 * bar).min(track.duration_s));
        let a = model.axes(&track, Selection::All, t0, t1);
        let chord = track.chord_at(t0 + 0.01).map_or("-", |c| c.name.as_str());
        match a {
            Some(a) => println!("{k:>3} {t0:>6.2} {:>6.2} {:>7.2}   {chord}", a.edge, a.energy),
            None => println!("{k:>3} {t0:>6.2}   none"),
        }
        k += 1;
    }
    println!("\nstem   edge  energy  (drop to impact)");
    for s in &track.stems {
        if let Some(a) = model.axes(&track, Selection::Stem(&s.name), track.drop_t, track.impact_t) {
            println!("{:<6} {:>5.2} {:>6.2}", s.name, a.edge, a.energy);
        }
    }
}
