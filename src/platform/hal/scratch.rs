#![no_std]
use py32f071_pac as pac;
pub fn check() {
    let pwr = pac::raw::PWR::PTR;
    let exti = pac::raw::EXTI::PTR;
}
