//! Scene & SceneManager navigation abstractions inspired by Flipper Zero GUI framework.

use crate::Canvas;
use kernel::event::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneId(pub u8);

/// A Scene represents a single application view state (e.g. Start Grid, Settings, Detail).
pub trait Scene<M> {
    /// Called when navigating into this Scene.
    fn on_enter(&mut self, _model: &mut M) {}

    /// Process incoming events and optionally request a transition to a new SceneId.
    fn on_event(&mut self, model: &mut M, event: &Event) -> Option<SceneId>;

    /// Render the Scene onto the Canvas.
    fn render(&self, canvas: &mut Canvas, model: &M);

    /// Called when navigating away from this Scene.
    fn on_exit(&mut self, _model: &mut M) {}
}
