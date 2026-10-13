//! Flipper Zero inspired GUI View Modules.

pub mod form;
pub mod grid_menu;
pub mod menu;
pub mod number_picker;
pub mod popup;
pub mod progress_modal;
pub mod text_input;
pub mod toast;

pub use form::{FormItem, FormItemKind, FormModel, FormView};
pub use grid_menu::{GridMenuItem, GridMenuModel, GridMenuView};
pub use menu::{
    MenuItem, MenuItemType, MenuModel, MenuView, OptionSheet, SheetOptions, SheetResult,
};
pub use number_picker::{NumberPickerModel, NumberPickerView};
pub use popup::{PopupModel, PopupView};
pub use progress_modal::{ProgressModalModel, ProgressModalView};
pub use text_input::{Charset, TextInputModel, TextInputView};
pub use toast::{ToastModel, ToastView};
