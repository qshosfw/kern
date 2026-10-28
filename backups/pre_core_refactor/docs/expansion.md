# Hardware Mods, Expansion Port & Remote Compute Specification

`kern` is designed to support hardware modifications, secondary receiver chips, and high-speed serial compute offloading to external co-processors (such as ESP32, Raspberry Pi, or host PCs).

---

## 1. Si4732 / Si4735 HF Receiver Hardware Mod (`dev/si473x.rs`)

A popular hardware mod for Quansheng radios is soldering a Silicon Labs **Si4732 / Si4735** broad-band receiver chip onto the internal I2C bus.

`kern` includes a dedicated, feature-gated driver (`--features si473x`) to manage this secondary receiver IC, unlocking Shortwave (SW), Medium Wave (MW), Longwave (LW), and Single Sideband (SSB / CW) reception across **150 kHz to 30 MHz**.

---

## 2. Si473x Architecture & SSB Patch Loading

```
               ┌──────────────────────────────────────────────┐
               │           Si4732 / Si4735 IC                 │
               │                                              │
               │  RF Ant ──► AM/FM/SW Front-End               │
               │                   │                          │
               │                   ▼                          │
               │           Internal DSP Core                  │
               │         (Requires SSB Patch RAM)             │
               │                   │                          │
               │                   ▼                          │
               │           Analog Audio Out ──► BK4819 Aux In │
               └───────────────────▲──────────────────────────┘
                                   │
                           I2C Bus (Bit-Bang / Hardware I2C)
                                   │
                                   ▼
                        ┌────────────────────┐
                        │  PY32F071 MCU      │
                        └────────────────────┘
```

### 2.1. SSB Patch Uploading Routine

To enable Single Sideband (LSB/USB) reception on Si473x chips, the driver must stream an official ~8 KB binary DSP patch over I2C into the chip's internal RAM during power-up:

```rust
pub fn init_ssb_mode(&mut self) -> Result<(), SiError> {
    self.power_down()?;
    self.power_up_patch_mode()?;
    
    // Stream patch data blocks over I2C
    for chunk in SSB_PATCH_DATA.chunks(16) {
        self.i2c_write_cmd(CMD_PATCH_DATA, chunk)?;
    }
    
    self.set_property(PROP_SSB_MODE, 0x0001)?; // Enable SSB DSP engine
    Ok(())
}
```

---

## 3. External Serial Expansion Port & Remote Compute Bridge

For advanced applications—such as high-resolution FFT waterfall displays, digital voice codecs (FreeDV / Codec2), or automated packet routing—the PY32F071 can offload compute tasks to an external co-processor over the UART / USB expansion interface.

```
┌────────────────────────────────┐                 ┌──────────────────────────────┐
│       Quansheng Radio          │                 │ External Compute Unit        │
│   (kern microkernel)           │                 │ (ESP32-S3 / Raspberry Pi)    │
│                                │   KWP Serial    │                              │
│ - Transceiver Hardware Control │ ──────────────► │ - DSP Heavy Lifting (FFT)    │
│ - Physical Keypad & Display    │   High-Speed    │ - Complex UI / Waterfalls    │
│ - Fast RSSI / Digital Demod    │ ◄────────────── │ - Codec2 Digital Voice Codec │
└────────────────────────────────┘                 └──────────────────────────────┘
```

### 3.1. Remote Compute Protocols

1. **FFT Streaming**: Radio streams raw 128-point RSSI sweep arrays to the external unit via KWP opcode `0x0301`. External unit computes high-resolution waterfall display and sends rendered frame metadata back for local screen display.
2. **Digital Voice Bridge**: BK4819 bypass mode sends raw discriminator audio to external ESP32 ADC. ESP32 decodes Codec2 digital voice and sends control commands back to `kern` via KWP.
3. **GPS NMEA Bridge**: External GPS module streams `$GPRMC` NMEA sentences into `kern` via expansion UART. Radio embeds live GPS coordinates into outgoing APRS position reports.
