pub mod config;
pub mod detector;
pub mod fretboard;
pub mod license;
pub mod overrides;
pub mod patterns;
pub mod spelling;
pub mod voicing;

pub use detector::ChordDetector;
pub use voicing::{Voicing, VoicingSession, Weights};
#[cfg(feature = "learning")]
pub use detector::TrainOutcome;
pub use overrides::{OverrideInfo, OverrideStore};
pub use spelling::ChordSpelling;
