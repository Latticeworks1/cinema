//! The setup/edit/run API: open a packet, inspect or edit its resolved `direction` config, and
//! preview or export a frame — all without touching Rust source or recompiling.
//!
//! `state`/`set` sit directly on `direct::Plan::from_packet`/`packet::Packet`'s existing
//! resolution path (base `vocab/direction.json` → packet `set.direction` overrides, merged,
//! then deserialized); they add no new resolution logic. `Plan` holds no open handles and isn't
//! `Clone`, so every method rebuilds it fresh from the packet rather than caching a stale copy.
//!
//! `render_flat_frame`/`render_lens_frame`/`export_flat`/`export_lens` all shell out to the
//! existing `cinema-flat`/`cinema-lens` CLI binaries rather than linking `cinema_flat`/
//! `cinema_lens` in-process: those two crates deliberately build against incompatible `fframes`
//! feature sets (`cinema-lens` needs `compile-time-svgtree`; `cinema-flat` cannot have it — see
//! the comment in `cinema_lens`'s own `Cargo.toml` and `tools/render.sh`), so a single crate
//! depending on both would force one unified, broken feature set on `fframes` for whichever one
//! it links. Going through the already-separately-built binaries sidesteps that entirely, at the
//! cost of a process spawn per call.

use direct::Plan;
use packet::{Packet, Section};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Project {
    packet: Packet,
}

impl Project {
    /// SETUP: loads the packet and confirms it resolves to a `Plan` before accepting it.
    pub fn open(packet_path: impl AsRef<Path>) -> Result<Project, String> {
        let packet = Packet::load(packet_path)?;
        Plan::from_packet(&packet)?;
        Ok(Project { packet })
    }

    fn plan(&self) -> Result<Plan, String> {
        Plan::from_packet(&self.packet)
    }

    /// INSPECT: the fully resolved `direction` config (base values, look, and every packet
    /// override already applied) as JSON.
    pub fn state(&self) -> Result<Value, String> {
        serde_json::to_value(&self.plan()?.dir).map_err(|e| e.to_string())
    }

    /// EDIT: sets one field of the resolved `direction` config at a dotted path (e.g.
    /// `"ink.flat_lock"`) and re-resolves the plan. Rejects a value that would make the
    /// direction config invalid (`direct::Direction::parse`'s own range checks) and leaves the
    /// project at its previous, valid state rather than half-applying the edit.
    pub fn set(&mut self, path: &str, value: Value) -> Result<(), String> {
        let before = self.packet.clone();
        self.packet.set_override(Section::Direction, path, value);
        if let Err(e) = self.plan() {
            self.packet = before;
            return Err(e);
        }
        Ok(())
    }

    /// PREVIEW: one frame of the flat pass (pass 1, no camera shader), as PNG bytes.
    pub fn render_flat_frame(&self, index: usize) -> Result<Vec<u8>, String> {
        self.render_frame("cinema-flat", index)
    }

    /// PREVIEW: one frame of the lens pass (pass 2, the camera shader), as PNG bytes. The lens
    /// pass reads the flat pass's already-rendered `pass/flat.mp4` as a texture; render the flat
    /// pass at least once (`export_flat`) before calling this, or it returns a named error
    /// instead of a silently black frame (the lens pass's own fallback for a missing texture).
    pub fn render_lens_frame(&self, index: usize) -> Result<Vec<u8>, String> {
        let flat = self.packet.flat_path();
        if !flat.exists() {
            return Err(format!(
                "{} does not exist yet — render the flat pass (Project::export_flat) before previewing lens frames",
                flat.display()
            ));
        }
        self.render_frame("cinema-lens", index)
    }

    /// RUN: full export of the flat pass, through the existing `cinema-flat render` CLI.
    pub fn export_flat(&self, range: Option<&str>, out: &Path) -> Result<(), String> {
        self.export("cinema-flat", range, out)
    }

    /// RUN: full export of the lens pass, through the existing `cinema-lens render` CLI.
    pub fn export_lens(&self, range: Option<&str>, out: &Path) -> Result<(), String> {
        self.export("cinema-lens", range, out)
    }

