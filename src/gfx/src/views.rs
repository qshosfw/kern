//! Layer 4 — Stateful Interactive Monochrome Views, Motion & Navigation Controllers.

pub mod animation;
pub mod chrome;
pub mod dialog;
pub mod grid;
pub mod input;
pub mod list;
pub mod repeater;
pub mod scene;
pub mod sheet;
pub mod tabs;

use crate::Canvas;
use kernel::event::Event;

pub use animation::{
    HoldProgress, KineticScroller, ListScrollMode, MarqueeScroller, SlideUpAnim, TransitionProgress,
};
pub use chrome::{StatusBar, StatusRightConfig, ToastBanner, ToastState};
pub use dialog::{DialogState, DialogView, TextLine, draw_text_screen};
pub use grid::{GridItem, GridState, GridTile, GridView};
pub use input::{Charset, LocaleKeymap, TextPromptState, TextPromptView};
pub use list::{
    ChangeDir, ListItem, ListItemKind, ListModel, ListModelMut, ListNavDir, ListView, ShortString,
    get_edit_change_dir, get_list_nav_dir,
};
pub use repeater::KeyRepeater;
pub use scene::{ListAction, ListCursor, SceneFrame, SceneStack};
pub use sheet::{OptionSheet, SheetOptions, SheetResult, SheetView};
pub use tabs::{TabConfig, TabState, TabView};

/// Core View trait decoupling screen rendering and input dispatch from state models.
pub trait View<M> {
    fn render(&self, canvas: &mut Canvas, model: &M);
    fn on_event(&mut self, model: &mut M, event: &Event) -> bool;
}
