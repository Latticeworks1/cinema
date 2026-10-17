//! Write the SVG of one flat-pass frame to stdout: `dump <frame> > frame.svg`. The spectrum
//! strip needs the soundtrack, so this dump leaves it out.
use cinema_flat::FlatVideo;
use direct::Plan;

fn main() {
    let f: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).expect("usage: dump <frame>");
    let plan = Plan::load("data/analysis.json", "vocab").expect("plan");
    print!("{}", FlatVideo::new(plan).artwork(f, None));
}
