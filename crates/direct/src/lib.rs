//! Directing: turn a track into scenes, cameras and looks that are all derived from it.
//!
//! The pipeline is `track` (what the recording contains) -> `signals` (what each stem is doing
//! and how the song feels) -> this crate (which vocabulary entries fit, how the camera moves,
//! which parameters follow which signals) -> [`Plan::frame`], the state both render passes draw.

pub mod camera;
pub mod geometry;
pub mod layout;
pub mod matrix;
pub mod plan;
pub mod scenes;
pub mod settings;
pub mod vocab;

pub use camera::{Camera, project};
pub use geometry::{H, Rect, W};
pub use layout::{Layout, Panel, Timeline};
pub use matrix::{Matrix, Param};
pub use plan::{FrameState, LookState, Plan, Region, Sources, register};
pub use scenes::{Direction, Scene};
pub use settings::{Attention, CameraLimits, Chrome, Marks, Priority, Shots, Stage, Tracker};
pub use vocab::{DisplayLayout, EmitKind, Slider, Vocabulary};
