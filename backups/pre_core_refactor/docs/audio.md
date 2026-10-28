# Physical Layer (PHY) & Sub-Audible Telemetry Specification

`kern` does not hardcode heavy protocol stacks (such as AX.25 or POCSAG) directly into the core firmware image. Instead, `kern` provides a clean, modular set of **Physical Layer (PHY) and Link Layer driver primitives**. 

Applications, system services, or external co-processors can build higher-level protocols, digital voice codecs, or custom telemetry engines on top of these exposed PHY drivers.

---

## 1. BK4819 Physical Layer (PHY) Driver Stack

The Beken BK4819 transceiver provides low-level hardware primitives for RF modulation, sub-audible tone synthesis, FSK modem framing, and audio spectrum inversion. `kern` exposes these primitives via generic Rust driver traits (`FskPhy`, `SubAudiblePhy`, `ScramblerPhy`, `DtmfPhy`).

```
┌────────────────────────────────────────────────────────────────────────┐
│                   Layer 4: Apps / Optional Services                    │
│   (e.g., Custom Telemetry App, Dynamic Scrambler, Optional AX.25 Service)  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Exposes PHY Driver Traits
┌───────────────────────────────────▼────────────────────────────────────┐
│              Layer 2/3: PHY Drivers & Modulation Engine                │
│                                                                        │
│  ┌──────────────┐  ┌──────────────────┐  ┌──────────────┐  ┌─────────┐ │
│  │ FskPhy       │  │ SubAudiblePhy    │  │ ScramblerPhy │  │ DtmfPhy │ │
│  │ (FSK / FFSK) │  │ (CTCSS / DCS)    │  │ (Frequency   │  │ (DTMF   │ │
│  │ RAW PHY FIFO │  │ Tone / Phase Key │  │ Inversion)   │  │ Tones)  │ │
│  └──────────────┘  └──────────────────┘  └──────────────┘  └─────────┘ │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ SPI Register Commands
┌───────────────────────────────────▼────────────────────────────────────┐
│                    BK4819 RF Transceiver Core (HW)                     │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Low-Level PHY Primitives

### 2.1. `FskPhy` — Raw FSK Modem Access

The `FskPhy` driver grants raw access to the BK4819 internal FSK/FFSK modem hardware (Register `0x58`):

```rust
pub trait FskPhy {
    /// Configure FSK mode (000: FSK 1.2K/2.4K, 001: FFSK Bell 202, 101: NOAA)
    fn set_fsk_mode(&mut self, mode: FskMode) -> Result<(), PhyError>;
    
    /// Write raw bitstream / bytes directly to the BK4819 hardware TX FIFO
    fn transmit_raw_bytes(&mut self, data: &[u8]) -> Result<(), PhyError>;
    
    /// Read raw received packet bytes from the BK4819 RX FIFO
    fn receive_raw_bytes(&mut self, buffer: &mut [u8]) -> Result<usize, PhyError>;
}
```

This raw interface allows applications to implement custom FSK protocols, APRS, M17 telemetry, or binary data transfers without firmware modifications.

### 2.2. `SubAudiblePhy` — CTCSS & DCS Control

Sub-audible tones (67.0 Hz to 254.1 Hz) and 23-bit DCS digital codewords operate below the audible voice band (300 Hz - 3000 Hz).

```rust
pub trait SubAudiblePhy {
    /// Generate a continuous CTCSS tone frequency (freq in 0.1 Hz increments)
    fn set_ctcss_tx(&mut self, freq_01hz: u16) -> Result<(), PhyError>;
    
    /// Set sub-audible DCS codeword
    fn set_dcs_tx(&mut self, codeword: u32) -> Result<(), PhyError>;
    
    /// Read active detected CTCSS/DCS sub-audible tone from RX decoder
    fn get_subaudible_rx(&mut self) -> Option<SubAudibleTone>;
}
```

---

## 3. Creative Physical Layer Features & Telemetry Tricks

By exposing raw PHY primitives, `kern` enables creative audio and signaling capabilities:

### 3.1. DVS (Dynamic Voice Scrambling) with DCS Subtone Sync

Standard radio voice scramblers use a fixed 2.6 kHz or 3.3 kHz inversion frequency, making them trivial to decode with SDR software. 

`kern` implements **DVS (Dynamic Voice Scrambling)**: The inversion frequency shifts (hops) dynamically 5-10 times per second according to a pseudo-random sequence seeded by the 32-byte `contact_secret`.

To keep the transmitter and receiver hopping at the exact same time without dropping audio, `kern` uses **DCS Subtone Sync**:

```
Transmitter Radio (DVS Active):
1. PRNG (seeded by `contact_secret`) calculates the next carrier inversion hop.
2. Modulates analog FM voice using the BK4819 scrambler register.
3. Continuously transmits a digital 23-bit DCS sub-audible stream containing the PRNG frame sequence counter (e.g. Frame #42).

Receiver Radio (kern):
1. `SubAudiblePhy` continuously decodes the 23-bit DCS stream below 300 Hz.
2. Extracts the PRNG sequence counter (Frame #42).
3. `ScramblerPhy` instantly updates the BK4819 descrambler inversion frequency to match Frame #42!
```

This creates an incredibly robust, dynamic frequency-hopping voice scrambler operating on analog FM with zero audio distortion, synchronized perfectly via the continuous sub-audible DCS digital stream.

### 3.2. FSK Telemetry (In-Band Signaling)

During analog voice transmission, the `FskPhy` driver can modulate high-speed FSK telemetry data (using audible 1200Hz/1800Hz tones) just before or after the voice payload:
- **Sender ID / GPS Transmission**: Embeds Hermes node tags or coordinates in fast FSK bursts when the user presses or releases PTT.
- **Data Bursts**: Transmits FSK telemetry frames to open repeaters or trigger remote equipment.

### 3.3. Dual-Tone & DTMF Keying Engine (`DtmfPhy`)

The `DtmfPhy` driver provides raw dual-tone generation and decoding:
- Fast DTMF sequence bursts for selective calling (SelCall), PTT ID, or remote station telemetry.
- Programmable tone pair synthesis for custom acoustic modems.
