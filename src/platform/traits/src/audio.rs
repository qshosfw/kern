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

/// Low-level MCU ADC/DAC analog voice frontend backing `AudioCaptureDriver`.
pub trait VoiceAdcBackend {
    /// Power up DC bias DAC + ADC and select the microphone input channel.
    fn begin_voice_bias(&mut self);
    /// Re-select the voice ADC channel (guards against battery telemetry sharing the ADC).
    fn select_voice_channel(&mut self);
    /// Perform one 12-bit ADC conversion on the active voice channel.
    fn read_voice_sample(&mut self) -> i32;
    /// Power down DC bias DAC and restore default ADC channel.
    fn end_voice_bias(&mut self);
    /// Non-blocking abort probe (e.g. keypad press during capture batch).
    fn should_abort(&self) -> bool;
}
