# Security & Lightweight Cryptography Specification

Security in resource-constrained microcontroller systems requires realistic threat modeling and fast, native 32-bit cryptographic primitives.

`kern` adopts **ChaCha20-Poly1305 (RFC 8439)** as its single, unified cryptographic standard across the entire microkernel, bootloader, filesystem, and Rigwire wire protocol.

---

## 1. Why ChaCha20-Poly1305 is the Optimal Solution for ARM Cortex-M0+

| Metric | SHA-256 / AES-GCM | Ascon-128 | **ChaCha20-Poly1305 (kern standard)** |
|---|---|---|---|
| **Native Word Size** | 32-bit / Complex Tables | 64-bit (Slower on M0+) | **Pure 32-bit (`u32`)** |
| **CPU Cycle Efficiency** | Slow (Table lookups) | Moderate (Emulated 64-bit math) | **Fastest (Native 32-bit ARM `ADD/EOR/ROR`)** |
| **Flash Footprint** | ~12 KB | ~3.5 KB | **~1.8 KB total (`no_std`)** |
| **SRAM / Stack Usage** | High (256-byte stack arrays) | 40-byte state | **Tiny (32-byte state, 0 alloc)** |
| **Ecosystem Parity** | Universal | Emerging | **Universal (WireGuard, RFC 8439, Python, OpenSSL)** |

Because the ARM Cortex-M0+ is a native 32-bit processor, ChaCha20-Poly1305 executes significantly faster than 64-bit-oriented ciphers (Ascon) or hardware-heavy ciphers (AES without HW acceleration).

---

## 2. Universal Application Across `kern`

By using **ChaCha20-Poly1305** as our sole cryptographic engine, we achieve zero code duplication:

1. **Rigwire Serial Bus Auth (`Rigwire`)**:
   - Packets tagged with `SECURE` flag attach a 16-byte Poly1305 MAC tag calculated over the header and payload.
   - Packets tagged with `ENCRYPTED` use ChaCha20 streaming encryption.

2. **LittleFS Filesystem Encryption (`fs/`)**:
   - Files marked with `ATTR_FILE_FLAGS = 0x02` are encrypted using ChaCha20 with the 12-byte nonce stored in `ATTR_CRYPTO_NONCE`.

3. **SPI Flash Calibration Integrity**:
   - Factory calibration and system settings stored on SPI Flash include a 16-byte Poly1305 MAC tag to prevent tampering or corrupted reads.

4. **Firmware Binary Authenticator (`bootloader/`)**:
   - The Visual DFU bootloader validates new application binaries using Poly1305 MAC tags before jumping to main (`0x08002800`).

---

## 3. Physical & Option Byte Protection (RDP)

The PY32F071 MCU protects internal Flash memory via Option Bytes:

```
Option Byte Address: 0x1FFF0E00
RDP Byte Values:
  0xAA: Level 0 (Unprotected - Development Mode)
  0xBB: Level 1 (Memory Read Protection Active - Production Mode)
  0xCC: Level 2 (Chip Permanent Lock)
```

- **RDP Level 1** blocks SWD readback of firmware.
- If an attacker attempts to reset RDP Level 1 back to Level 0 via SWD, the PY32 MCU hardware **mass-erases all internal Flash**, destroying code and keys instantly.
