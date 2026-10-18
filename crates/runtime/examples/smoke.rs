//! The full setup → inspect → edit → preview → export arc against a real packet, writing only
//! to a scratch directory (never `out/<name>`). Run with:
//!   cargo run --release -p cinema-runtime --example smoke
use cinema_runtime::Project;
use serde_json::Value;
use std::path::PathBuf;

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let packet_path = format!("{root}/packets/hopeless.json");
    let scratch = std::env::temp_dir().join(format!("cinema-runtime-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();

    // The flat_lock override only shows up on a frame where the leading stem is actually
    // locked (see cinema_flat's own `a_lock_frame_carries_the_red_marker...` test) — find one
    // for real rather than assuming an arbitrary frame index has the marker.
    let plan = direct::Plan::from_packet(&packet::Packet::load(&packet_path).unwrap()).unwrap();
    let lock_frame = (0..plan.frames()).find(|&f| plan.frame(f).look.lock).expect("a lock frame exists");
    println!("lock frame = {lock_frame} ({:.2}s)", lock_frame as f32 / track::FPS as f32);

    println!("open {packet_path}");
    let mut project = Project::open(&packet_path).expect("open");

    let state = project.state().expect("state");
    println!("default ink.flat_lock = {}", state["ink"]["flat_lock"]);
    assert_eq!(state["ink"]["flat_lock"], "#ff0000");

    project.set("ink.flat_lock", Value::String("#00ff00".into())).expect("set");
    let state = project.state().expect("state after set");
    println!("after set,  ink.flat_lock = {}", state["ink"]["flat_lock"]);
    assert_eq!(state["ink"]["flat_lock"], "#00ff00");
    assert_eq!(state["ink"]["flat_lit"], "#e6e6e6", "untouched sibling must survive the merge");

    println!("render_flat_frame({lock_frame}) with the override in effect");
    let png = project.render_flat_frame(lock_frame).expect("render_flat_frame");
    assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let frame_path: PathBuf = scratch.join("frame_lock_override.png");
    std::fs::write(&frame_path, &png).unwrap();
    println!("wrote {} ({} bytes)", frame_path.display(), png.len());

    println!("render_flat_frame({lock_frame}) again, without the override, to prove the pixels actually differ");
    let default_project = Project::open(&packet_path).expect("open (fresh, no override)");
    let default_png = default_project.render_flat_frame(lock_frame).expect("render_flat_frame (default)");
    assert_ne!(png, default_png, "overriding ink.flat_lock must change the rendered lock-marker pixels");
    println!("confirmed: the override changed the rendered bytes ({} vs {} bytes)", png.len(), default_png.len());

    println!("export_flat 0s..2s to the scratch directory");
    let flat_excerpt = scratch.join("flat_excerpt.mp4");
    project.export_flat(Some("0s..2s"), &flat_excerpt).expect("export_flat");
    let size = std::fs::metadata(&flat_excerpt).unwrap().len();
    println!("wrote {} ({size} bytes)", flat_excerpt.display());
    assert!(size > 0);

    // This packet's own out/hopeless/pass/flat.mp4 already exists (rendered in an earlier
    // session), so render_lens_frame can succeed against it without exporting a full pass here.
    println!("render_lens_frame({lock_frame}) against the existing pass/flat.mp4");
    let lens_png = project.render_lens_frame(lock_frame).expect("render_lens_frame");
    assert_eq!(&lens_png[0..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let lens_path = scratch.join("lens_frame_lock.png");
    std::fs::write(&lens_path, &lens_png).unwrap();
    println!("wrote {} ({} bytes)", lens_path.display(), lens_png.len());

    println!("all steps succeeded; scratch output left at {}", scratch.display());
}
