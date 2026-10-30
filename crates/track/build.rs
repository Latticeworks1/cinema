use std::{env, fs, path::PathBuf};

fn main() {
    let path = "../../config/output.json";
    println!("cargo:rerun-if-changed={path}");
    let c: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    // The existing scene and schema support this profile. Reject unsupported edits rather
    // than silently stretching artwork or misreading schema-2 analysis.
    for (key, expected) in [("version", 1), ("fps", 30), ("logical_width", 1920),
        ("logical_height", 1080), ("output_width", 1920), ("output_height", 1080), ("flat_scale", 2)] {
        assert_eq!(c[key].as_u64(), Some(expected), "unsupported output contract: {key}");
    }
    let mut out = String::new();
    for key in ["fps", "logical_width", "logical_height", "output_width", "output_height", "flat_scale"] {
        out += &format!("pub const {}: usize = {};\n", key.to_uppercase(), c[key]);
    }
    out += "pub const FLAT_WIDTH: usize = LOGICAL_WIDTH * FLAT_SCALE;\npub const FLAT_HEIGHT: usize = LOGICAL_HEIGHT * FLAT_SCALE;\n";
    for pass in ["flat", "lens"] {
        let e = &c[format!("{pass}_encoder")];
        for key in ["codec", "crf", "preset", "tune"] {
            assert!(e[key].as_str().is_some(), "encoder {key} must be a string");
        }
        out += &format!("pub const {}_CODEC: &str = {};\n", pass.to_uppercase(), e["codec"]);
        out += &format!("pub const {}_PARAMS: &[(&str, &str)] = &[(\"crf\", {}), (\"preset\", {}), (\"tune\", {})];\n", pass.to_uppercase(), e["crf"], e["preset"], e["tune"]);
    }
    fs::write(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("output.rs"), out).unwrap();
}
