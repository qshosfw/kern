# Unified Architecture & Protocol Overview

`kern` avoids protocol bloat by standardizing on a single, high-performance serial bus protocol (**Rigwire**) over USB CDC / UART, paired with a memory-aware **Node Identity Architecture** for over-the-air radio-to-radio interaction.

---

## 1. Unified Bus Protocol: Rigwire

All machine-to-machine (M2M) communication, host coprocessor bridging, fast RSSI spectrum streaming, and serial control run over **Rigwire**:

- **Transport**: USB CDC Serial / UART (115200 to 921600 bps).
- **Framing**: Zero-delimiter COBS (`0x00`) framing.
- **Multiplexing**: Single wire carries Control (`CH 0`), Telemetry (`CH 1`), Audio PCM (`CH 2`), and Kernel Debug (`CH 3`).
- **Security**: ChaCha20 encryption and Poly1305 MAC authentication tags.

*(See [`docs/rigwire.md`](docs/rigwire.md) for full wire format and header layout).*

---

## 2. Over-The-Air Node Interaction Overview

For over-the-air (OTA) radio-to-radio signaling, `kern` uses a compact 32-bit Node Tag derived from our **Hermes Node Identity System**:

- **Rigwire Wire Tag**: Embeds the 4-byte Compact Node Tag in COBS frames over UART.
- **Hermes FSK Node Burst**: Embeds the 4-byte Compact Node Tag in an ultra-fast FSK 1200bps packet at the start/end of a transmission (using 1200Hz/1800Hz audible tones).
- **Node Recognition**: Fellow radios look up the 4-byte tag in their SRAM cache or SPI Flash contact database to instantly identify the caller, verify cryptographic credentials, and apply custom scrambler keys.

*(See [`docs/node-identity.md`](docs/node-identity.md) for full memory-aware identity and contact database specification).*
