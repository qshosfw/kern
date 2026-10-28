# Native Linux Desktop Emulator Specification (`sim/`)

A core requirement for `kern` is enabling developers to test, visually inspect, and debug firmware features on a standard Linux workstation **without requiring physical radio hardware or SWD flasher connections**.

The `sim` crate implements a full-featured desktop simulator using **SDL2**, compiling standard `kern` application, graphics, and protocol logic natively on `x86_64`.

---

## 1. Emulator Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Application Layer (app)                         │
│         Dual VFO  │  Spectrum Analyzer  │  Channel Scanner  │  Settings │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│             Layer 0.5: Platform Abstraction Traits (platform)          │
│   DisplayBackend │ RadioBackend │ StorageBackend │ InputBackend │ Serial │
└─────────────────┬──────────────────────────────────┬───────────────────┘
                  │                                  │
      (Target: thumbv6m)                             │ (Target: x86_64)
                  ▼                                  ▼
┌───────────────────────────────────┐    ┌───────────────────────────────┐
│ Real Physical Hardware (arch/dev) │    │ Desktop Simulator (sim)       │
│ - ST7565 SPI LCD                  │    │ - SDL2 Pixel Window (128x64)  │
│ - BK4819 RF Transceiver           │    │ - Synthetic RF RSSI Generator │
│ - PY25Q16 SPI Flash               │    │ - File-backed `flash.bin`     │
│ - Keypad Matrix GPIO              │    │ - Keyboard Event Pump         │
│ - Hardware USART                  │    │ - TCP Socket KWP Server       │
└───────────────────────────────────┘    └───────────────────────────────┘
```

---

## 2. Desktop Interface & User Inputs

When launched (`cargo run --bin sim`), the simulator opens a high-contrast desktop window rendering the radio's 128×64 LCD display scaled 4× with phosphor pixel simulation.

```
┌──────────────────────────────────────────────────┐
│  kern Desktop Radio Simulator v1.0 [x86_64]      │
│  ┌────────────────────────────────────────────┐  │
│  │ VFO A  145.500 MHz  FM                     │  │
│  │ VFO B  438.500 MHz  FM                     │  │
│  │ S████████████░░░░░  -72 dBm                │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  Keyboard Controls:                              │
│  0 - 9        : Keypad Numpad Input              │
│  M / ESC      : Menu Key                         │
│  F            : Function (F) Key                 │
│  Up / Down    : Frequency / Channel Selection    │
│  SPACE        : Key Radio PTT (TX Simulation)    │
│  Q / W        : Adjust Synthetic RSSI Level      │
└──────────────────────────────────────────────────┘
```

### 2.1. Physical Keymap Mapping

| Radio Key | Desktop Keyboard Binding |
|---|---|
| Keypad `0` - `9` | Keyboard Numpad `0` - `9` or Top Row `0` - `9` |
| `MENU` | `M` or `Escape` |
| `EXIT` | `Backspace` |
| `UP` / `DOWN` | Up Arrow / Down Arrow |
| `PTT` | `Spacebar` (Hold to transmit) |
| `F` (Function) | `F` |
| `SIDE 1` / `SIDE 2` | `S` / `D` |

---

## 3. Mock Subsystem Implementations

### 3.1. Display Backend (`sim/display.rs`)
- Implements `DisplayBackend` trait.
- Allocates an SDL2 texture representing 128×64 monochrome pixels.
- On `write_page()`, translates monochrome page bytes into RGBA pixels with customizable LCD color themes (Classic Amber, Blue Backlight, High-Contrast White).

### 3.2. Radio Backend (`sim/radio.rs`)
- Implements `RadioBackend` trait.
- Simulates BK4819 register state vector.
- Generates realistic synthetic RSSI noise floor with configurable signal peaks:
  ```rust
  pub fn get_rssi(&self) -> i8 {
      let noise: i8 = (rand::random::<u8>() % 6) as i8;
      if self.current_freq == 145_500_000 {
          -65 + noise // Simulated active signal on 145.500 MHz
      } else {
          -115 + noise // Background noise floor
      }
  }
  ```

### 3.3. Storage Backend (`sim/storage.rs`)
- Implements `StorageBackend` trait.
- Maps 2 MB SPI Flash read/write/erase operations to a local file named `flash.bin`.
- Preserves LittleFS volume contents, settings, and channel memory between desktop runs.

### 3.4. Serial & Protocol Backend (`sim/serial.rs`)
- Opens a TCP listening socket on port `7341` (`127.0.0.1:7341`).
- Host Python utilities (`tools/kwp_client.py`) can connect to `127.0.0.1:7341` and issue KWP binary packets directly to the simulator for automated testing.

---

## 4. Running the Simulator

```bash
# Launch emulator on Linux workstation
cargo run --bin sim

# Connect host Python KWP client to desktop emulator
python3 tools/kwp_client.py --port 7341 --ping
```
