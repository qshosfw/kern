# UART Protocol Analysis: GoguFW vs. `kern`

The user's PC communicates with the radio over a UART/USB interface to program settings, backup EEPROM, and control the radio (often called "CAT control"). GoguFW implements the legacy Baofeng/Quansheng UART protocol, whereas `kern` will implement a modern, zero-copy, highly secure architecture.

## 1. Framing and Parsing

### GoguFW's Implementation (`uart.c`)
GoguFW uses a cumbersome framing method based on magic bytes. A packet must start with `0xAB 0xCD`, followed by a 16-bit length, and end with `0xDC 0xBA`.
Because UART is a continuous stream, `UART_IsCommandAvailable()` manually scans the DMA ring buffer byte-by-byte in a `while` loop, searching for `0xAB`. When it finds it, it calculates the offsets, copies the payload into a secondary buffer, XORs the data, and checks the CRC-16. This approach is:
- **CPU Intensive**: Scanning a ring buffer in the main loop consumes valuable cycles.
- **Error Prone**: Handling split frames across the end of a circular DMA buffer requires complex index arithmetic (which `uart.c` spends ~140 lines doing).

### `kern`'s Architecture (Rigwire + COBS)
`kern` abandons magic byte framing and instead uses **COBS (Consistent Overhead Byte Stuffing)**.
COBS guarantees that the byte `0x00` *never* appears in the payload. Therefore, `0x00` is solely used as the frame delimiter. 
- **Zero-Copy Parsing**: The hardware DMA simply writes incoming bytes into memory. The moment it writes a `0x00`, the DMA interrupt fires, signaling a complete frame is ready. 
- **O(1) Delimiting**: The CPU does no scanning or searching. It simply passes the pointer of the DMA buffer directly to the COBS decoder, instantly yielding the payload.

## 2. Command Dispatching & Modularity

### GoguFW's Implementation
`UART_HandleCommand()` uses a monolithic switch statement to process commands like `0x0514` (Init) or `0x051D` (Write EEPROM). Inside these handlers, the UART module directly mutates global radio state (e.g., `gIsLocked = bIsLocked`, `gFmRadioCountdown_500ms = ...`, `BACKLIGHT_TurnOff()`). 
This tightly couples the UART driver to the FM Radio app, the UI, and the EEPROM, violating modularity.

### `kern`'s Architecture
`kern` uses an **Event-Driven Kernel Dispatcher**. 
When a COBS frame is decoded, the Rigwire driver simply emits an OS-level event: `Event::UartPacket(Packet)`.
- The `Kernel` or the currently active `App` receives the event.
- If the packet is a memory write command, the `SettingsApp` handles it and safely commits it to LittleFS.
- The UART driver has absolutely zero knowledge of the radio's state, backlight, or filesystem. It just passes pointers.

## 3. Security & Obfuscation

### GoguFW's Implementation
GoguFW attempts to secure the UART bus using "Obfuscation". It iterates over the payload and XORs it against a hardcoded 16-byte array:
```c
static const uint8_t Obfuscation[16] = { 0x16, 0x6C, 0x14... };
for (i = 0; i < Size; i++) pBytes[i] ^= Obfuscation[i % 16];
```
This is security by obscurity. Anyone listening to the serial port can trivially decode the EEPROM contents or spoof commands to unlock the radio.

### `kern`'s Architecture
`kern` utilizes the exact same security primitive for UART as it does for OTA FSK: **ChaCha20-Poly1305**.
Instead of hardcoded XOR arrays, the PC authenticates via a cryptographic handshake. All configuration commands and filesystem transfers over UART are fully AEAD-encrypted. A malicious agent cannot read the radio's memory or inject commands without the cryptographic keys, ensuring enterprise-grade device security.

## Summary

By replacing the legacy `AB CD` byte-scanning protocol with **COBS**, removing monolithic switch statements in favor of **Event Dispatching**, and replacing XOR arrays with **ChaCha20**, `kern` achieves a faster, safer, and infinitely more modular UART pipeline (known internally as "Rigwire").
