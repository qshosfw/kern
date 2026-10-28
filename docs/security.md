# Security & Lightweight Cryptography Specification

Security in resource-constrained microcontroller systems requires realistic threat modeling, constant-time execution, and unified cryptographic primitives.

`kern` adopts **Ascon-128 (NIST SP 800-232)** as its single, unified cryptographic standard across the entire microkernel, bootloader, filesystem, and wire protocol.

---

## 1. Why Ascon-128 Wins on Cortex-M0+ and System Size

| Metric | SHA-256 / AES-GCM | ChaCha20 + Poly1305 + BLAKE2s | **Ascon-128 (kern standard - NIST SP 800-232)** |
|---|---|---|---|
| **Underlying Primitives** | 2 completely different structures | 2 distinct engines (stream + MAC) | **1 single 320-bit permutation sponge ($p$)** |
| **Multiplier Dependency** | Table lookups / GF mult | Poly1305 requires 64-bit mults (timing leak on M0+) | **Zero multiplications (only `EOR`, `AND`, `BIC`, `ROR`)** |
| **Constant-Time Security** | Cache/table timing vulnerabilities | Poly1305 software multiply timing leak risk | **Strictly constant-time across all silicon** |
| **Total Flash Footprint** | ~12 KB | ~2.8 KB total (ChaCha20 + Poly1305) | **~1.1 KB total suite (AEAD, Hash, XOF, MAC, KDF)** |
| **SRAM / Stack Usage** | High (256-byte stack arrays) | Multiple separate contexts | **40-byte compact state (0 heap alloc)** |
| **NIST Standardization** | Legacy NIST FIPS | RFC 8439 | **Official NIST Lightweight Standard (SP 800-232)** |

### One Engine vs Three
ChaCha20 (encryption), Poly1305 (MAC), and BLAKE2s/SHA (hashing) are three completely different mathematical structures. Putting all three into Cortex-M0+ requires compiling three distinct algorithms. Ascon is a sponge construction: Ascon-AEAD128, Ascon-Hash256, Ascon-XOF128, and Ascon-MAC all call the exact same 320-bit permutation function ($p$). The permutation is written once (~500 bytes of Thumb code), and Encryption, Decryption, Hashing, MAC, KDF, and TRNG are tiny loops around it.

### The Cortex-M0+ Multiply Trap in Poly1305
Poly1305 relies heavily on 32x32-bit multiplications modulo $2^{130}-5$. On Cortex-M0+ (which lacks the Cortex-M3/M4 1-cycle `UMULL` 64-bit multiply instruction), emulating 64-bit multiplication in Thumb-1 takes ~20 instructions per multiply, and if the silicon uses the 32-cycle multiplier option, Poly1305 leaks timing. Ascon uses **zero multiplications**—strictly bitwise `EOR`, `AND`, `BIC`, and `ROR`.

---

## 2. Universal Application Across `kern`

By using **Ascon-128** as our sole cryptographic engine, we achieve zero code duplication:

1. **Rigwire Serial Bus Auth (`Rigwire`)**:
   - Packets tagged with `SECURE` flag attach a 16-byte Ascon-128 MAC tag calculated over the header and payload.
   - Packets tagged with `ENCRYPTED` use Ascon-128 streaming encryption.

2. **LittleFS Filesystem Encryption (`fs/`)**:
   - Files marked with `ATTR_FILE_FLAGS = 0x02` are encrypted using Ascon-128 stream keystream with the 16-byte nonce.

3. **SPI Flash Calibration Integrity**:
   - Factory calibration and system settings stored on SPI Flash include a 16-byte Ascon-128 MAC tag in `/sys/calib.sig` to prevent tampering or corrupted reads.

4. **Security Enclave & Keychain (`/sys/device.key`, `/sys/passcode.key`, `/sys/storage.key`)**:
   - Ascon permutation sponge stretches user passcodes over 8,192 rounds to derive key encryption keys, with 16-byte nonces and MAC verification.

5. **Physical Hardware TRNG Seeding**:
   - TRNG incorporates Ascon permutation sponge with continuous NIST SP 800-90B Repetition Count Test (RCT) and forward-secrecy key erasure.

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
