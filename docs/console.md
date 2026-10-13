# Industrial Text Console & Serial Interface Specification (`console`)

`kern` implements an **industrial-grade, plain-ASCII text serial interface** compliant with embedded telecom standards and test equipment interfaces (IEEE 488.2 / SCPI-inspired).

It provides clean, unadorned, 7-bit ASCII text output with zero ANSI color escapes, ensuring 100% compatibility with serial logging utilities, terminal software, automated scripts, and test benches.

---

## 1. Serial Port Parameters & Plug-In Identification

### 1.1. Default Bus Configuration
- **Baud Rate**: `115200` bps
- **Data Bits**: `8`
- **Parity**: `None`
- **Stop Bits**: `1` (`8N1`)
- **Flow Control**: None / Hardware RTS-CTS optional

### 1.2. Plug-in Identification Banner

Upon serial port connection (detecting DTR assertion or initial byte activity), `kern` emits an unadorned industrial identification line:

```text
kern v1.0.0 (PY32F071-ARM-CM0+; HW: UV-K1; SN: 4B45524E-0001) Ready [115200 8N1]
```

### 1.3. Universal SCPI Identification (`*IDN?`)

For automated test suites, Python scripts, LabVIEW, and PyVISA compatibility, `kern` responds to the IEEE 488.2 standard identification query:

```text
Input:  *IDN?
Output: KERN,UV-K1-PY32F071,SN-4B45524E0001,FW-1.0.0
```

---

## 2. Command Architecture & Prompt Format

The terminal prompt is a minimal 2-character string (`> `). Every response is terminated by clean CRLF (`\r\n`) and starts with a standard status code (`OK` or `ERR: ...`).

### 2.1. Industrial Command Catalog

| Command | Syntax | Example Response | Description |
|---|---|---|---|
| **`*IDN?`** | `*IDN?` | `KERN,UV-K1-PY32F071,SN-4B45524E0001,FW-1.0.0` | IEEE 488.2 device identification string |
| **`sys`** | `sys` | `OK SYS: 72MHz | SRAM: 4096/16384B | Vbat: 7.82V` | Microkernel status and power diagnostics |
| **`freq`** | `freq 145.500` | `OK FREQ: VFO_A = 145.500000 MHz` | Gets/sets active VFO frequency in MHz |
| **`mode`** | `mode fm` | `OK MODE: FM (Dev: 2.5kHz)` | Sets modulation mode (`fm`, `am`, `usb`, `lsb`, `fsk`) |
| **`power`** | `power 2.0` | `OK POWER: 2.0W (PA_BIAS: 0x4F)` | Sets RF PA power level in Watts |
| **`sq`** | `sq 3` | `OK SQUELCH: 3 (-102 dBm)` | Sets squelch threshold |
| **`rssi?`** | `rssi?` | `OK RSSI: -105 dBm (S3)` | Queries instantaneous RSSI and S-meter level |
| **`scan`** | `scan 144.0 146.0` | `OK SCAN: 145.500MHz RSSI=-68dBm` | Executes single spectrum sweep |
| **`fs`** | `fs ls` | `OK FS: config.bin (128B), calib.bin (512B)` | Filesystem directory listing |
| **`log`** | `log` | `OK LOG: 0 events stored. Flight recorder clean.` | Dumps persistent SRAM crash flight recorder |
| **`reboot`** | `reboot` | `OK REBOOTING` | Executes clean software reboot |

---

## 3. Standardized Error Response Codes

When an invalid command or out-of-range parameter is received, `kern` outputs deterministic ASCII error codes:

```text
> freq 9999.000
ERR: FREQ_OUT_OF_RANGE (Min: 18.000, Max: 1300.000 MHz)

> invalid_cmd
ERR: UNKNOWN_COMMAND (Type 'help' for command list)

> power 50.0
ERR: POWER_TOO_HIGH (Max: 5.0W)
```

---

## 4. Zero-Copy Parser Architecture

The console parser uses zero dynamic memory allocation and parses commands in-place:

```rust
pub fn parse_industrial_cmd(line: &str) -> ConsoleResponse {
    let mut parts = line.trim().split_whitespace();
    match parts.next() {
        Some("*IDN?") => ConsoleResponse::Ok("KERN,UV-K1-PY32F071,SN-4B45524E0001,FW-1.0.0"),
        Some("freq") => handle_freq_cmd(parts),
        Some("sys") => handle_sys_cmd(),
        Some("rssi?") => handle_rssi_query(),
        _ => ConsoleResponse::Err("UNKNOWN_COMMAND"),
    }
}
```
