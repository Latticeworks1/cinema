//! Print the scene plan of the packet named by `CINEMA_PACKET` (default `packets/default.json`)
//! and write it to the packet's `plan.json`.
use direct::Plan;
use packet::Packet;

fn main() {
    let (packet, plan) = match Packet::from_env().and_then(|p| Plan::from_packet(&p).map(|plan| (p, plan))) {
        Ok(x) => x,
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
    let out = packet.plan_path();
    std::fs::write(&out, serde_json::to_string_pretty(&plan.report()).unwrap()).unwrap_or_else(|e| {
        eprintln!("plan: {}: {e}", out.display());
        std::process::exit(1);
    });
    println!("\nwrote {} ({} scenes)", out.display(), plan.scenes.len());
}
