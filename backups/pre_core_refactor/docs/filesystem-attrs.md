# Advanced Filesystem & Custom Metadata Specification (`fs/`)

`kern` leverages **LittleFS**, an open-source, power-fail-resilient, wear-leveling filesystem specifically optimized for microcontrollers interfacing with SPI Flash chips.

On the **Quansheng UV-K1 / K5 v3**, external non-volatile storage is provided by a **2 MB SPI NOR Flash chip (PY25Q16)**. There is no discrete external physical I2C EEPROM on the board; factory calibration is preserved at fixed sectors on the SPI Flash / MCU Flash, while **LittleFS manages the main 2 MB Flash volume**.

---

## 1. SPI Flash Partition Table & Calibration Coexistence

The 2 MB (2,097,152 bytes) SPI Flash chip (`PY25Q16`) is organized into 512 sectors of 4 KB each:

```
Address Range       Size         Partition Name     Purpose
0x000000 - 0x001FFF   8 KB       PART_FACTORY_CAL   Quansheng Factory Calibration Data (Preserved)
0x002000 - 0x00FFFF  56 KB       PART_SYS_CONFIG    kern Core System Settings, Security Keys & Crash Dump
0x010000 - 0x1FFFFF   1.937 MB   PART_LITTLEFS      LittleFS Volume Root (496 Blocks @ 4KB Block Size)
```

By placing LittleFS from `0x010000` to `0x1FFFFF` (1.937 MB), `kern` preserves original factory RF calibration data at `0x000000`–`0x001FFF`, preventing radio performance degradation while giving LittleFS **maximum usable disk space (1.937 MB)**.

---

## 2. Maximizing LittleFS Capabilities

To get the absolute maximum utility out of the 1.937 MB LittleFS volume on a resource-constrained radio, `kern` implements an advanced file management framework:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   LittleFS Virtual Filesystem Structure                │
├─────────────────┬─────────────────┬──────────────────┬─────────────────┤
│ /sys/           │ /channels/      │ /apps/           │ /logs/          │
│ - config.bin    │ - main.db       │ - scanner.elf    │ - flight.log    │
│ - calib.hmap    │ - repeaters.db  │ - waterfall.elf  │ - kwp_audit.log │
│ - rdp_keys.bin  │                 │ - APRS_tnc.elf   │                 │
└─────────────────┴─────────────────┴──────────────────┴─────────────────┤
│                 Managed via LittleFS 32-bit Custom Attributes (`lfs_attr`)│
└────────────────────────────────────────────────────────────────────────┘
```

### 2.1. Metadata Tag Catalog (`lfs_attr`)

LittleFS custom attributes attach lightweight metadata to any file without modifying file content:

| Tag ID | Name | Data Format | Purpose |
|---|---|---|---|
| `0x01` | `ATTR_FILE_FLAGS` | `u8` Bitfield | `0x01` = LZ4 Compressed<br>`0x02` = ChaCha20 Encrypted<br>`0x04` = Read-Only System File<br>`0x08` = Executable App Manifest |
| `0x02` | `ATTR_COMP_TYPE` | `u8` Enum | `0x01` = LZ4 Streaming<br>`0x02` = Heatshrink (LZSS)<br>`0x03` = Run-Length Encoding (RLE) |
| `0x03` | `ATTR_CRYPTO_NONCE` | `[u8; 12]` | 12-byte initialization vector for encrypted files |
| `0x04` | `ATTR_APP_MANIFEST` | `AppManifest` Struct | Sideloaded app permissions, RAM footprint, entry point offset |
| `0x05` | `ATTR_CRC32` | `u32` | File payload integrity checksum |

---

## 3. Transparent File Compression & Encryption

### 3.1. LZ4 Streaming Decompressor

To save space on SPI Flash, text files, custom font bitmaps, and channel lists marked with `ATTR_FILE_FLAGS = 0x01` are compressed using LZ4. 

The `fs` service layer transparently decompresses data chunks into a 256-byte SRAM ring buffer during reads:

```rust
pub struct CompressedFile<'a> {
    file: lfs::File<'a>,
    cache_buffer: [u8; 256],
    comp_type: CompressionType,
}

impl<'a> CompressedFile<'a> {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        // Reads compressed blocks from SPI Flash, streams decompressed output
    }
}
```

This effectively increases usable filesystem capacity from **1.937 MB to over 3.0 MB equivalent storage** for compressible text and channel databases.

### 3.2. Encrypted Secure Files

For privacy-sensitive logs or encryption keys, files tagged `ATTR_FILE_FLAGS = 0x02` are transparently decrypted using ChaCha20 with the nonce stored in `ATTR_CRYPTO_NONCE`.
