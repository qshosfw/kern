# DeltaFW (`uv-k1-k5v3-firmware-custom`) Architectural Analysis

An analysis of the user's `uv-k1-k5v3-firmware-custom` repository (copied to `inspo/deltafw`), which serves as the foundational reference for `kern`'s architecture.

## 1. The Hermes OSI Stack

The most critical component of this repository is the **Hermes Mesh & Messaging** implementation (`src/apps/hermes`). Unlike GoguFW, which crams all RF logic into a single 1,600-line `messenger_rf.c` file, DeltaFW implements a true, decoupled OSI networking model:
- **Physical (`physical/phy.c`)**: Manages the BK4819 FSK modem, handling raw FIFO interrupts and CSMA (Carrier Sense Multiple Access) logic to prevent packet collisions.
- **Data Link (`datalink/framing.c`)**: Handles sync words, FEC (Forward Error Correction), and packing payloads into OTA frames.
- **Network & Routing (`network/routing.c`)**: Manages 32-bit Base40 Node IDs, Hop limits (TTL), and mesh topology packet rebroadcasting (Implicit ACKs).
- **Transport (`transport/arq.c`)**: Handles Automatic Repeat reQuests (ACKs) and SNR-weighted backoff delays natively.
- **Security (`security/seal.c` & `kdf.c`)**: Natively implements ChaCha20-Poly1305 AEAD encryption on the wire, deriving isolated Network and Traffic keys using HKDF.

### How `kern` Builds on This:
`kern` will perfectly port this OSI stack into Rust, but with a **Zero-Copy Pipeline**. In the C implementation (`hermes.c`), the CPU constantly copies memory around (e.g., `memcpy(m->payload, block.payload)`, `memcpy(rx_buffer, ...)`). In `kern`, the DMA will write bytes into an `mbuf` block, and the network layers will simply pass references (`&[u8]`) between the PHY, MAC, and App layers.

## 2. Decoupled UI & Input Primitives

DeltaFW separates application logic from UI drawing:
- **`ui/ag_menu.c`**: An advanced menu routing system.
- **`ui/textinput.c`**: The optimized T9 text input UI we analyzed earlier.
- In `hermes.c` `HERMES_ProcessKeys()`, keystrokes are cleanly routed to the active UI state (e.g., `TextInput_HandleInput()` or `AG_MENU_HandleInput()`) rather than executing hardcoded logic.

### How `kern` Builds on This:
`kern` formalizes this using the `App` trait. In `kern`, the UI framework is entirely agnostic. Apps yield rendering instructions or subscribe to hardware `Events` (like `Event::KeyReleased(Key::Num2)`), keeping the application logic 100% platform independent.

## 3. The Flawed Physical Layer (FSK Modem)

While the high-level OSI architecture of DeltaFW's Hermes is conceptually excellent, its actual C implementation of the FSK Physical layer (`HERMES_HandleFSKInterrupt`) is highly unreliable and buggy.
- The attempt to use ISR-driven logic (`BK4819_REG_02_FSK_RX_SYNC`) in the constraints of a monolithic C polling architecture resulted in severe race conditions, dropped frames, and unstable CSMA backoffs.

### How `kern` Builds on This:
Because the C implementation of DeltaFW's Hermes modem interaction is fundamentally broken, **`kern` will discard it entirely**. Instead, we will build the FSK physical layer completely from scratch in Rust. 
To achieve stability, we will draw inspiration from how **GoguFW** interacts with the FSK modem (e.g., its robust FIFO polling and `NarrowLock` mechanisms) while avoiding GoguFW's monolithic spaghetti code. By combining GoguFW's reliable modem state-machine strategies with DeltaFW's brilliant high-level OSI architecture, `kern` will achieve a perfect, zero-copy, highly stable network stack.

## 4. Hardware Telemetry & Native USB Stack

Beyond networking, DeltaFW acts as a mature embedded operating system, managing the MCU's peripherals far better than stock firmwares.
- **Native USB CDC (`usb/usbd_cdc_if.c`)**: DeltaFW implements a full native USB Virtual COM Port stack. Rather than using static device descriptors, it dynamically injects the radio's unique hardware MAC and Crockford-encoded serial number directly into the USB String Descriptors at boot.
- **SysInfo & Telemetry (`apps/sysinfo/sysinfo.c`)**: A dedicated system information app provides deep hardware introspection, rendering battery voltage, internal chip temperature, active RAM usage (e.g., `14016/16K`), Master Key Hashes, and Git commit hashes. It even implements an interactive `HexDump_Render` view allowing the user to scroll through the raw bytes of the CPU's unique identifier.

### How `kern` Builds on This:
`kern` will adopt DeltaFW's dynamic USB initialization and robust hardware telemetry. In `kern`, this telemetry will be exposed as virtual files in the `/proc` filesystem (e.g., `cat /proc/meminfo` or `cat /proc/cpuinfo`), allowing any app or external Rigwire client to query the radio's exact hardware state using standard POSIX-like file reads.

## 5. App Ecosystem & Security Enclave

DeltaFW completely abandons the traditional "push a hardware button to change a hardcoded mode" paradigm in favor of a smartphone-like application ecosystem.
- **The Launcher (`apps/launcher/launcher.c`)**: Serves as the OS home screen. Using the `AG_MENU` UI framework, it routes the user into specific, decoupled apps (Spectrum, Scanner, Memories, Network, Info).
- **Table-Driven Settings (`apps/settings/settings_ui.c`)**: Instead of hardcoding 60 different menu screens, DeltaFW uses a beautiful `SettingConfig` struct table to dynamically generate deeply nested menus (Audio, Display, Radio, DTMF, System) that mutate variables and trigger side effects in real time.
- **Passcode & Crypto Auth (`apps/security/passcode.c`)**: Most impressively, DeltaFW implements a secure authentication enclave. It uses ChaCha20 as a Key Derivation Function (KDF), stretching the user's passcode over 8,192 iterations to derive a Key Encryption Key (KEK). It handles stealth lockouts, animated lockout countdowns, and dynamic hardware watchdog kicking to prevent brute-forcing.

### How `kern` Builds on This:
`kern` is explicitly designed to be the architectural successor to DeltaFW. `kern` will visually and functionally recreate DeltaFW's Launcher, Settings tree, and Security Enclave identically. However, where DeltaFW is bound by C-style global variables (e.g., `gEeprom.BACKLIGHT_MAX`), `kern` will persist these settings via the LittleFS Virtual Filesystem and route all UI interactions through safe, isolated Rust `App` traits.

## Summary

`uv-k1-k5v3-firmware-custom` (`deltafw`) provides a masterclass in high-level OS concepts, successfully modeling a full OSI networking stack, deep hardware telemetry, a smartphone-like app launcher, and a cryptographically secure authentication enclave on a $15 radio. While its low-level C implementation of the FSK modem is flawed and will be rewritten, `kern`'s goal is to be the direct successor to DeltaFW: marrying its brilliant high-level app ecosystem with the reliable modem interaction patterns of GoguFW, porting the best of both worlds into memory-safe, zero-copy Rust.
