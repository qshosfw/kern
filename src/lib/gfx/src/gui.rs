//! Flipper Zero inspired GUI Framework for microkernel applications.

pub mod animation;
pub mod modules;
pub mod repeater;
pub mod view;

pub use animation::{
    HoldProgress, KineticScroller, ListScrollMode, MarqueeScroller, SlideUpAnim, TransitionProgress,
};
pub use repeater::KeyRepeater;
pub use view::View;
