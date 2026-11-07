//! Milestone 3, step 1: exports a Clipkit (clipkit.dev) `Source` JSON whose only element is the
//! existing, unchanged full-song render (`out/p_stems/cinema.mp4`) as a full-canvas background
//! `video`. The real renderer's real pixels are used as-is; nothing about the shader, camera or
//! overlay pipeline is reimplemented here.
//!
//! Run with: cargo run --release -p cinema-runtime --example clipkit_export
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let packet_path = format!("{root}/examples/springtime/packets/p_stems.json");
    let background = PathBuf::from(format!("{root}/out/p_stems/cinema.mp4"));
    assert!(background.exists(), "expected an existing full render at {}", background.display());

    let plan = direct::Plan::from_packet(&packet::Packet::load(&packet_path).unwrap()).unwrap();
    let track = plan.track();
    let (width, height) = (1920, 1080);

    let elements = vec![json!({
        "id": "background",
        "type": "video",
        "layer": 1,
        "time": 0,
        "x": 0, "y": 0, "width": width, "height": height,
        "source": std::env::var("CLIPKIT_MEDIA_BASE").map(|base| format!("{base}/cinema.mp4")).unwrap_or_else(|_| background.canonicalize().unwrap().to_string_lossy().into_owned()),
    })];

    let source = json!({
        "clipkit_version": "1.0",
        "width": width,
        "height": height,
        "duration": track.duration_s,
        "frame_rate": track::FPS,
        "elements": elements,
    });

    let out_path = PathBuf::from(format!("{root}/out/p_stems/p_stems.clipkit.json"));
    std::fs::write(&out_path, serde_json::to_string_pretty(&source).unwrap()).unwrap();
    println!(
        "wrote {} ({} element: background video)",
        out_path.display(),
        source["elements"].as_array().unwrap().len()
    );
}
