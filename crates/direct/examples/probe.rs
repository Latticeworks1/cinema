//! Print the state of one frame: scene, camera, subject, and where the lead note lands.
use direct::{Plan, project, W, H};

fn main() {
    let f: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).expect("usage: probe <frame>");
    let plan = Plan::load("data/analysis.json", "vocab").expect("plan");
    let s = plan.frame(f);
    let sc = &plan.scenes[s.scene];
    println!("frame {f} t={:.3} scene {} [{:.2},{:.2}] lead {} behavior {} layout {}", s.t, sc.index, sc.t0, sc.t1, sc.dominant,
        plan.vocab.behaviors[sc.behavior].name, plan.vocab.layouts[sc.layout].name);
    println!("camera {:?}", s.camera);
    println!("focus {:?} -> screen {:?}", s.focus, project(&s.camera, s.focus, W, H));
    if let Some(p) = plan.lead_panel(s.scene) { println!("lead panel {:?}", p.rect); }
    println!("look {:?}", s.look);
}
