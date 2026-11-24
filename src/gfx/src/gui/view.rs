//! View trait decoupling screen rendering and input dispatch from data models.

use crate::Canvas;
use kernel::event::Event;

/// Core View trait mapping a data Model M to screen rendering and event processing.
pub trait View<M> {
    /// Render the current state of Model M onto the Canvas.
    fn render(&self, canvas: &mut Canvas, model: &M);

    /// Handle incoming system/hardware event and update Model M accordingly.
    /// Returns true if the event was consumed.
    fn on_event(&mut self, model: &mut M, event: &Event) -> bool;
}
