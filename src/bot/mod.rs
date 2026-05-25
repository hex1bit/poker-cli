//! AI Bot 模块。

pub mod decision;
pub mod mood;
pub mod names;
pub mod personality;
pub mod voice;

pub use decision::decide;
pub use mood::Mood;
pub use personality::Personality;
pub use voice::{VoiceEvent, pick_line};
