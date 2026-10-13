# Visual DFU Bootloader Specification (`bootloader/`)

The `kern` bootloader is a standalone, lightweight, fail-safe binary occupying the first **10 KB of internal Flash** (`0x08000000` - `0x080027FF`).

Unlike conventional radio bootloaders that present a blank screen and a static LED, the `kern` bootloader features a **rich graphical user interface (GUI)** on the ST7565 LCD display, complete with custom boot animations, interactive flashing progress bars, error diagnostic screens, and multi-interface DFU (Device Firmware Update) capability.

---

## 1. Memory Map & Sector Layout

```
Internal Flash Layout (128 KB Total):
+---------------------------------------+ 0x08000000
| Bootloader Vector Table & Reset Entry | (0x08000000 - 0x080001FF)
| Bootloader Core Logic & Drivers       | (0x08000200 - 0x08001FFF)
| Bootloader UI Assets & Graphics       | (0x08002000 - 0x080027FF)
+---------------------------------------+ 0x08002800 (10 KB Mark)
| Application Vector Table Relocation   | Main Firmware Start
| Main Application Binary (118 KB)      | (0x08002800 - 0x0801FFFF)
+---------------------------------------+ 0x08020000 (End of Flash)
```

---

## 2. Graphical Display Engine (`ui.rs`)

The bootloader embeds a stripped-down, ultra-compact ST7565 driver and bitmap renderer to display visual feedback during boot and update operations.

### 2.1. Visual States & Screens

1. **Normal Boot Splash Screen**:
   - Displays `kern` high-contrast vector logo.
   - Shows bootloader version (e.g., `v1.0.0-dfu`) and hardware revision (`PY32F071`).
   - If app CRC32 check passes, transitions to Application Entry Point (`0x08002800`) within 300ms.

2. **DFU Flashing Mode Screen**:
   ```
   ┌────────────────────────────────────┐
   │  ================================  │
   │           KERN DFU MODE            │
   │  ================================  │
   │                                    │
   │  FLASHING FIRMWARE...              │
   │  [████████████████░░░░░░░░] 64%    │
   │                                    │
   │  PORT: USB-C (DFU CLASS)           │
   │  BLOCK: 128 / 200                  │
   └────────────────────────────────────┘
   ```
   - Dynamic 128×64 page rendering using direct SPI writes.
   - Live progress percentage counter and animated progress bar.
   - Active transport status (`USB DFU` or `UART 115200 BAUD`).

3. **Recovery / Emergency Mode Screen**:
   - Displays a warning header (`! SYSTEM RECOVERY !`).
   - Shows fault reason: `INVALID APP CRC`, `SIGNATURE VERIFY FAIL`, or `USER RECOVERY KEY`.
   - Prompts user to connect serial cable or USB to flash valid firmware binary.

---

## 3. Boot Trigger & State Machine

```
                            ┌─────────────────────┐
                            │    Power On Reset   │
                            └──────────┬──────────┘
                                       │
                                       ▼
                            ┌─────────────────────┐
                            │ Hardware Init       │
                            │ (Clock 48MHz, GPIO) │
                            └──────────┬──────────┘
                                       │
                                       ▼
                    ┌─────────────────────────────────────┐
                    │ Check Key Combo (PTT + Side Key) OR │
                    │ Magic Word in SRAM (0xDEADBEEF)?    │
                    └──────────┬──────────────┬───────────┘
                               │              │
                     YES       │              │ NO
        ┌──────────────────────┘              └──────────────────────┐
        ▼                                                            ▼
┌──────────────────────┐                                  ┌──────────────────────┐
│ Enter Visual DFU     │                                  │ Validate Application │
│ Mode (USB/UART DFU)  │                                  │ CRC32 & Header Tag   │
└──────────┬───────────┘                                  └──────────┬───────────┘
           │                                                         │
           │ Flashing Complete                               PASS    │    FAIL
           ▼                                                         ▼     ▼
┌──────────────────────┐                                  ┌──────────────────────┐
│ Reboot System        │                                  │ Jump to Application  │
└──────────────────────┘                                  │ @ 0x08002800         │
                                                          └──────────────────────┘
```

---

## 4. DFU Transports & Protocol

The bootloader supports two concurrent firmware update transports:

### 4.1. Native USB DFU (Device Firmware Update Class)
- Device registers as standard USB DFU Class (`VID: 0x0483`, `PID: 0xDF11`).
- Standard `dfu-util` compatible protocol:
  - `DFU_DETACH`, `DFU_DNLOAD`, `DFU_UPLOAD`, `DFU_GETSTATUS`, `DFU_CLRSTATUS`, `DFU_ABORT`.
- Page write size: 128-byte pages matching PY32F071 internal Flash sector layout.

### 4.2. High-Speed Serial UART DFU Protocol
- UART1 (`115200` or `460800` baud) fallback interface.
- 4-byte Handshake Packet: `[0xAA, 0x55, 0xDF, 0x55]`.
- Packet Structure:
  ```
  ┌──────┬────────────┬────────────┬──────────────┬──────────┐
  │ SOF  │ ADDR (u32) │ LEN (u16)  │ PAYLOAD      │ CRC16    │
  │ 0x02 │ 0x08002800 │ 128 Bytes  │ Flash Page   │ CCITT    │
  └──────┴────────────┴────────────┴──────────────┴──────────┘
  ```

---

## 5. PY32 Readout Protection (RDP) & Security

The bootloader manages PY32 Option Bytes (`FLASH_OB`) to protect firmware integrity:

- **RDP Level 0 (Development)**: SWD debug interface active; full flash read/write access.
- **RDP Level 1 (Production Security)**: Option Bytes programmed to disable SWD Flash readback.
  - If an external SWD programmer attempts memory access, MCU hardware blocks read requests.
  - If RDP Level 1 is disabled back to Level 0 via SWD, MCU hardware automatically performs a **full mass erase** of all Flash sectors, preventing firmware or key extraction.
