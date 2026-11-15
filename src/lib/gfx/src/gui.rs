//! Flipper Zero inspired GUI Framework for microkernel applications.

pub mod animation;
pub mod modules;
pub mod repeater;
pub mod scene;
pub mod view;

pub use animation::{
    HoldProgress, KineticScroller, ListScrollMode, MarqueeScroller, SlideUpAnim, TransitionProgress,
};
pub use repeater::KeyRepeater;
pub use scene::{Scene, SceneId};
pub use view::View;
