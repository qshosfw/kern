//! P0 Leaf 26: Choices Hydrator & Picker for Display, Audio, Buttons & Battery (`0..25`).
#![no_std]
#![no_main]

mod choices_leaf;
mod schema;
mod shared;

define_choices_leaf!(0, 25, 0, 11, true);
