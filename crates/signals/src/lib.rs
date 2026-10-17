//! Everything a visual parameter may depend on, derived from the track and nothing else.
//!
//! The bus answers "what is this stem doing at time t" as numbers in 0..1. The feel model
//! answers "how angular and how energetic is this span, compared with the rest of the song".
//! Both are pure functions of the track, so any frame can be evaluated independently.

mod bus;
mod feel;
mod rhythm;
mod settings;

pub use bus::{Bus, MasterSignal, Source, SourceError, StemSignal};
pub use settings::{BusSettings, RhythmSettings, SignalSettings};
pub use rhythm::{MAX_OSCILLATORS, Oscillator, Rhythm, RhythmSignal};
pub use feel::{Axes, EdgeWeights, EnergyWeights, FeelModel, FeelSettings, Selection};
