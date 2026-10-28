# Filesystem Architecture & Zero-Overhead Storage (`fs/`)

`kern` operates under extreme memory constraints (16 KB SRAM, 118 KB Flash). To maintain low flash usage and avoid memory-heavy serialization libraries (like serde, JSON, or Protobuf), all data stored on LittleFS utilizes a **Zero-Overhead Flat Binary Architecture**.

Rather than serializing/deserializing structured data, `kern` uses `#[repr(C, packed)]` Rust structs and directly `memcpy`s them into file buffers on LittleFS.

---

## 1. POSIX-Style Flat Binary Namespace

The filesystem is organized into a clean, minimal directory structure containing flat arrays of tightly packed `structs`.

| Path | Description | Struct Mapping |
|---|---|---|
| `/sys/config.bin` | Global hardware settings (Squelch, Vol, Backlight, Boot mode) | `struct SystemConfig (64 Bytes)` |
| `/sys/keys.db` | Master cryptographic roots (Network Salt, Bootloader keys) | `struct KeyStore (32 Bytes)` |
| `/usr/contacts.db` | The Universal Contact database | Array of `UniversalContact (64 Bytes)` |
| `/usr/channels.db` | VFO memory channels (Freq, Offset, Bandwidth, Subtone) | Array of `Channel (32 Bytes)` |
| `/logs/flight.bin` | Persistent SRAM crash/panic recorder | `struct PanicContext (128 Bytes)` |

### 1.1. Zero-Overhead Read/Write Model

Because all structs are fixed-size and `#[repr(C, packed)]`, we can append, read, and index into LittleFS databases in $O(1)$ time without reading the entire file into SRAM.

```rust
// To read the 5th contact in the database:
let mut file = lfs.open("/usr/contacts.db", Mode::Read)?;
file.seek(SeekFrom::Start(5 * 64))?; // 5 * sizeof(UniversalContact)

let mut buffer = [0u8; 64];
file.read_exact(&mut buffer)?;

// Zero-copy cast into the strongly typed struct:
let contact: &UniversalContact = unsafe { core::mem::transmute(&buffer) };
```

This ensures that managing 1,000 contacts uses **exactly 0 bytes of dynamic heap memory**!

---

## 2. LittleFS Native Attributes (`lfs_attr`)

LittleFS supports custom inline metadata attributes for files. `kern` leverages these tags to apply security and compression transparently at the VFS layer, completely decoupling the logic from application code.

| Attribute ID | Tag | Purpose |
|---|---|---|
| `0x01` | `ATTR_ENCRYPTED` | File is transparently encrypted at rest using ChaCha20. |
| `0x02` | `ATTR_LZ4_COMPRESS`| File is transparently decompressed during `read()` streams. |
| `0x03` | `ATTR_SYSTEM_RO` | File is marked Read-Only. Prevents accidental app deletion. |

---

## 3. Protocol-Agnostic FW Features (Hermes Inspired)

By analyzing Hermes Link, `kern` integrates generalized cryptographic routing paradigms as core OS capabilities, abstracted away from specific protocols:

### 3.1. Dual-Layer (Inner/Outer) Cryptographic Pipeline
The FW exposes an abstract `CryptoSink` that natively supports layered encapsulation:
- **Outer Routing Layer**: Obfuscated with a public Network Salt / Hop Nonce (allowing mesh routers to see destinations and TTLs without exposing payloads).
- **Inner Payload Layer**: Encrypted strictly via the `contact_secret` using ChaCha20-Poly1305.

Any protocol (Rigwire, Sub-Audible, custom sideloaded apps) can push data through this OS-level `CryptoSink` rather than reinventing AEAD wrapping.

### 3.2. Universal Address Types
Identities are not strictly tied to "APRS" or "Hermes". Instead, `kern` classifies endpoints purely by their mathematical shape:
- `AddressType::Base40 (0x01)` — 6-Byte Integer (e.g. M17, Hermes)
- `AddressType::Ascii7 (0x02)` — 7-Byte ASCII String (e.g. AX.25 / APRS)
- `AddressType::Integer32 (0x03)` — 4-Byte Integer (e.g. DMR, IPv4)

This decoupling ensures `kern` remains a versatile, minimalist microkernel capable of executing any open digital mode smoothly.