    /// A temporary packet that `extends` this project's own packet file and carries its current
    /// in-memory overrides — what every subprocess call below runs against, so edits made via
    /// `set` reach the CLI without being written back to the original packet file.
    fn write_temp_packet(&self, tag: &str) -> Result<PathBuf, String> {
        let tmp =
            std::env::temp_dir().join(format!("cinema-runtime-{}-{tag}-{}.json", self.packet.name, std::process::id()));
        let doc = serde_json::json!({
            "extends": self.packet.path.to_string_lossy(),
            "name": self.packet.name,
            "set": self.packet.all_overrides(),
        });
        std::fs::write(&tmp, serde_json::to_string(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(tmp)
    }

    fn render_frame(&self, bin: &str, index: usize) -> Result<Vec<u8>, String> {
        let tmp_packet = self.write_temp_packet(&format!("{bin}-frame"))?;
        let out_dir = std::env::temp_dir()
            .join(format!("cinema-runtime-{}-{bin}-frame-{index}-{}", self.packet.name, std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--release", "-p", bin, "--", "frame", &index.to_string(), "-o"]).arg(&out_dir);
        cmd.env("CINEMA_PACKET", &tmp_packet);
        let result = cmd.status();
        let _ = std::fs::remove_file(&tmp_packet);
        let status = result.map_err(|e| e.to_string())?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&out_dir);
            return Err(format!("{bin} frame exited with {status}"));
        }
        let png = std::fs::read_dir(&out_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|ext| ext == "png"))
            .ok_or_else(|| format!("{bin} frame produced no PNG in {}", out_dir.display()))?;
        let bytes = std::fs::read(&png).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&out_dir);
        Ok(bytes)
    }

    fn export(&self, bin: &str, range: Option<&str>, out: &Path) -> Result<(), String> {
        let tmp_packet = self.write_temp_packet(&format!("{bin}-export"))?;
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--release", "-p", bin, "--"]).arg("render");
        if let Some(r) = range {
            cmd.arg(r);
        }
        cmd.args(["-o", &out.to_string_lossy()]);
        cmd.env("CINEMA_PACKET", &tmp_packet);
        let result = cmd.status();
        let _ = std::fs::remove_file(&tmp_packet);
        let status = result.map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("{bin} render exited with {status}")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet_path() -> String {
        format!("{}/../../packets/hopeless.json", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn open_reports_the_known_defaults() {
        let p = Project::open(packet_path()).unwrap();
        let state = p.state().unwrap();
        assert_eq!(state["ink"]["flat_lock"], "#ff0000");
        assert_eq!(state["camera"]["focal"].as_f64().unwrap(), 1.7_f32 as f64, "f32 widened to f64, not the f64 literal");
    }

    #[test]
    fn set_reaches_one_field_and_leaves_its_siblings() {
        let mut p = Project::open(packet_path()).unwrap();
        p.set("ink.flat_lock", Value::String("#00ff00".into())).unwrap();
        let state = p.state().unwrap();
        assert_eq!(state["ink"]["flat_lock"], "#00ff00", "the field asked for");
        assert_eq!(state["ink"]["flat_lit"], "#e6e6e6", "an untouched sibling");
    }

    #[test]
    fn set_rejects_an_out_of_range_value_and_keeps_the_old_state() {
        let mut p = Project::open(packet_path()).unwrap();
        let err = p.set("fit", Value::from(5.0)).unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(p.state().unwrap()["fit"].as_f64().unwrap(), 0.92_f32 as f64, "rejected edit must not stick");
    }

    #[test]
    fn render_lens_frame_without_a_rendered_flat_pass_is_a_named_error() {
        let root = env!("CARGO_MANIFEST_DIR");
        let scratch = std::env::temp_dir().join(format!("cinema-runtime-test-nolens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let run_dir = scratch.join("out/hopeless");
        std::fs::create_dir_all(&run_dir).unwrap();
        // A real analysis.json (so Plan::from_packet resolves) but deliberately no pass/flat.mp4.
        std::fs::copy(format!("{root}/../../out/hopeless/analysis.json"), run_dir.join("analysis.json")).unwrap();
        let packet_json = format!(
            r#"{{"extends":"{root}/../../packets/hopeless.json","name":"hopeless","out":"{}"}}"#,
            scratch.join("out").display()
        );
        let path = scratch.join("no_flat.json");
        std::fs::write(&path, packet_json).unwrap();

        let p = Project::open(&path).unwrap();
        let err = p.render_lens_frame(0).unwrap_err();
        assert!(err.contains("flat.mp4"), "error should name the missing file, got: {err}");
        assert!(err.contains("export_flat"), "error should say what to do, got: {err}");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    // `render_flat_frame`/`render_lens_frame`/`export_flat`/`export_lens` spawn `cargo run
    // --release`, which takes real wall-clock time (a release build, then a render) — exercised
    // directly against the project in the verification pass below rather than as a `cargo test`
    // (keeping the default `cargo test --workspace` run fast), the same way milestone 1's own
    // render-based checks were run by hand, not as unit tests.
}
