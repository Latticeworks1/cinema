//! Shared output profile, generated from config/output.json.
include!(concat!(env!("OUT_DIR"), "/output.rs"));

/// Frame timestamps are starts of half-open intervals [i/fps, (i+1)/fps).
pub fn seconds(frame: usize) -> f32 { frame as f32 / FPS as f32 }

/// Nearest frame, with positive half-frame ties rounded upward.
pub fn nearest_frame(seconds: f32) -> usize { (seconds * FPS as f32).round() as usize }

/// Analysis duration is stored as f32; quantize before rounding in every producer.
pub fn frame_count(duration: f32) -> usize { nearest_frame(duration) }

pub fn shader_constants() -> String {
    format!("const float FLAT_SCALE = {}.0;\nconst float LW = {}.0;\nconst float LH = {}.0;\nconst float OUTPUT_FPS = {}.0;\n", FLAT_SCALE, LOGICAL_WIDTH, LOGICAL_HEIGHT, FPS)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_boundaries() {
        assert_eq!(frame_count(0.15), 5);
        assert_eq!(frame_count(31.0), 930);
        assert_eq!(seconds(30), 1.0);
        assert_eq!(FLAT_WIDTH, LOGICAL_WIDTH * FLAT_SCALE);
    }
}
