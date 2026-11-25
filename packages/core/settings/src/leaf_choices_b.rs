//! P0 Leaf 27: Choices Hydrator & Picker for Device, Apps, System, Security & Submenus (`25..59`).
#![no_std]
#![no_main]

mod choices_leaf;
mod schema;
mod shared;

define_choices_leaf!(25, 59, 11, 21, false);
