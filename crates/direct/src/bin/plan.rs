//! Print the scene plan of a track and write it to `out/plan.json`.
//!
//! Usage: plan [analysis.json] [vocab dir]
use direct::Plan;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let analysis = args.get(1).map_or("data/analysis.json", String::as_str);
    let vocab = args.get(2).map_or("vocab", String::as_str);
    let plan = match Plan::load(analysis, vocab) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("plan: {e}");
            std::process::exit(1);
        }
    };
    println!(" #   start    end   lead   palette   behavior     look         layout     edge  energy  starts with");
    let v = &plan.vocab;
    for s in &plan.scenes {
        println!(
            "{:>2} {:>7.2} {:>6.2}   {:<6} {:<9} {:<12} {:<12} {:<10} {:>5.2} {:>7.2}  {}",
            s.index,
            s.t0,
            s.t1,
            s.dominant,
            v.palettes[s.palette].name,
            v.behaviors[s.behavior].name,
            v.looks[s.look].name,
            v.layouts[s.layout].name,
            s.axes.edge,
            s.axes.energy,
            s.starts_with
        );
    }
    std::fs::create_dir_all("out").expect("create out/");
    std::fs::write("out/plan.json", serde_json::to_string_pretty(&plan.report()).unwrap()).expect("write out/plan.json");
    println!("\nwrote out/plan.json ({} scenes)", plan.scenes.len());
}
