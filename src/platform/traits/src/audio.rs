//! Audio hardware abstraction traits.

/// Low-level Audio Power Amplifier (PA) Hardware Control.
pub trait AudioPa {
    fn enable(&mut self);
    fn disable(&mut self);
    fn is_enabled(&self) -> bool;
}

/// Continuous Audio Input & ADC Sampling Hardware Control.
pub trait AudioCapture {
    fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()>;
    fn read_samples(&mut self, out: &mut [i16]) -> usize;
    fn stop_capture(&mut self);
    fn is_capturing(&self) -> bool;
}
