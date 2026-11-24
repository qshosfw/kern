#!/usr/bin/env python3
"""
Kern Firmware Container (.kfw v1.0) — Reference Tool & Library.

Zero-dependency Python 3 implementation of `docs/kfw-spec.md`:
  • 3 binary structures: KfwHeader (64 B), KfwPart (32 B), KfwTlv (4 B)
  • 100% Ascon cryptography (bit-exact with `src/lib/crypto/src/ascon.rs`):
      Ascon-Hash256, Ascon-AEAD128, Ascon-128-MAC, and PQ LMS-Ascon
  • Solid ARMTHUMB BCJ + LZMA2 compression (~41% universal multi-target size)
    and MCU-streamable None / PackBits RLE codecs
  • Full anti-brick validation: target matching, protected address fences
    (bootloader + 0x0800_2700 QS-1 page), ARMv6-M vector checks, KRX-ABI
    bounds, KernFS path-traversal guards, and sec_cnt anti-rollback.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import lzma
import os
import re
import secrets
import struct
import subprocess
import sys
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# =============================================================================
# 0. KERN TUI STYLE HELPERS (Shared cross-platform cli_ui)
# =============================================================================

_TOOLS_DIR = Path(__file__).resolve().parent
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))

import cli_ui as _cli_ui  # noqa: E402

_cli_ui.init_console()
_C = _cli_ui.ansi_map()
_RESET  = _C["RESET"]
_BOLD   = _C["BOLD"]
_INVERT = _C["INVERT"]
_RED    = _C["RED"]
_GREEN  = _C["GREEN"]
_YELLOW = _C["YELLOW"]
_BLUE   = _C["BLUE"]
_CYAN   = _C["CYAN"]
_GRAY   = _C["GRAY"]


def _log_info(msg: str) -> None: _cli_ui.info(msg)
def _log_ok(msg: str) -> None:   _cli_ui.ok(msg)
def _log_err(msg: str) -> None:  _cli_ui.err(msg)

# =============================================================================
# 1. FORMAT CONSTANTS & REGISTRY
# =============================================================================

FILE_ID_KFW = b"\x89KFW\r\n\x1a\n"
KFW_VERSION_1_0 = 0x10
KFW_HEADER_FMT = "<8sBBHIIIIHH32s"
KFW_PART_FMT = "<BBBBIII16s"
KFW_TLV_FMT = "<HH"

KFW_HEADER_SIZE = 64
KFW_PART_SIZE = 32
KFW_TLV_SIZE = 4

# Container kinds
KIND_FIRMWARE = 0x01
KIND_BUNDLE = 0x02
KIND_MODULE_PACK = 0x03
KIND_BACKUP = 0x04
KIND_RECOVERY = 0x05

KIND_NAMES = {
    KIND_FIRMWARE: "Firmware",
    KIND_BUNDLE: "Bundle",
    KIND_MODULE_PACK: "ModulePack",
    KIND_BACKUP: "Backup",
    KIND_RECOVERY: "Recovery",
}

# Header flags — low 16 advisory, high 16 critical
HFLAG_REQUIRE_CALIB_BACKUP = 1 << 0
HFLAG_PRESERVE_BOOTLOADER = 1 << 1
HFLAG_VERIFY_READBACK = 1 << 2
HFLAG_REPRODUCIBLE = 1 << 3
HFLAG_PRERELEASE = 1 << 4
HFLAG_ALLOW_DOWNGRADE = 1 << 5
HFLAG_SOLID = 1 << 6
HFLAG_MCU_OTA = 1 << 7

HCRIT_AUTH_REQUIRED = 1 << 16
HCRIT_ENCRYPTED = 1 << 17
HCRIT_DELTA = 1 << 18
HCRIT_IRREVERSIBLE_FUSE = 1 << 19
HCRIT_KNOWN_MASK = (
    HCRIT_AUTH_REQUIRED | HCRIT_ENCRYPTED | HCRIT_DELTA | HCRIT_IRREVERSIBLE_FUSE
)

# Part kinds (bit 7: 0 = Critical, 1 = Optional)
PART_SOLID_ARENA = 0x01
PART_FIRMWARE = 0x10
PART_BOOTLOADER = 0x11
PART_DELTA_PATCH = 0x12
PART_OPTION_BYTES = 0x13
PART_KRX_MODULE = 0x20
PART_KERNFS_FILE = 0x21
PART_CALIBRATION = 0x30
PART_MEMORY_DUMP = 0x31

PART_OPT_KRX = 0x80
PART_OPT_FILE = 0x81
PART_RELEASE_NOTES = 0x82
PART_DEBUG_SYMBOLS = 0x83

PART_KIND_NAMES = {
    PART_SOLID_ARENA: "SOLID_ARENA",
    PART_FIRMWARE: "FIRMWARE",
    PART_BOOTLOADER: "BOOTLOADER",
    PART_DELTA_PATCH: "DELTA_PATCH",
    PART_OPTION_BYTES: "OPTION_BYTES",
    PART_KRX_MODULE: "KRX_MODULE",
    PART_KERNFS_FILE: "KERNFS_FILE",
    PART_CALIBRATION: "CALIBRATION",
    PART_MEMORY_DUMP: "MEMORY_DUMP",
    PART_OPT_KRX: "OPT_KRX",
    PART_OPT_FILE: "OPT_FILE",
    PART_RELEASE_NOTES: "RELEASE_NOTES",
    PART_DEBUG_SYMBOLS: "DEBUG_SYMBOLS",
}
KNOWN_CRITICAL_PARTS = {k for k in PART_KIND_NAMES if (k & 0x80) == 0}

# Part flags
COMP_NONE = 0
COMP_PACKBITS_RLE = 1
COMP_HEATSHRINK = 2
COMP_DEFLATE = 3
COMP_LZMA2 = 4
COMP_ARMTHUMB_LZMA2 = 5
COMP_ZSTD = 6
COMP_ARMTHUMB_ZSTD = 7

COMP_NAMES = {
    COMP_NONE: "none",
    COMP_PACKBITS_RLE: "rle",
    COMP_HEATSHRINK: "heatshrink",
    COMP_DEFLATE: "deflate",
    COMP_LZMA2: "lzma2",
    COMP_ARMTHUMB_LZMA2: "armthumb+lzma2",
    COMP_ZSTD: "zstd",
    COMP_ARMTHUMB_ZSTD: "armthumb+zstd",
}

PFLAG_COMP_MASK = 0x07
PFLAG_ENCRYPTED = 1 << 3
PFLAG_IN_SOLID = 1 << 4
PFLAG_POST_FLASH = 1 << 5
PFLAG_PAGE_ALIGNED = 1 << 6
PFLAG_MCU_STREAMABLE = 1 << 7

# KernFS install policies
POLICY_ALWAYS = 0
POLICY_IF_MISSING = 1
POLICY_UPGRADE_ONLY = 2
POLICY_PRESERVE_USER = 3
POLICY_DELETE = 4
POLICY_VERIFY_ONLY = 5

POLICY_NAMES = {
    POLICY_ALWAYS: "always",
    POLICY_IF_MISSING: "if-missing",
    POLICY_UPGRADE_ONLY: "upgrade-only",
    POLICY_PRESERVE_USER: "preserve-user",
    POLICY_DELETE: "delete",
    POLICY_VERIFY_ONLY: "verify-only",
}
POLICY_BY_NAME = {v: k for k, v in POLICY_NAMES.items()}

# Protected TLV tags (bit 15: 1 = Critical, 0 = Optional)
TLV_MANIFEST = 0x0001
TLV_BUILD_STATS = 0x0002
TLV_DEPENDENCY = 0x0003

TLV_TARGET = 0x8001
TLV_BOOT_POLICY = 0x8002
TLV_PROTECT_RANGE = 0x8003
TLV_PART_ADDR = 0x8004
TLV_PART_PATH = 0x8005
TLV_SOLID_PARAMS = 0x8006
TLV_DELTA_BASE = 0x8007
TLV_DEVICE_ID = 0x8008

KNOWN_CRITICAL_TLVS = {
    TLV_TARGET,
    TLV_BOOT_POLICY,
    TLV_PROTECT_RANGE,
    TLV_PART_ADDR,
    TLV_PART_PATH,
    TLV_SOLID_PARAMS,
    TLV_DELTA_BASE,
    TLV_DEVICE_ID,
}

# Unprotected Auth Trailer TLVs
TLV_AUTH_ASCON_MAC = 0x0101
TLV_AUTH_LMS_ASCON = 0x0102

# Boot policy flags
BPFLAG_PRESERVE_FACTORY_PAGE = 1 << 0
BPFLAG_FORBID_RDP2 = 1 << 1
BPFLAG_ALLOW_ROLLBACK = 1 << 2
BPFLAG_REQUIRE_CLEAN_QUIESCE = 1 << 3
BPFLAG_SKIP_IF_DIGEST_MATCH = 1 << 4
BPFLAG_ERASE_TAIL_PAGES = 1 << 5

# Firmware address flags
FWFLAG_HAS_VECTORS = 1 << 0
FWFLAG_STAGE_FIRST = 1 << 1
FWFLAG_REQUIRES_REBOOT = 1 << 2

# Memory buses
BUS_INTERNAL_FLASH = 1
BUS_SPI_FLASH = 2
BUS_COPROC_FLASH = 3
BUS_EEPROM = 4
BUS_SYSTEM_MEM = 5

# Quansheng bootloader XOR-16 wire table
QS_OBFUS_TBL = bytes([
    0x16, 0x6C, 0x14, 0xE6, 0x2E, 0x91, 0x0D, 0x40,
    0x21, 0x35, 0xD5, 0x40, 0x13, 0x03, 0xE9, 0x80,
])

# Built-in hardware target profiles
BUILTIN_TARGETS: dict[str, dict[str, Any]] = {
    "uv-k1": {
        "slug": "uv-k1-k5v3",
        "target_id": 0x51530101,
        "mcu_id": 0x0001,       # PY32F071xB
        "arch_rf": (1 << 4) | 2,  # ARMv6-M | BK4829
        "transport_disp": (1 << 4) | 1,  # QS PY32 | ST7565
        "flash_base": 0x08002800,
        "flash_max": 118 * 1024,
        "page_size": 256,
        "sram_kb": 16,
        "hw_rev_mask": 0xFFFF,
        "ext_flash_kb": 2048,
        "boot_family": 1,       # Stock QS PY32 (7.x)
        "min_boot_ver": 0x00070000,
        "max_boot_ver": 0,
        "min_battery_mv": 7200,
        "usb_vid": 0x36B7,
        "usb_pid": 0x0001,
        "uart_baud": 38400,
        "bl_prefix": b"7.0\x00",
        "protect_ranges": [
            (BUS_INTERNAL_FLASH, 0x0001, 0x08000000, 0x08002700),  # Stock bootloader
            (BUS_INTERNAL_FLASH, 0x0003, 0x08002700, 0x08002800),  # QS-1 factory page (strict)
        ],
    },
    "uv-k10pro": {
        "slug": "uv-k10pro",
        "target_id": 0x51530102,
        "mcu_id": 0x0001,
        "arch_rf": (1 << 4) | 2,
        "transport_disp": (1 << 4) | 1,
        "flash_base": 0x08002800,
        "flash_max": 120 * 1024,
        "page_size": 256,
        "sram_kb": 16,
        "hw_rev_mask": 0xFFFF,
        "ext_flash_kb": 2048,
        "boot_family": 1,
        "min_boot_ver": 0x00070000,
        "max_boot_ver": 0,
        "min_battery_mv": 7200,
        "usb_vid": 0x36B7,
        "usb_pid": 0x0001,
        "uart_baud": 38400,
        "bl_prefix": b"7.0\x00",
        "protect_ranges": [
            (BUS_INTERNAL_FLASH, 0x0001, 0x08000000, 0x08002700),
            (BUS_INTERNAL_FLASH, 0x0003, 0x08002700, 0x08002800),
        ],
    },
}
BUILTIN_TARGETS["uv-k5v3"] = BUILTIN_TARGETS["uv-k1"]
BUILTIN_TARGETS["uv-k1-k5v3"] = BUILTIN_TARGETS["uv-k1"]
BUILTIN_TARGETS["k10pro"] = BUILTIN_TARGETS["uv-k10pro"]


class KfwError(Exception):
    """Raised on any `.kfw` format, cryptographic, or anti-brick verification failure."""


# =============================================================================
# 2. ASCON CRYPTOGRAPHIC ENGINE (Bit-Exact with `src/lib/crypto/src/ascon.rs`)
# =============================================================================

_M64 = 0xFFFFFFFFFFFFFFFF


def _rotr(val: int, r: int) -> int:
    return ((val >> r) | ((val << (64 - r)) & _M64)) & _M64


def _ascon_permute(s: list[int], rounds: int) -> None:
    x0, x1, x2, x3, x4 = s
    m = 0xFFFFFFFFFFFFFFFF
    for r in range(12 - rounds, 12):
        x2 ^= 0xF0 - r * 0x0F
        x0 ^= x4
        x4 ^= x3
        x2 ^= x1
        t0 = x0 ^ ((~x1 & m) & x2)
        t1 = x1 ^ ((~x2 & m) & x3)
        t2 = x2 ^ ((~x3 & m) & x4)
        t3 = x3 ^ ((~x4 & m) & x0)
        t4 = x4 ^ ((~x0 & m) & x1)
        x1 = t1 ^ t0
        x0 = t0 ^ t4
        x3 = t3 ^ t2
        x2 = (~t2) & m
        x4 = t4
        x0 ^= ((x0 >> 19) | ((x0 << 45) & m)) ^ ((x0 >> 28) | ((x0 << 36) & m))
        x1 ^= ((x1 >> 61) | ((x1 << 3) & m)) ^ ((x1 >> 39) | ((x1 << 25) & m))
        x2 ^= ((x2 >> 1) | ((x2 << 63) & m)) ^ ((x2 >> 6) | ((x2 << 58) & m))
        x3 ^= ((x3 >> 10) | ((x3 << 54) & m)) ^ ((x3 >> 17) | ((x3 << 47) & m))
        x4 ^= ((x4 >> 7) | ((x4 << 57) & m)) ^ ((x4 >> 41) | ((x4 << 23) & m))
    s[0], s[1], s[2], s[3], s[4] = x0, x1, x2, x3, x4


class AsconHash256:
    """Streaming 256-bit Ascon-Hash matching `src/lib/crypto/src/ascon.rs`."""

    IV = 0x00400C0000000100

    def __init__(self) -> None:
        self._s = [self.IV, 0, 0, 0, 0]
        _ascon_permute(self._s, 12)
        self._buf = bytearray()

    def update(self, data: bytes | bytearray | memoryview) -> None:
        mv = memoryview(data)
        if self._buf:
            need = 8 - len(self._buf)
            if len(mv) < need:
                self._buf.extend(mv)
                return
            self._buf.extend(mv[:need])
            self._s[0] ^= int.from_bytes(self._buf, "big")
            _ascon_permute(self._s, 12)
            self._buf.clear()
            mv = mv[need:]

        n_full = len(mv) // 8
        off = 0
        for _ in range(n_full):
            self._s[0] ^= int.from_bytes(mv[off : off + 8], "big")
            _ascon_permute(self._s, 12)
            off += 8
        if off < len(mv):
            self._buf.extend(mv[off:])

    def digest(self) -> bytes:
        s = list(self._s)
        pad = bytearray(8)
        pad[: len(self._buf)] = self._buf
        pad[len(self._buf)] = 0x80
        s[0] ^= int.from_bytes(pad, "big")
        _ascon_permute(s, 12)

        out = bytearray(32)
        out[0:8] = s[0].to_bytes(8, "big")
        _ascon_permute(s, 12)
        out[8:16] = s[0].to_bytes(8, "big")
        _ascon_permute(s, 12)
        out[16:24] = s[0].to_bytes(8, "big")
        _ascon_permute(s, 12)
        out[24:32] = s[0].to_bytes(8, "big")
        return bytes(out)


def ascon_hash256(data: bytes | bytearray | memoryview) -> bytes:
    h = AsconHash256()
    h.update(data)
    return h.digest()


def _ascon128_init(key: bytes, nonce: bytes) -> tuple[list[int], int, int]:
    if len(key) != 16 or len(nonce) != 16:
        raise KfwError("Ascon-128 requires a 16-byte key and 16-byte nonce")
    k0 = int.from_bytes(key[0:8], "big")
    k1 = int.from_bytes(key[8:16], "big")
    n0 = int.from_bytes(nonce[0:8], "big")
    n1 = int.from_bytes(nonce[8:16], "big")
    s = [0x80400C0600000000, k0, k1, n0, n1]
    _ascon_permute(s, 12)
    s[3] ^= k0
    s[4] ^= k1
    return s, k0, k1


def _ascon128_ad(s: list[int], aad: bytes) -> None:
    if aad:
        n_full = len(aad) // 8
        for i in range(n_full):
            s[0] ^= int.from_bytes(aad[i * 8 : (i + 1) * 8], "big")
            _ascon_permute(s, 6)
        rem = aad[n_full * 8 :]
        pad = bytearray(8)
        pad[: len(rem)] = rem
        pad[len(rem)] = 0x80
        s[0] ^= int.from_bytes(pad, "big")
        _ascon_permute(s, 6)
    s[4] ^= 1


def ascon128_encrypt(key: bytes, nonce: bytes, aad: bytes, pt: bytes) -> tuple[bytes, bytes]:
    s, k0, k1 = _ascon128_init(key, nonce)
    _ascon128_ad(s, aad)
    ct = bytearray(len(pt))
    n_full = len(pt) // 8
    for i in range(n_full):
        s[0] ^= int.from_bytes(pt[i * 8 : (i + 1) * 8], "big")
        ct[i * 8 : (i + 1) * 8] = s[0].to_bytes(8, "big")
        _ascon_permute(s, 6)
    rem_off = n_full * 8
    rem = pt[rem_off:]
    st = s[0].to_bytes(8, "big")
    pad = bytearray(8)
    for j, b in enumerate(rem):
        ct[rem_off + j] = b ^ st[j]
        pad[j] = b
    pad[len(rem)] = 0x80
    s[0] ^= int.from_bytes(pad, "big")
    s[1] ^= k0
    s[2] ^= k1
    _ascon_permute(s, 12)
    s[3] ^= k0
    s[4] ^= k1
    tag = s[3].to_bytes(8, "big") + s[4].to_bytes(8, "big")
    return bytes(ct), tag


def ascon128_decrypt(key: bytes, nonce: bytes, aad: bytes, ct: bytes, tag: bytes) -> bytes:
    if len(tag) != 16:
        raise KfwError("Ascon-128 tag must be 16 bytes")
    s, k0, k1 = _ascon128_init(key, nonce)
    _ascon128_ad(s, aad)
    pt = bytearray(len(ct))
    n_full = len(ct) // 8
    for i in range(n_full):
        c_word = int.from_bytes(ct[i * 8 : (i + 1) * 8], "big")
        p_word = s[0] ^ c_word
        s[0] = c_word
        pt[i * 8 : (i + 1) * 8] = p_word.to_bytes(8, "big")
        _ascon_permute(s, 6)
    rem_off = n_full * 8
    rem = ct[rem_off:]
    st = s[0].to_bytes(8, "big")
    pad = bytearray(8)
    for j, b in enumerate(rem):
        p = b ^ st[j]
        pt[rem_off + j] = p
        pad[j] = p
    pad[len(rem)] = 0x80
    s[0] ^= int.from_bytes(pad, "big")
    s[1] ^= k0
    s[2] ^= k1
    _ascon_permute(s, 12)
    s[3] ^= k0
    s[4] ^= k1
    expected = s[3].to_bytes(8, "big") + s[4].to_bytes(8, "big")
    if not hmac.compare_digest(tag, expected):
        raise KfwError("Ascon-AEAD128 authentication tag mismatch")
    return bytes(pt)


def ascon128_mac(key: bytes, nonce: bytes, aad: bytes, data: bytes = b"") -> bytes:
    _, tag = ascon128_encrypt(key, nonce, aad, data)
    return tag


# =============================================================================
# 3. POST-QUANTUM LMS-ASCON SIGNATURES (n=16, w=4, H=1..16, 100% Ascon-Hash256)
# =============================================================================

LMS_N = 16
LMS_W = 4
LMS_DEFAULT_H = 8        # 256 releases per key (up to H=12 = 4096)
LMS_LEN1 = 32            # 128 bits / 4 bits
LMS_LEN2 = 3             # checksum nibbles (max sum = 32*15 = 480 < 4096)
LMS_P = LMS_LEN1 + LMS_LEN2  # 35 chains


def _h16(data: bytes) -> bytes:
    return ascon_hash256(data)[:16]


def _wots_nibbles(msg16: bytes) -> list[int]:
    nibs: list[int] = []
    csum = 0
    for b in msg16:
        hi = (b >> 4) & 0xF
        lo = b & 0xF
        nibs.append(hi)
        nibs.append(lo)
        csum += (15 - hi) + (15 - lo)
    nibs.append((csum >> 8) & 0xF)
    nibs.append((csum >> 4) & 0xF)
    nibs.append(csum & 0xF)
    return nibs


def _wots_chain(tree_i: bytes, q: int, chain_idx: int, start: int, steps: int, val: bytes) -> bytes:
    cur = val
    prefix = b"KFW-WOTS" + tree_i + struct.pack("<IH", q, chain_idx)
    for j in range(start, start + steps):
        cur = _h16(prefix + bytes([j]) + cur)
    return cur


def _wots_leaf_pk(seed: bytes, tree_i: bytes, q: int) -> bytes:
    h = AsconHash256()
    h.update(b"KFW-LEAF" + tree_i + struct.pack("<I", q))
    for i in range(LMS_P):
        sk_i = _h16(b"KFW-SK00" + seed + tree_i + struct.pack("<IH", q, i))
        pk_i = _wots_chain(tree_i, q, i, 0, 15, sk_i)
        h.update(pk_i)
    return h.digest()[:16]


def _lms_build_tree(seed: bytes, tree_i: bytes, height: int) -> list[bytes]:
    leaves = 1 << height
    tree = [b""] * (2 * leaves)
    for q in range(leaves):
        tree[leaves + q] = _wots_leaf_pk(seed, tree_i, q)
    for node in range(leaves - 1, 0, -1):
        tree[node] = _h16(
            b"KFW-NODE" + tree_i + struct.pack("<I", node) + tree[2 * node] + tree[2 * node + 1]
        )
    return tree


def lms_keygen(signer: str = "krel", height: int = LMS_DEFAULT_H) -> dict[str, Any]:
    if not (1 <= height <= 12):
        raise KfwError(f"LMS-Ascon height must be 1..12 (got {height})")
    seed = secrets.token_bytes(32)
    tree_i = secrets.token_bytes(16)
    tree = _lms_build_tree(seed, tree_i, height)
    root = tree[1]
    pubkey = tree_i + root
    return {
        "alg": f"lms-ascon-n16-w4-h{height}",
        "height": height,
        "signer": signer[:4],
        "seed_hex": seed.hex(),
        "tree_i_hex": tree_i.hex(),
        "root_hex": root.hex(),
        "pubkey_hex": pubkey.hex(),
        "key_id_hex": ascon_hash256(pubkey)[:4].hex(),
        "next_leaf": 0,
        "max_leaves": 1 << height,
        "tree_hex": [n.hex() for n in tree],
    }


def lms_sign(key_obj: dict[str, Any], root_digest: bytes, signer: str | None = None) -> bytes:
    height = int(key_obj.get("height", LMS_DEFAULT_H))
    leaves = 1 << height
    q = int(key_obj["next_leaf"])
    if q < 0 or q >= leaves:
        raise KfwError(f"LMS-Ascon key exhausted (next_leaf={q} >= {leaves})")
    seed = bytes.fromhex(key_obj["seed_hex"])
    tree_i = bytes.fromhex(key_obj["tree_i_hex"])
    if "tree_hex" in key_obj and len(key_obj["tree_hex"]) == 2 * leaves:
        tree = [bytes.fromhex(x) if x else b"" for x in key_obj["tree_hex"]]
    else:
        tree = _lms_build_tree(seed, tree_i, height)
    pub_root = tree[1]
    pubkey = tree_i + pub_root
    key_id = ascon_hash256(pubkey)[:4]
    signer_b = (signer or key_obj.get("signer", "krel")).encode("ascii")[:4].ljust(4, b"\x00")

    msg16 = _h16(b"KFW-LMS-V1\x00" + tree_i + struct.pack("<IB", q, height) + root_digest)
    nibs = _wots_nibbles(msg16)

    sig_chains = bytearray()
    for i, a in enumerate(nibs):
        sk_i = _h16(b"KFW-SK00" + seed + tree_i + struct.pack("<IH", q, i))
        sig_chains.extend(_wots_chain(tree_i, q, i, 0, a, sk_i))

    auth_path = bytearray()
    node = leaves + q
    for _ in range(height):
        auth_path.extend(tree[node ^ 1])
        node >>= 1

    key_obj["next_leaf"] = q + 1
    return (
        key_id
        + signer_b
        + struct.pack("<IB3s", q, height, b"\x00\x00\x00")
        + tree_i
        + pub_root
        + bytes(sig_chains)
        + bytes(auth_path)
    )


def lms_verify(payload: bytes, root_digest: bytes, pinned_pubkey: bytes | None = None) -> tuple[bytes, str, int]:
    if len(payload) < 48:
        raise KfwError(f"LMS-Ascon payload too short ({len(payload)} B)")
    key_id = payload[0:4]
    signer = payload[4:8].rstrip(b"\x00").decode("ascii", errors="replace")
    q, height, _pad = struct.unpack_from("<IB3s", payload, 8)
    if not (1 <= height <= 12):
        raise KfwError(f"Invalid LMS-Ascon height {height}")
    leaves = 1 << height
    expected_len = 48 + LMS_P * LMS_N + height * LMS_N
    if len(payload) != expected_len:
        raise KfwError(f"Invalid LMS-Ascon payload size {len(payload)} != {expected_len}")
    if q >= leaves:
        raise KfwError(f"LMS-Ascon leaf_idx {q} out of range for H={height}")
    tree_i = payload[16:32]
    pub_root = payload[32:48]
    embedded_pk = tree_i + pub_root
    if pinned_pubkey is not None:
        if len(pinned_pubkey) != 32:
            raise KfwError("Pinned LMS-Ascon public key must be 32 bytes")
        if not hmac.compare_digest(embedded_pk, pinned_pubkey):
            raise KfwError("LMS-Ascon public key does not match pinned key")
    if not hmac.compare_digest(key_id, ascon_hash256(embedded_pk)[:4]):
        raise KfwError("LMS-Ascon key_id mismatch")

    msg16 = _h16(b"KFW-LMS-V1\x00" + tree_i + struct.pack("<IB", q, height) + root_digest)
    nibs = _wots_nibbles(msg16)

    h = AsconHash256()
    h.update(b"KFW-LEAF" + tree_i + struct.pack("<I", q))
    off = 48
    for i, a in enumerate(nibs):
        sig_i = payload[off : off + LMS_N]
        off += LMS_N
        pk_i = _wots_chain(tree_i, q, i, a, 15 - a, sig_i)
        h.update(pk_i)
    cur = h.digest()[:16]

    node = leaves + q
    for _ in range(height):
        sib = payload[off : off + LMS_N]
        off += LMS_N
        parent = node >> 1
        if node & 1:
            cur = _h16(b"KFW-NODE" + tree_i + struct.pack("<I", parent) + sib + cur)
        else:
            cur = _h16(b"KFW-NODE" + tree_i + struct.pack("<I", parent) + cur + sib)
        node = parent

    if not hmac.compare_digest(cur, pub_root):
        raise KfwError("LMS-Ascon signature verification failed")
    return embedded_pk, signer, q


# =============================================================================
# 4. COMPRESSION CODECS (PackBits RLE, Deflate, LZMA2, ARMTHUMB+LZMA2)
# =============================================================================

DEFAULT_LZMA2_DICT = 1 << 20  # 1 MiB
DEFAULT_LZMA2_PROP = 0x14     # XZ/LZMA2 dict prop byte for 1 MiB


def lzma2_prop_to_dict(prop: int) -> int:
    if prop > 40:
        raise KfwError(f"Invalid LZMA2 dict property byte: 0x{prop:02x}")
    if prop == 40:
        return 0xFFFFFFFF
    return (2 | (prop & 1)) << (prop // 2 + 11)


def packbits_encode(data: bytes) -> bytes:
    out = bytearray()
    n = len(data)
    i = 0
    while i < n:
        # Check for run of >= 3 identical bytes
        run = 1
        while i + run < n and run < 128 and data[i + run] == data[i]:
            run += 1
        if run >= 3:
            out.append((257 - run) & 0xFF)
            out.append(data[i])
            i += run
        else:
            lit_start = i
            lit_len = 0
            while i < n and lit_len < 128:
                if i + 2 < n and data[i] == data[i + 1] == data[i + 2]:
                    break
                i += 1
                lit_len += 1
            out.append(lit_len - 1)
            out.extend(data[lit_start : lit_start + lit_len])
    return bytes(out)


def packbits_decode(data: bytes, expected_len: int) -> bytes:
    out = bytearray()
    i = 0
    n = len(data)
    while i < n:
        ctrl = data[i]
        i += 1
        if ctrl <= 127:
            count = ctrl + 1
            if i + count > n:
                raise KfwError("Truncated PackBits literal run")
            out.extend(data[i : i + count])
            i += count
        elif ctrl >= 129:
            count = 257 - ctrl
            if i >= n:
                raise KfwError("Truncated PackBits repeat run")
            out.extend(bytes([data[i]]) * count)
            i += 1
        # 128 is NOP
        if len(out) > expected_len:
            raise KfwError("PackBits output exceeds declared raw_size")
    if len(out) != expected_len:
        raise KfwError(f"PackBits decoded {len(out)} B != raw_size {expected_len} B")
    return bytes(out)


def compress_payload(data: bytes, comp: int, dict_size: int = DEFAULT_LZMA2_DICT) -> tuple[bytes, int]:
    """Returns (compressed_bytes, lzma2_prop_byte)."""
    if comp == COMP_NONE:
        return data, 0
    if comp == COMP_PACKBITS_RLE:
        return packbits_encode(data), 0
    if comp == COMP_DEFLATE:
        c = zlib.compressobj(level=9, wbits=-15)
        return c.compress(data) + c.flush(), 0
    if comp == COMP_LZMA2:
        filters = [{"id": lzma.FILTER_LZMA2, "preset": 9 | lzma.PRESET_EXTREME, "dict_size": dict_size}]
        return lzma.compress(data, format=lzma.FORMAT_RAW, filters=filters), DEFAULT_LZMA2_PROP
    if comp == COMP_ARMTHUMB_LZMA2:
        filters = [
            {"id": lzma.FILTER_ARMTHUMB},
            {"id": lzma.FILTER_LZMA2, "preset": 9 | lzma.PRESET_EXTREME, "dict_size": dict_size},
        ]
        return lzma.compress(data, format=lzma.FORMAT_RAW, filters=filters), DEFAULT_LZMA2_PROP
    raise KfwError(f"Unsupported compression algorithm: {comp}")


def decompress_payload(data: bytes, comp: int, raw_size: int, lzma2_prop: int = DEFAULT_LZMA2_PROP) -> bytes:
    if comp == COMP_NONE:
        out = data
    elif comp == COMP_PACKBITS_RLE:
        out = packbits_decode(data, raw_size)
    elif comp == COMP_DEFLATE:
        d = zlib.decompressobj(wbits=-15)
        out = d.decompress(data, raw_size + 1)
        if len(out) > raw_size or not d.eof:
            raise KfwError("Deflate stream exceeded raw_size or failed to terminate")
    elif comp in (COMP_LZMA2, COMP_ARMTHUMB_LZMA2):
        dict_size = lzma2_prop_to_dict(lzma2_prop or DEFAULT_LZMA2_PROP)
        filters = []
        if comp == COMP_ARMTHUMB_LZMA2:
            filters.append({"id": lzma.FILTER_ARMTHUMB})
        filters.append({"id": lzma.FILTER_LZMA2, "dict_size": dict_size})
        dec = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=filters)
        out = dec.decompress(data, max_length=raw_size + 1)
        if len(out) > raw_size:
            raise KfwError("LZMA2 output exceeded declared raw_size (zip-bomb guard)")
    else:
        raise KfwError(f"Unsupported compression algorithm: {comp}")

    if len(out) != raw_size:
        raise KfwError(f"Decompressed length {len(out)} != declared raw_size {raw_size}")
    return out


# =============================================================================
# 5. HELPERS: SEMVER, TLV CODEC, KRX INSPECTOR, PATH & VECTOR VALIDATORS
# =============================================================================

def align_up(n: int, a: int) -> int:
    return (n + a - 1) & ~(a - 1)


def pack_semver(ver_str: str) -> int:
    """Packs 'MAJOR.MINOR.PATCH[-stage[.N]]' into monotonic u32."""
    m = re.match(r"^v?(\d+)\.(\d+)\.(\d+)(?:-(dev|alpha|beta|rc)(?:\.?(\d+))?)?$", ver_str.strip())
    if not m:
        raise KfwError(f"Invalid SemVer '{ver_str}' (expected e.g. 0.1.0 or 0.2.0-rc.1)")
    maj, mn, pat = int(m.group(1)), int(m.group(2)), int(m.group(3))
    if not (0 <= maj <= 255 and 0 <= mn <= 255 and 0 <= pat <= 255):
        raise KfwError(f"SemVer component out of 0..255 range: {ver_str}")
    stage_name, stage_num = m.group(4), m.group(5)
    n = int(stage_num) if stage_num else 1
    if stage_name is None:
        stage = 0xFF
    elif stage_name == "rc":
        stage = min(0xFE, 0xC0 + n)
    elif stage_name == "beta":
        stage = min(0xBF, 0x80 + n)
    elif stage_name == "alpha":
        stage = min(0x7F, 0x40 + n)
    else:
        stage = min(0x3F, n)
    return (maj << 24) | (mn << 16) | (pat << 8) | stage


def format_semver(v: int) -> str:
    maj = (v >> 24) & 0xFF
    mn = (v >> 16) & 0xFF
    pat = (v >> 8) & 0xFF
    st = v & 0xFF
    base = f"{maj}.{mn}.{pat}"
    if st == 0xFF:
        return base
    if st >= 0xC0:
        return f"{base}-rc.{st - 0xC0}"
    if st >= 0x80:
        return f"{base}-beta.{st - 0x80}"
    if st >= 0x40:
        return f"{base}-alpha.{st - 0x40}"
    return f"{base}-dev.{st}"


def detect_workspace_abi(repo_root: Path | None = None) -> str:
    """Read canonical `VERSION` from `src/abi/src/lib.rs`, returning `'MAJOR.0'` (e.g. `'5.0'`)."""
    root = repo_root or _TOOLS_DIR.parent
    abi_lib = root / "src" / "abi" / "src" / "lib.rs"
    if abi_lib.is_file():
        m = re.search(r"pub\s+const\s+VERSION\s*:\s*u16\s*=\s*(\d+)\s*;", abi_lib.read_text(errors="ignore"))
        if m:
            return f"{int(m.group(1))}.0"
    return "5.0"


def pack_abi(s: str) -> int:
    s_clean = s.strip()
    if s_clean.isdigit():
        return int(s_clean) << 8
    m = re.match(r"^(\d+)\.(\d+)$", s_clean)
    if not m:
        raise KfwError(f"Invalid ABI version '{s}' (expected MAJOR or MAJOR.MINOR, e.g. 5.0)")
    return (int(m.group(1)) << 8) | int(m.group(2))


def format_abi(v: int) -> str:
    return f"{(v >> 8) & 0xFF}.{v & 0xFF}"


def encode_tlv(tag: int, value: bytes) -> bytes:
    if len(value) > 0xFFFF:
        raise KfwError(f"TLV 0x{tag:04x} value exceeds 65535 bytes ({len(value)})")
    pad = (4 - (len(value) & 3)) & 3
    return struct.pack(KFW_TLV_FMT, tag, len(value)) + value + (b"\x00" * pad)


def parse_tlvs(buf: bytes | memoryview, strict_critical: bool = True) -> list[tuple[int, bytes]]:
    out: list[tuple[int, bytes]] = []
    off = 0
    total = len(buf)
    while off < total:
        if off + KFW_TLV_SIZE > total:
            raise KfwError(f"Truncated TLV header at offset {off}")
        tag, length = struct.unpack_from(KFW_TLV_FMT, buf, off)
        off += KFW_TLV_SIZE
        if off + length > total:
            raise KfwError(f"TLV 0x{tag:04x} length {length} overruns region")
        val = bytes(buf[off : off + length])
        pad = (4 - (length & 3)) & 3
        if off + length + pad > total:
            raise KfwError(f"TLV 0x{tag:04x} alignment padding overruns region")
        if any(buf[off + length : off + length + pad]):
            raise KfwError(f"Non-zero padding after TLV 0x{tag:04x}")
        off += length + pad
        if strict_critical and (tag & 0x8000) and tag not in KNOWN_CRITICAL_TLVS:
            raise KfwError(f"Unknown critical TLV tag 0x{tag:04x}")
        out.append((tag, val))
    return out


def validate_kernfs_path(path: str, allow_calib: bool = False) -> None:
    if not path or not path.startswith("/"):
        raise KfwError(f"KernFS path must be absolute (start with '/'): {path!r}")
    if len(path) > 96:
        raise KfwError(f"KernFS path too long: {path!r}")
    if any(ord(c) < 0x21 or ord(c) > 0x7E for c in path):
        raise KfwError(f"KernFS path contains invalid characters: {path!r}")
    if "\\" in path or "//" in path or "/../" in path or path.endswith("/..") or path == "/..":
        raise KfwError(f"Path traversal forbidden in KernFS path: {path!r}")
    if not allow_calib and (path == "/sys/calib.dat" or path == "/sys/calib.sig" or path.startswith("/sys/calib.")):
        raise KfwError(f"Refusing to overwrite protected calibration file {path!r} outside backup/recovery")


def inspect_krx_binary(data: bytes) -> dict[str, Any]:
    """Extracts profile, capabilities, ABI major, and SemVer from a KRX v2..v5 binary."""
    info = {"profile": 0, "caps": 0, "pkg_ver": pack_semver("1.0.0"), "min_abi": 0}
    if len(data) < 24 or data[0:4] != b"\xC0\xDE\x01\xFE":
        return info
    # KRX 24-byte preamble: ident[4], app_class(u8), abi_ver(u8), sec_cnt(u16), hdr_len(u16), ...
    abi_ver = data[5]
    sec_cnt, hdr_len = struct.unpack_from("<HH", data, 6)
    info["min_abi"] = (abi_ver << 8) if abi_ver > 0 else 0
    if 24 <= hdr_len <= len(data):
        off = 24
        for _ in range(min(sec_cnt, 32)):
            if off + 4 > hdr_len:
                break
            sid, slen = struct.unpack_from("<HH", data, off)
            if slen < 4 or off + slen > hdr_len:
                break
            body = data[off + 4 : off + slen]
            if sid == 0x0001 and len(body) >= 5:  # SectionIdentity
                maj, mn, pat = body[2], body[3], body[4]
                info["pkg_ver"] = (maj << 24) | (mn << 16) | (pat << 8) | 0xFF
            elif sid == 0x0002 and len(body) >= 27:  # SectionExecution
                info["profile"] = body[26]
            elif sid == 0x0009 and len(body) >= 12:  # SectionRuntime
                info["caps"] = struct.unpack_from("<I", body, 8)[0]
            off += (slen + 3) & ~3
    return info


def validate_arm_vectors(fw: bytes, load_addr: int, sram_kb: int, expected_sp: int = 0, expected_pc: int = 0) -> tuple[int, int]:
    if len(fw) < 16:
        raise KfwError(f"Firmware too short for ARM vector table ({len(fw)} B)")
    sp, reset_pc, nmi_pc, hf_pc = struct.unpack_from("<IIII", fw, 0)
    sram_base = 0x20000000
    sram_end = sram_base + sram_kb * 1024
    if not (sram_base < sp <= sram_end) or (sp & 3) != 0:
        raise KfwError(f"Invalid ARM initial SP 0x{sp:08X} (expected 4B-aligned in 0x{sram_base:08X}..0x{sram_end:08X})")
    fw_end = load_addr + len(fw)
    for label, vec in (("Reset_Handler", reset_pc), ("NMI_Handler", nmi_pc), ("HardFault_Handler", hf_pc)):
        if (vec & 1) != 1:
            raise KfwError(f"ARM {label} 0x{vec:08X} missing Thumb bit (bit 0)")
        addr = vec & ~1
        if not (load_addr <= addr < fw_end):
            raise KfwError(f"ARM {label} 0x{vec:08X} outside firmware bounds [0x{load_addr:08X}, 0x{fw_end:08X})")
    if expected_sp and sp != expected_sp:
        raise KfwError(f"SP mismatch: binary 0x{sp:08X} != TLV 0x{expected_sp:08X}")
    if expected_pc and reset_pc != expected_pc:
        raise KfwError(f"Reset_Handler mismatch: binary 0x{reset_pc:08X} != TLV 0x{expected_pc:08X}")
    return sp, reset_pc


def xor_quansheng_wire(data: bytes) -> bytes:
    buf = bytearray(data)
    rem = len(buf) % 16
    if rem:
        buf.extend(b"\xFF" * (16 - rem))
    for i in range(len(buf)):
        buf[i] ^= QS_OBFUS_TBL[i & 15]
    return bytes(buf)


# =============================================================================
# 6. CONTAINER MODEL, PACKER & VERIFIER
# =============================================================================

@dataclass
class TargetSpec:
    target_id: int
    mcu_id: int
    arch_rf: int
    transport_disp: int
    flash_base: int
    flash_max: int
    page_size: int
    sram_kb: int
    hw_rev_mask: int
    ext_flash_kb: int
    slug: str

    def pack(self) -> bytes:
        return (
            struct.pack(
                "<IHBBIIHHHH",
                self.target_id, self.mcu_id, self.arch_rf, self.transport_disp,
                self.flash_base, self.flash_max, self.page_size, self.sram_kb,
                self.hw_rev_mask, self.ext_flash_kb,
            )
            + self.slug.encode("utf-8")
        )

    @classmethod
    def unpack(cls, b: bytes) -> "TargetSpec":
        if len(b) < 25:
            raise KfwError("TLV_TARGET too short")
        vals = struct.unpack_from("<IHBBIIHHHH", b, 0)
        slug = b[24:].decode("utf-8")
        return cls(*vals, slug)


@dataclass
class BootPolicySpec:
    target_idx: int
    boot_family: int
    max_attempts: int
    confirm_sec: int
    min_boot_ver: int
    max_boot_ver: int
    min_battery_mv: int
    min_fs_schema: int
    usb_vid: int
    usb_pid: int
    uart_baud: int
    policy_flags: int
    bl_prefix: bytes

    def pack(self) -> bytes:
        return struct.pack(
            "<BBBBIIHHHHII4s",
            self.target_idx, self.boot_family, self.max_attempts, self.confirm_sec,
            self.min_boot_ver, self.max_boot_ver, self.min_battery_mv, self.min_fs_schema,
            self.usb_vid, self.usb_pid, self.uart_baud, self.policy_flags,
            self.bl_prefix[:4].ljust(4, b"\x00"),
        )

    @classmethod
    def unpack(cls, b: bytes) -> "BootPolicySpec":
        if len(b) != 32:
            raise KfwError(f"TLV_BOOT_POLICY size {len(b)} != 32")
        return cls(*struct.unpack("<BBBBIIHHHHII4s", b))


@dataclass
class ProtectRangeSpec:
    target_idx: int
    bus: int
    flags: int
    start: int
    end: int

    def pack(self) -> bytes:
        return struct.pack("<BBHII", self.target_idx, self.bus, self.flags, self.start, self.end)

    @classmethod
    def unpack(cls, b: bytes) -> "ProtectRangeSpec":
        if len(b) != 12:
            raise KfwError(f"TLV_PROTECT_RANGE size {len(b)} != 12")
        return cls(*struct.unpack("<BBHII", b))


@dataclass
class PartInput:
    kind: int
    data: bytes
    target_idx: int = 0xFF
    slot_or_policy: int = 0
    comp: int = COMP_ARMTHUMB_LZMA2
    in_solid: bool = False
    post_flash: bool = False
    page_aligned: bool = False
    mcu_streamable: bool = False
    encrypt_key: bytes | None = None
    # Typed TLV attachments
    load_addr: int | None = None
    bus: int = BUS_INTERNAL_FLASH
    fw_flags: int = FWFLAG_HAS_VECTORS | FWFLAG_REQUIRES_REBOOT
    dest_path: str | None = None
    file_type: int = 0
    krx_profile: int = 0
    min_abi: int = 0
    max_abi: int = 0
    krx_caps: int = 0
    pkg_ver: int = 0
    build_stats: tuple[int, int, int, int, int] | None = None  # clock, text, rodata, data, sram


def compute_root_digest(header_prefix_32: bytes, tail_after_header: bytes) -> bytes:
    h = AsconHash256()
    h.update(b"KFW-ROOT-V1\x00")
    h.update(header_prefix_32)
    h.update(tail_after_header)
    return h.digest()


def build_kfw(
    *,
    kind: int,
    fw_version: int,
    sec_cnt: int = 0,
    krx_abi_ver: int = 0x0302,
    header_flags: int = HFLAG_REQUIRE_CALIB_BACKUP | HFLAG_PRESERVE_BOOTLOADER | HFLAG_VERIFY_READBACK | HFLAG_REPRODUCIBLE,
    targets: list[TargetSpec],
    boot_policies: list[BootPolicySpec],
    protect_ranges: list[ProtectRangeSpec],
    parts: list[PartInput],
    manifest: dict[str, Any] | None = None,
    solid_comp: int = COMP_ARMTHUMB_LZMA2,
    solid_encrypt_key: bytes | None = None,
    extra_tlvs: list[tuple[int, bytes]] | None = None,
    mac_key: bytes | None = None,
    mac_signer: str = "krel",
    signer: str | None = None,
    lms_key_obj: dict[str, Any] | None = None,
) -> bytes:
    """Packs a conformant, deterministic `.kfw` v1.0 container."""
    tlvs: list[tuple[int, bytes]] = []
    for t in targets:
        tlvs.append((TLV_TARGET, t.pack()))
    for bp in boot_policies:
        tlvs.append((TLV_BOOT_POLICY, bp.pack()))
    for pr in protect_ranges:
        tlvs.append((TLV_PROTECT_RANGE, pr.pack()))

    solid_members = [p for p in parts if p.in_solid]
    if solid_members:
        header_flags |= HFLAG_SOLID
    if solid_encrypt_key is not None or any(p.encrypt_key is not None for p in parts):
        header_flags |= HCRIT_ENCRYPTED

    # Build final part list: if solid members exist, insert PART_SOLID_ARENA at index 0
    all_parts_meta: list[tuple[int, int, int, int, int, int, int, bytes]] = []
    stored_blobs: list[tuple[int, bytes, bool]] = []  # (part_idx, stored_bytes, page_aligned)

    part_offset_idx = 1 if solid_members else 0

    if solid_members:
        solid_raw = bytearray()
        for p in solid_members:
            if p.kind == PART_DEBUG_SYMBOLS:
                raise KfwError("PART_DEBUG_SYMBOLS must not be placed inside PART_SOLID_ARENA")
            solid_raw.extend(p.data)
        solid_raw_bytes = bytes(solid_raw)
        solid_comp_bytes, lzma2_prop = compress_payload(solid_raw_bytes, solid_comp)
        solid_pflags = solid_comp & PFLAG_COMP_MASK
        if solid_comp in (COMP_NONE, COMP_PACKBITS_RLE, COMP_HEATSHRINK):
            solid_pflags |= PFLAG_MCU_STREAMABLE
        if solid_encrypt_key is not None:
            solid_pflags |= PFLAG_ENCRYPTED
        solid_digest = ascon_hash256(solid_raw_bytes)[:16]
        all_parts_meta.append(
            (PART_SOLID_ARENA, solid_pflags, 0xFF, 0, 0, len(solid_comp_bytes) + (32 if solid_encrypt_key else 0), len(solid_raw_bytes), solid_digest)
        )
        stored_blobs.append((0, solid_comp_bytes, False))
        dict_kb = (DEFAULT_LZMA2_DICT // 1024) if solid_comp in (COMP_LZMA2, COMP_ARMTHUMB_LZMA2) else 4
        max_ram_kb = dict_kb + 256 if dict_kb > 4 else 4
        tlvs.append((TLV_SOLID_PARAMS, struct.pack("<HBBII", 0, lzma2_prop, 0, dict_kb, max_ram_kb)))

    solid_cursor = 0
    for idx_in, p in enumerate(parts):
        pidx = idx_in + part_offset_idx
        plain_digest = ascon_hash256(p.data)[:16]

        # Pre-flight validation at pack time (never emit a brickable .kfw!)
        if p.kind in (PART_FIRMWARE, PART_BOOTLOADER):
            if p.target_idx >= len(targets):
                raise KfwError(f"Firmware part {pidx} references invalid target_idx {p.target_idx}")
            tgt = targets[p.target_idx]
            load_addr = p.load_addr if p.load_addr is not None else tgt.flash_base
            # Check protected ranges for PART_FIRMWARE; for PART_BOOTLOADER still enforce strict (bit 1) ranges like QS-1
            for pr in protect_ranges:
                if pr.target_idx in (0xFF, p.target_idx) and pr.bus == p.bus:
                    if p.kind == PART_BOOTLOADER and (pr.flags & 0x0002) == 0:
                        continue
                    if load_addr < pr.end and (load_addr + len(p.data)) > pr.start:
                        raise KfwError(
                            f"Part {pidx} [0x{load_addr:08X}..0x{load_addr+len(p.data):08X}] "
                            f"overlaps protected range [0x{pr.start:08X}..0x{pr.end:08X}]"
                        )
            if p.kind == PART_FIRMWARE and p.bus == BUS_INTERNAL_FLASH and len(p.data) > tgt.flash_max:
                raise KfwError(f"Firmware size {len(p.data)} B exceeds target flash_max {tgt.flash_max} B")
            sp, pc = (0, 0)
            if p.fw_flags & FWFLAG_HAS_VECTORS:
                sp, pc = validate_arm_vectors(p.data, load_addr, tgt.sram_kb)
            tlvs.append((TLV_PART_ADDR, struct.pack("<HBBIII", pidx, p.bus, p.fw_flags, load_addr, sp, pc)))
            if p.build_stats:
                clk, t_sz, r_sz, d_sz, sram_sz = p.build_stats
                tlvs.append((TLV_BUILD_STATS, struct.pack("<HHIIII", pidx, clk, t_sz, r_sz, d_sz, sram_sz)))
        elif p.kind in (PART_KRX_MODULE, PART_KERNFS_FILE, PART_OPT_KRX, PART_OPT_FILE):
            if not p.dest_path:
                raise KfwError(f"KernFS/KRX part {pidx} missing dest_path")
            validate_kernfs_path(p.dest_path, allow_calib=(kind in (KIND_BACKUP, KIND_RECOVERY)))
            if p.kind in (PART_KRX_MODULE, PART_OPT_KRX) and p.min_abi and krx_abi_ver:
                if krx_abi_ver < p.min_abi or (p.max_abi and krx_abi_ver > p.max_abi):
                    raise KfwError(f"KRX module {p.dest_path} ABI mismatch with container krx_abi_ver")
            tlvs.append((
                TLV_PART_PATH,
                struct.pack(
                    "<HBBHHQI",
                    pidx, p.file_type, p.krx_profile, p.min_abi, p.max_abi, p.krx_caps, p.pkg_ver,
                )
                + p.dest_path.encode("utf-8"),
            ))

        pflags = 0
        if p.post_flash:
            pflags |= PFLAG_POST_FLASH
        if p.page_aligned:
            pflags |= PFLAG_PAGE_ALIGNED
        if p.mcu_streamable or (not p.in_solid and p.comp in (COMP_NONE, COMP_PACKBITS_RLE, COMP_HEATSHRINK)):
            pflags |= PFLAG_MCU_STREAMABLE

        if p.in_solid:
            pflags |= PFLAG_IN_SOLID
            all_parts_meta.append((p.kind, pflags, p.target_idx, p.slot_or_policy, solid_cursor, 0, len(p.data), plain_digest))
            solid_cursor += len(p.data)
        else:
            comp_bytes, _ = compress_payload(p.data, p.comp)
            pflags |= p.comp & PFLAG_COMP_MASK
            if p.encrypt_key is not None:
                pflags |= PFLAG_ENCRYPTED
            stored_len = len(comp_bytes) + (32 if p.encrypt_key is not None else 0)
            all_parts_meta.append((p.kind, pflags, p.target_idx, p.slot_or_policy, 0, stored_len, len(p.data), plain_digest))
            stored_blobs.append((pidx, comp_bytes, p.page_aligned))

    if manifest is not None:
        m_bytes = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode("utf-8")
        tlvs.append((TLV_MANIFEST, m_bytes))
    if extra_tlvs:
        tlvs.extend(extra_tlvs)

    tlv_blob = b"".join(encode_tlv(t, v) for t, v in tlvs)
    if len(tlv_blob) > 0xFFFF:
        raise KfwError(f"Protected TLV region exceeds 65535 bytes ({len(tlv_blob)})")

    part_count = len(all_parts_meta)
    cur_off = align_up(KFW_HEADER_SIZE + part_count * KFW_PART_SIZE + len(tlv_blob), 16)

    # Assign offsets & encrypt stored blobs (now that exact KfwPart[0..16] AAD is final)
    part_entries = list(all_parts_meta)
    final_blobs: list[tuple[int, bytes]] = []
    for pidx, comp_bytes, page_aligned in stored_blobs:
        cur_off = align_up(cur_off, 256 if page_aligned else 16)
        k_kind, pflags, tidx, sop, _, stored_len, raw_len, dig = part_entries[pidx]
        part_entries[pidx] = (k_kind, pflags, tidx, sop, cur_off, stored_len, raw_len, dig)
        blob = comp_bytes
        if pflags & PFLAG_ENCRYPTED:
            enc_key = solid_encrypt_key if pidx == 0 and solid_members else parts[pidx - part_offset_idx].encrypt_key
            assert enc_key is not None
            # Deterministic nonce if REPRODUCIBLE? Never reuse nonces with same key; use CSPRNG 128-bit nonce.
            nonce = secrets.token_bytes(16)
            part_hdr16 = struct.pack("<BBBBIII", k_kind, pflags, tidx, sop, cur_off, stored_len, raw_len)
            ct, tag = ascon128_encrypt(enc_key, nonce, b"KFW-ENC-V1\x00" + part_hdr16, comp_bytes)
            blob = nonce + tag + ct
            assert len(blob) == stored_len
        final_blobs.append((cur_off, blob))
        cur_off += len(blob)

    body_size = align_up(cur_off, 16)
    part_table_bytes = b"".join(struct.pack(KFW_PART_FMT, *pe) for pe in part_entries)

    body = bytearray(body_size)
    header_prefix = struct.pack(
        "<8sBBHIIIIHH",
        FILE_ID_KFW, KFW_VERSION_1_0, kind, part_count,
        header_flags, body_size, fw_version, sec_cnt, krx_abi_ver, len(tlv_blob),
    )
    body[0:32] = header_prefix
    pt_start = KFW_HEADER_SIZE
    body[pt_start : pt_start + len(part_table_bytes)] = part_table_bytes
    tlv_start = pt_start + len(part_table_bytes)
    body[tlv_start : tlv_start + len(tlv_blob)] = tlv_blob
    for off, blob in final_blobs:
        body[off : off + len(blob)] = blob

    root_digest = compute_root_digest(bytes(body[0:32]), bytes(body[64:body_size]))
    body[32:64] = root_digest

    # Optional Unprotected Auth Trailer (strictly after body_size — never mutates root_digest!)
    eff_signer = signer or mac_signer
    trailer = bytearray()
    if mac_key is not None:
        trailer.extend(make_mac_auth_tlv(root_digest, mac_key, eff_signer))
    if lms_key_obj is not None:
        trailer.extend(encode_tlv(TLV_AUTH_LMS_ASCON, lms_sign(lms_key_obj, root_digest, eff_signer)))

    return bytes(body) + bytes(trailer)


def make_mac_auth_tlv(root_digest: bytes, key: bytes, signer: str = "krel") -> bytes:
    if len(key) != 16:
        raise KfwError("Ascon-128-MAC key must be 16 bytes")
    key_id = ascon_hash256(key)[:4]
    signer_b = signer.encode("ascii")[:4].ljust(4, b"\x00")
    nonce = b"KFW-MAC-V1\x00\x00" + key_id
    tag = ascon128_mac(key, nonce, signer_b + root_digest, b"")
    return encode_tlv(TLV_AUTH_ASCON_MAC, key_id + signer_b + tag)


@dataclass
class ParsedKfw:
    raw: bytes
    version: int
    kind: int
    flags: int
    body_size: int
    fw_version: int
    sec_cnt: int
    krx_abi_ver: int
    tlv_size: int
    root_digest: bytes
    parts: list[dict[str, Any]]
    targets: list[TargetSpec]
    boot_policies: list[BootPolicySpec]
    protect_ranges: list[ProtectRangeSpec]
    part_addrs: dict[int, tuple[int, int, int, int, int]]  # bus, fw_flags, load_addr, sp, pc
    part_paths: dict[int, dict[str, Any]]
    solid_params: dict[int, tuple[int, int, int, int]]
    build_stats: dict[int, tuple[int, int, int, int, int]]
    manifest: dict[str, Any] | None
    auth_tlvs: list[tuple[int, bytes]]


def parse_kfw(data: bytes, verify_root: bool = True) -> ParsedKfw:
    if len(data) < KFW_HEADER_SIZE:
        raise KfwError(f"File too short ({len(data)} B < 64 B)")
    (
        ident, version, kind, part_count, flags, body_size,
        fw_version, sec_cnt, krx_abi_ver, tlv_size, root_digest,
    ) = struct.unpack_from(KFW_HEADER_FMT, data, 0)

    if ident != FILE_ID_KFW:
        raise KfwError("Invalid .kfw format identifier")
    if (version >> 4) != 1:
        raise KfwError(f"Unsupported .kfw major version {version >> 4}")
    if (flags & 0xFFFF0000) & ~HCRIT_KNOWN_MASK:
        raise KfwError(f"Unknown critical header flags: 0x{(flags & 0xFFFF0000) & ~HCRIT_KNOWN_MASK:08X}")
    if body_size < KFW_HEADER_SIZE or body_size > len(data) or (body_size & 15) != 0:
        raise KfwError(f"Invalid body_size {body_size} (file len {len(data)})")
    if (tlv_size & 3) != 0:
        raise KfwError(f"tlv_size {tlv_size} not 4-byte aligned")

    pt_end = KFW_HEADER_SIZE + part_count * KFW_PART_SIZE
    tlv_end = pt_end + tlv_size
    if tlv_end > body_size:
        raise KfwError("Part table + TLV region exceeds body_size")

    if verify_root:
        calc_root = compute_root_digest(data[0:32], data[64:body_size])
        if not hmac.compare_digest(calc_root, root_digest):
            raise KfwError("Root Ascon-Hash256 digest mismatch (file corrupted or tampered)")

    parts: list[dict[str, Any]] = []
    spans: list[tuple[int, int]] = []
    for i in range(part_count):
        p_kind, p_flags, tidx, sop, off, s_sz, r_sz, dig = struct.unpack_from(
            KFW_PART_FMT, data, KFW_HEADER_SIZE + i * KFW_PART_SIZE
        )
        if (p_kind & 0x80) == 0 and p_kind not in KNOWN_CRITICAL_PARTS:
            raise KfwError(f"Unknown critical part kind 0x{p_kind:02X} at index {i}")
        in_solid = bool(p_flags & PFLAG_IN_SOLID)
        if not in_solid:
            align = 256 if (p_flags & PFLAG_PAGE_ALIGNED) else 16
            if (off & (align - 1)) != 0 or off < tlv_end or off + s_sz > body_size:
                raise KfwError(f"Part {i} offset/size [0x{off:X}..0x{off+s_sz:X}] invalid or unaligned")
            for a, b in spans:
                if off < b and off + s_sz > a:
                    raise KfwError(f"Part {i} overlaps another stored part")
            spans.append((off, off + s_sz))
        elif s_sz != 0:
            raise KfwError(f"IN_SOLID part {i} must have stored_size == 0")

        parts.append({
            "index": i,
            "kind": p_kind,
            "flags": p_flags,
            "comp": p_flags & PFLAG_COMP_MASK,
            "encrypted": bool(p_flags & PFLAG_ENCRYPTED),
            "in_solid": in_solid,
            "post_flash": bool(p_flags & PFLAG_POST_FLASH),
            "page_aligned": bool(p_flags & PFLAG_PAGE_ALIGNED),
            "mcu_streamable": bool(p_flags & PFLAG_MCU_STREAMABLE),
            "target_idx": tidx,
            "slot_or_policy": sop,
            "offset": off,
            "stored_size": s_sz,
            "raw_size": r_sz,
            "digest": dig,
        })

    tlvs = parse_tlvs(memoryview(data)[pt_end:tlv_end], strict_critical=True)
    targets: list[TargetSpec] = []
    boot_policies: list[BootPolicySpec] = []
    protect_ranges: list[ProtectRangeSpec] = []
    part_addrs: dict[int, tuple[int, int, int, int, int]] = {}
    part_paths: dict[int, dict[str, Any]] = {}
    solid_params: dict[int, tuple[int, int, int, int]] = {}
    build_stats: dict[int, tuple[int, int, int, int, int]] = {}
    manifest: dict[str, Any] | None = None

    for tag, val in tlvs:
        if tag == TLV_TARGET:
            targets.append(TargetSpec.unpack(val))
        elif tag == TLV_BOOT_POLICY:
            boot_policies.append(BootPolicySpec.unpack(val))
        elif tag == TLV_PROTECT_RANGE:
            protect_ranges.append(ProtectRangeSpec.unpack(val))
        elif tag == TLV_PART_ADDR:
            if len(val) != 16:
                raise KfwError("Invalid TLV_PART_ADDR length")
            pidx, bus, fwf, load_addr, sp, pc = struct.unpack("<HBBIII", val)
            part_addrs[pidx] = (bus, fwf, load_addr, sp, pc)
        elif tag == TLV_PART_PATH:
            if len(val) < 21:
                raise KfwError("Invalid TLV_PART_PATH length")
            pidx, ftype, prof, min_abi, max_abi, caps, pver = struct.unpack_from("<HBBHHQI", val, 0)
            path_str = val[20:].decode("utf-8")
            validate_kernfs_path(path_str, allow_calib=(kind in (KIND_BACKUP, KIND_RECOVERY)))
            part_paths[pidx] = {
                "file_type": ftype, "krx_profile": prof, "min_abi": min_abi,
                "max_abi": max_abi, "krx_caps": caps, "pkg_ver": pver, "path": path_str,
            }
        elif tag == TLV_SOLID_PARAMS:
            if len(val) != 12:
                raise KfwError("Invalid TLV_SOLID_PARAMS length")
            pidx, lprop, bcj_sh, d_kb, ram_kb = struct.unpack("<HBBII", val)
            solid_params[pidx] = (lprop, bcj_sh, d_kb, ram_kb)
        elif tag == TLV_BUILD_STATS:
            if len(val) != 20:
                raise KfwError("Invalid TLV_BUILD_STATS length")
            pidx, clk, t_sz, r_sz, d_sz, sram = struct.unpack("<HHIIII", val)
            build_stats[pidx] = (clk, t_sz, r_sz, d_sz, sram)
        elif tag == TLV_MANIFEST:
            manifest = json.loads(val.decode("utf-8"))

    auth_tlvs = parse_tlvs(memoryview(data)[body_size:], strict_critical=False) if body_size < len(data) else []

    return ParsedKfw(
        raw=data, version=version, kind=kind, flags=flags, body_size=body_size,
        fw_version=fw_version, sec_cnt=sec_cnt, krx_abi_ver=krx_abi_ver,
        tlv_size=tlv_size, root_digest=root_digest, parts=parts, targets=targets,
        boot_policies=boot_policies, protect_ranges=protect_ranges,
        part_addrs=part_addrs, part_paths=part_paths, solid_params=solid_params,
        build_stats=build_stats, manifest=manifest, auth_tlvs=auth_tlvs,
    )


def save_lms_key_atomic(path: str | Path, key_obj: dict[str, Any]) -> None:
    """Persist stateful LMS-Ascon key (`next_leaf`) atomically via temp file + fsync + replace."""
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_suffix(p.suffix + f".tmp.{os.getpid()}")
    payload = json.dumps(key_obj, indent=2) + "\n"
    with tmp.open("w", encoding="utf-8") as fh:
        fh.write(payload)
        fh.flush()
        os.fsync(fh.fileno())
    os.replace(tmp, p)
    rem = int(key_obj.get("max_leaves", 1 << int(key_obj.get("height", LMS_DEFAULT_H)))) - int(key_obj.get("next_leaf", 0))
    if rem <= 16:
        _cli_ui.warn(f"LMS-Ascon key {p.name} has only {rem} leaf signature(s) remaining — rotate key soon!")


def default_trust_store_path(repo_root: Path | None = None) -> Path:
    return (repo_root or _TOOLS_DIR.parent) / "keys" / "trusted-keys.json"


def load_trust_store(path: str | Path | None) -> dict[str, Any]:
    if not path:
        return {"schema": 1, "keys": []}
    p = Path(path)
    if not p.is_file():
        raise KfwError(f"Trust store not found: {p}")
    obj = json.loads(p.read_text(encoding="utf-8"))
    if not isinstance(obj, dict) or not isinstance(obj.get("keys"), list):
        raise KfwError(f"Invalid trust store JSON format: {p}")
    return obj


def save_trust_store(path: str | Path, store: dict[str, Any]) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_suffix(p.suffix + f".tmp.{os.getpid()}")
    tmp.write_text(json.dumps(store, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(tmp, p)


def lookup_trusted_lms_key(
    pubkey: bytes,
    trust_store: dict[str, Any],
    *,
    channel: str | None = None,
) -> dict[str, Any] | None:
    pk_hex = pubkey.hex().lower()
    for entry in trust_store.get("keys", []):
        if str(entry.get("pubkey_hex", "")).lower() != pk_hex:
            continue
        status = str(entry.get("status", "active")).lower()
        if status == "revoked":
            raise KfwError(
                f"Signing key {entry.get('key_id_hex', pk_hex[:8])} ({entry.get('signer', '?')}) is REVOKED in trust store"
            )
        if status != "active":
            raise KfwError(
                f"Signing key {entry.get('key_id_hex', pk_hex[:8])} has non-active status '{status}'"
            )
        if channel:
            allowed = entry.get("channels") or ([entry["channel"]] if entry.get("channel") else [])
            if allowed and channel not in allowed and "*" not in allowed:
                raise KfwError(
                    f"Signing key {entry.get('key_id_hex', pk_hex[:8])} is not authorized for channel '{channel}'"
                )
        return entry
    return None


def sign_manifest_bytes(
    canonical_json: bytes,
    *,
    lms_key_obj: dict[str, Any] | None = None,
    mac_key: bytes | None = None,
    signer: str = "krel",
) -> dict[str, Any]:
    """Produce a detached signature envelope over a canonical `release-manifest.json`."""
    digest = ascon_hash256(b"KFW-MANIFEST-V1\x00" + canonical_json)
    sigs: list[dict[str, Any]] = []
    if lms_key_obj is not None:
        raw_sig = lms_sign(lms_key_obj, digest, signer=signer)
        sigs.append({
            "alg": lms_key_obj.get("alg", "lms-ascon"),
            "key_id": raw_sig[0:4].hex(),
            "signer": raw_sig[4:8].rstrip(b"\x00").decode("ascii", errors="replace"),
            "leaf_idx": struct.unpack_from("<I", raw_sig, 8)[0],
            "pubkey_hex": lms_key_obj["pubkey_hex"],
            "sig_hex": raw_sig.hex(),
        })
    if mac_key is not None:
        key_id = ascon_hash256(mac_key)[:4]
        signer_b = signer.encode("ascii")[:4].ljust(4, b"\x00")
        nonce = b"KFW-MAC-V1\x00\x00" + key_id
        tag = ascon128_mac(mac_key, nonce, signer_b + digest, b"")
        sigs.append({
            "alg": "ascon-128-mac",
            "key_id": key_id.hex(),
            "signer": signer_b.rstrip(b"\x00").decode("ascii", errors="replace"),
            "tag_hex": tag.hex(),
        })
    if not sigs:
        raise KfwError("Supply lms_key_obj and/or mac_key to sign manifest")
    return {
        "schema": 1,
        "manifest_ascon256": digest.hex(),
        "signatures": sigs,
    }


def verify_manifest_bytes(
    canonical_json: bytes,
    sig_envelope: dict[str, Any],
    *,
    lms_pubkey: bytes | None = None,
    mac_key: bytes | None = None,
    trust_store: dict[str, Any] | None = None,
    channel: str | None = None,
    require_auth: bool = True,
) -> list[str]:
    digest = ascon_hash256(b"KFW-MANIFEST-V1\x00" + canonical_json)
    exp = str(sig_envelope.get("manifest_ascon256", "")).lower()
    if exp and not hmac.compare_digest(bytes.fromhex(exp), digest):
        raise KfwError("Release manifest Ascon-Hash256 mismatch against signature envelope")
    verified: list[str] = []
    for s in sig_envelope.get("signatures", []):
        alg = str(s.get("alg", ""))
        if alg.startswith("lms-ascon"):
            raw_sig = bytes.fromhex(str(s["sig_hex"]))
            pk, signer, leaf_idx = lms_verify(raw_sig, digest, pinned_pubkey=lms_pubkey)
            if trust_store is not None:
                entry = lookup_trusted_lms_key(pk, trust_store, channel=channel)
                if entry is None:
                    raise KfwError(f"Manifest LMS-Ascon key {pk[:4].hex()}.. is not in trust store")
                verified.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, trusted={entry.get('key_id_hex', pk[:4].hex())})")
            elif lms_pubkey is not None:
                verified.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, pinned={pk[:4].hex()}..)")
            elif not require_auth:
                verified.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, UNPINNED)")
        elif alg == "ascon-128-mac" and mac_key is not None:
            key_id = bytes.fromhex(str(s["key_id"]))
            signer_b = str(s.get("signer", "krel")).encode("ascii")[:4].ljust(4, b"\x00")
            tag = bytes.fromhex(str(s["tag_hex"]))
            if hmac.compare_digest(key_id, ascon_hash256(mac_key)[:4]):
                nonce = b"KFW-MAC-V1\x00\x00" + key_id
                exp_tag = ascon128_mac(mac_key, nonce, signer_b + digest, b"")
                if not hmac.compare_digest(tag, exp_tag):
                    raise KfwError("Manifest Ascon-128-MAC verification failed")
                verified.append(f"Ascon-128-MAC ({s.get('signer', 'krel')}, key={key_id.hex()})")
    if require_auth and not verified:
        raise KfwError("Release manifest has no trusted signature (provide --trust-store, --lms-pubkey, or --mac-key)")
    return verified


def verify_auth(
    parsed: ParsedKfw,
    *,
    mac_key: bytes | None = None,
    lms_pubkey: bytes | None = None,
    trust_store: dict[str, Any] | None = None,
    channel: str | None = None,
    require_auth: bool = False,
) -> list[str]:
    verified: list[str] = []
    unpinned: list[str] = []
    must_verify = require_auth or bool(parsed.flags & HCRIT_AUTH_REQUIRED)

    for tag, val in parsed.auth_tlvs:
        if tag == TLV_AUTH_ASCON_MAC:
            if len(val) != 24:
                raise KfwError("Invalid TLV_AUTH_ASCON_MAC length")
            key_id, signer_b, sig_tag = val[0:4], val[4:8], val[8:24]
            signer = signer_b.rstrip(b"\x00").decode("ascii", errors="replace")
            if mac_key is not None:
                expected_id = ascon_hash256(mac_key)[:4]
                if hmac.compare_digest(key_id, expected_id):
                    nonce = b"KFW-MAC-V1\x00\x00" + key_id
                    expected_tag = ascon128_mac(mac_key, nonce, signer_b + parsed.root_digest, b"")
                    if not hmac.compare_digest(sig_tag, expected_tag):
                        raise KfwError(f"Ascon-128-MAC verification failed for signer '{signer}'")
                    verified.append(f"Ascon-128-MAC ({signer}, key={key_id.hex()})")
        elif tag == TLV_AUTH_LMS_ASCON:
            pk, signer, leaf_idx = lms_verify(val, parsed.root_digest, pinned_pubkey=lms_pubkey)
            if lms_pubkey is not None:
                verified.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, pinned={pk[:4].hex()}..)")
            elif trust_store is not None:
                entry = lookup_trusted_lms_key(pk, trust_store, channel=channel)
                if entry is not None:
                    verified.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, trusted={entry.get('key_id_hex', pk[:4].hex())})")
                else:
                    unpinned.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, UNTRUSTED pk={pk[:4].hex()}..)")
            else:
                unpinned.append(f"LMS-Ascon PQ ({signer}, leaf={leaf_idx}, UNPINNED pk={pk[:4].hex()}..)")

    if must_verify and not verified:
        if unpinned:
            raise KfwError(
                f"Container authentication failed: signature not anchored to a trusted key ({'; '.join(unpinned)}). "
                "Supply --trust-store or --lms-pubkey / --mac-key."
            )
        raise KfwError("Container requires authentication but no trusted auth record verified")
    return verified if verified else unpinned


def extract_all_parts(parsed: ParsedKfw, decrypt_key: bytes | None = None) -> dict[int, bytes]:
    """Decompresses/decrypts and verifies every part's 128-bit Ascon plaintext digest + anti-brick invariants."""
    plaintexts: dict[int, bytes] = {}
    solid_plain: bytes | None = None

    # 1. Process non-solid parts (including PART_SOLID_ARENA)
    for p in parsed.parts:
        if p["in_solid"]:
            continue
        raw_stored = parsed.raw[p["offset"] : p["offset"] + p["stored_size"]]
        if p["encrypted"]:
            if decrypt_key is None:
                raise KfwError(f"Part {p['index']} is encrypted; supply --decrypt-key")
            if len(raw_stored) < 32:
                raise KfwError(f"Encrypted part {p['index']} missing 32 B Ascon-AEAD128 envelope")
            nonce, tag, ct = raw_stored[0:16], raw_stored[16:32], raw_stored[32:]
            part_hdr16 = struct.pack(
                "<BBBBIII", p["kind"], p["flags"], p["target_idx"], p["slot_or_policy"],
                p["offset"], p["stored_size"], p["raw_size"],
            )
            raw_stored = ascon128_decrypt(decrypt_key, nonce, b"KFW-ENC-V1\x00" + part_hdr16, ct, tag)
        lprop = parsed.solid_params.get(p["index"], (DEFAULT_LZMA2_PROP, 0, 0, 0))[0]
        pt = decompress_payload(raw_stored, p["comp"], p["raw_size"], lzma2_prop=lprop)
        if not hmac.compare_digest(ascon_hash256(pt)[:16], p["digest"]):
            raise KfwError(f"Part {p['index']} ({PART_KIND_NAMES.get(p['kind'], p['kind'])}) plaintext Ascon digest mismatch")
        plaintexts[p["index"]] = pt
        if p["kind"] == PART_SOLID_ARENA:
            solid_plain = pt

    # 2. Slice and verify IN_SOLID members
    for p in parsed.parts:
        if not p["in_solid"]:
            continue
        if solid_plain is None:
            raise KfwError(f"Part {p['index']} marked IN_SOLID but no PART_SOLID_ARENA present")
        end = p["offset"] + p["raw_size"]
        if end > len(solid_plain):
            raise KfwError(f"IN_SOLID part {p['index']} slice [{p['offset']}..{end}] exceeds solid arena {len(solid_plain)}")
        pt = solid_plain[p["offset"] : end]
        if not hmac.compare_digest(ascon_hash256(pt)[:16], p["digest"]):
            raise KfwError(f"IN_SOLID part {p['index']} plaintext Ascon digest mismatch")
        plaintexts[p["index"]] = pt

    # 3. Anti-brick checks on all firmware/bootloader/KRX parts
    for p in parsed.parts:
        pidx = p["index"]
        if p["kind"] in (PART_FIRMWARE, PART_BOOTLOADER):
            if p["target_idx"] >= len(parsed.targets):
                raise KfwError(f"Firmware part {pidx} has invalid target_idx {p['target_idx']}")
            if pidx not in parsed.part_addrs:
                raise KfwError(f"Firmware part {pidx} missing critical TLV_PART_ADDR")
            tgt = parsed.targets[p["target_idx"]]
            bus, fwf, load_addr, sp, pc = parsed.part_addrs[pidx]
            fw = plaintexts[pidx]
            if p["kind"] == PART_FIRMWARE and bus == BUS_INTERNAL_FLASH and len(fw) > tgt.flash_max:
                raise KfwError(f"Firmware part {pidx} size {len(fw)} > target max {tgt.flash_max}")
            for pr in parsed.protect_ranges:
                if pr.target_idx in (0xFF, p["target_idx"]) and pr.bus == bus:
                    if p["kind"] == PART_BOOTLOADER and (pr.flags & 0x0002) == 0:
                        continue
                    if load_addr < pr.end and (load_addr + len(fw)) > pr.start:
                        raise KfwError(f"Firmware part {pidx} violates protected range [0x{pr.start:08X}..0x{pr.end:08X}]")
            if fwf & FWFLAG_HAS_VECTORS:
                validate_arm_vectors(fw, load_addr, tgt.sram_kb, expected_sp=sp, expected_pc=pc)
        elif p["kind"] in (PART_KRX_MODULE, PART_OPT_KRX):
            meta = parsed.part_paths.get(pidx)
            if not meta:
                raise KfwError(f"KRX part {pidx} missing TLV_PART_PATH")
            if meta["min_abi"] and parsed.krx_abi_ver:
                if parsed.krx_abi_ver < meta["min_abi"] or (meta["max_abi"] and parsed.krx_abi_ver > meta["max_abi"]):
                    raise KfwError(f"KRX module {meta['path']} ABI incompatible with firmware")

    return plaintexts


def select_firmware_for_target(
    kfw_path: str | Path,
    target_slug: str | None = None,
    *,
    mac_key: bytes | None = None,
    lms_pubkey: bytes | None = None,
    decrypt_key: bytes | None = None,
) -> tuple[bytes, TargetSpec, ParsedKfw]:
    """High-level verified loader used by `tools/flasher.py`."""
    data = Path(kfw_path).read_bytes()
    parsed = parse_kfw(data, verify_root=True)
    if parsed.kind in (KIND_BACKUP, KIND_MODULE_PACK):
        raise KfwError(f"Refusing to flash container kind '{KIND_NAMES[parsed.kind]}' to MCU internal flash")
    verify_auth(parsed, mac_key=mac_key, lms_pubkey=lms_pubkey)
    plaintexts = extract_all_parts(parsed, decrypt_key=decrypt_key)

    fw_parts = [p for p in parsed.parts if p["kind"] == PART_FIRMWARE]
    if not fw_parts:
        raise KfwError("Container contains no PART_FIRMWARE image")

    chosen = None
    if target_slug:
        want = target_slug.lower()
        for p in fw_parts:
            t = parsed.targets[p["target_idx"]]
            if want in t.slug.lower() or (want in ("uv-k1", "uv-k5v3") and t.target_id == 0x51530101):
                chosen = p
                break
        if chosen is None:
            avail = ", ".join(parsed.targets[p["target_idx"]].slug for p in fw_parts)
            raise KfwError(f"Target '{target_slug}' not found in .kfw (available: {avail})")
    elif len(fw_parts) == 1:
        chosen = fw_parts[0]
    else:
        # Default to primary UV-K1/K5v3 if present, otherwise require explicit --target
        for p in fw_parts:
            if parsed.targets[p["target_idx"]].target_id == 0x51530101:
                chosen = p
                break
        if chosen is None:
            avail = ", ".join(parsed.targets[p["target_idx"]].slug for p in fw_parts)
            raise KfwError(f"Multi-target .kfw requires --target ({avail})")

    tgt = parsed.targets[chosen["target_idx"]]
    return plaintexts[chosen["index"]], tgt, parsed


# =============================================================================
# 7. CLI COMMANDS
# =============================================================================

def _parse_hex_key(s: str | None, expected_len: int) -> bytes | None:
    if not s:
        return None
    p = Path(s)
    raw = p.read_text().strip() if p.exists() and p.is_file() else s.strip()
    b = bytes.fromhex(raw)
    if len(b) != expected_len:
        raise KfwError(f"Expected {expected_len}-byte hex key, got {len(b)} bytes")
    return b


def _git_short_commit() -> str:
    try:
        return subprocess.check_output(["git", "rev-parse", "--short=12", "HEAD"], stderr=subprocess.DEVNULL).decode().strip()
    except Exception:
        return "000000000000"


def _make_target_entries(slug_key: str, tidx: int) -> tuple[TargetSpec, BootPolicySpec, list[ProtectRangeSpec]]:
    k = slug_key.lower()
    if k not in BUILTIN_TARGETS:
        raise KfwError(f"Unknown target '{slug_key}' (supported: uv-k1, uv-k5v3, uv-k10pro)")
    prof = BUILTIN_TARGETS[k]
    t = TargetSpec(
        target_id=prof["target_id"], mcu_id=prof["mcu_id"], arch_rf=prof["arch_rf"],
        transport_disp=prof["transport_disp"], flash_base=prof["flash_base"],
        flash_max=prof["flash_max"], page_size=prof["page_size"], sram_kb=prof["sram_kb"],
        hw_rev_mask=prof["hw_rev_mask"], ext_flash_kb=prof["ext_flash_kb"], slug=prof["slug"],
    )
    bp = BootPolicySpec(
        target_idx=tidx, boot_family=prof["boot_family"], max_attempts=3, confirm_sec=15,
        min_boot_ver=prof["min_boot_ver"], max_boot_ver=prof["max_boot_ver"],
        min_battery_mv=prof["min_battery_mv"], min_fs_schema=1,
        usb_vid=prof["usb_vid"], usb_pid=prof["usb_pid"], uart_baud=prof["uart_baud"],
        policy_flags=(
            BPFLAG_PRESERVE_FACTORY_PAGE | BPFLAG_FORBID_RDP2 | BPFLAG_ALLOW_ROLLBACK
            | BPFLAG_REQUIRE_CLEAN_QUIESCE | BPFLAG_SKIP_IF_DIGEST_MATCH
        ),
        bl_prefix=prof["bl_prefix"],
    )
    prs = [ProtectRangeSpec(tidx, bus, fl, st, en) for bus, fl, st, en in prof["protect_ranges"]]
    return t, bp, prs


def cmd_pack(args: argparse.Namespace) -> int:
    targets: list[TargetSpec] = []
    boot_policies: list[BootPolicySpec] = []
    protect_ranges: list[ProtectRangeSpec] = []
    target_map: dict[str, int] = {}
    parts: list[PartInput] = []

    use_solid = not args.no_solid and not args.mcu_ota
    comp = COMP_PACKBITS_RLE if args.mcu_ota else COMP_ARMTHUMB_LZMA2
    if args.compression:
        comp_lookup = {v: k for k, v in COMP_NAMES.items()}
        comp = comp_lookup[args.compression]

    for spec in args.fw or []:
        if ":" not in spec:
            raise KfwError(f"--fw must be TARGET:PATH (got {spec!r})")
        tname, fpath = spec.split(":", 1)
        tkey = tname.lower()
        if tkey not in target_map:
            tidx = len(targets)
            t, bp, prs = _make_target_entries(tkey, tidx)
            targets.append(t)
            boot_policies.append(bp)
            protect_ranges.extend(prs)
            target_map[tkey] = tidx
        else:
            tidx = target_map[tkey]

        fw_bytes = Path(fpath).read_bytes()
        if fpath.endswith("_packed.bin"):
            raise KfwError("Pass raw plaintext firmware.bin to --fw (never XOR-packed *_packed.bin)")
        parts.append(PartInput(
            kind=PART_FIRMWARE, data=fw_bytes, target_idx=tidx, slot_or_policy=0,
            comp=comp, in_solid=use_solid, page_aligned=not use_solid,
            mcu_streamable=args.mcu_ota,
        ))

    for spec in args.mod or []:
        toks = spec.split(":")
        mpath = Path(toks[0])
        dest = toks[1] if len(toks) >= 2 and toks[1] else f"/apps/{mpath.name}"
        pol = POLICY_BY_NAME[toks[2]] if len(toks) >= 3 and toks[2] else POLICY_ALWAYS
        mbytes = mpath.read_bytes()
        kinfo = inspect_krx_binary(mbytes)
        parts.append(PartInput(
            kind=PART_KRX_MODULE, data=mbytes, target_idx=0xFF, slot_or_policy=pol,
            comp=comp, in_solid=use_solid, post_flash=True, mcu_streamable=args.mcu_ota,
            dest_path=dest, file_type=8, krx_profile=kinfo["profile"],
            min_abi=kinfo["min_abi"], krx_caps=kinfo["caps"], pkg_ver=kinfo["pkg_ver"],
        ))

    for spec in args.file or []:
        toks = spec.split(":")
        if len(toks) < 2:
            raise KfwError(f"--file must be LOCAL_PATH:KERNFS_DEST[:POLICY] (got {spec!r})")
        fpath, dest = Path(toks[0]), toks[1]
        pol = POLICY_BY_NAME[toks[2]] if len(toks) >= 3 else POLICY_IF_MISSING
        parts.append(PartInput(
            kind=PART_KERNFS_FILE, data=fpath.read_bytes(), target_idx=0xFF, slot_or_policy=pol,
            comp=comp, in_solid=use_solid, post_flash=True, mcu_streamable=args.mcu_ota,
            dest_path=dest, file_type=0,
        ))

    if args.notes:
        parts.append(PartInput(
            kind=PART_RELEASE_NOTES, data=Path(args.notes).read_bytes(),
            comp=comp, in_solid=use_solid,
        ))

    kind_map = {v.lower(): k for k, v in KIND_NAMES.items()}
    if args.kind:
        c_kind = kind_map[args.kind.lower()]
    elif len([p for p in parts if p.kind == PART_FIRMWARE]) == 0:
        c_kind = KIND_MODULE_PACK
    elif len(parts) > 1:
        c_kind = KIND_BUNDLE
    else:
        c_kind = KIND_FIRMWARE

    hflags = HFLAG_REQUIRE_CALIB_BACKUP | HFLAG_PRESERVE_BOOTLOADER | HFLAG_VERIFY_READBACK | HFLAG_REPRODUCIBLE
    if args.mcu_ota:
        hflags |= HFLAG_MCU_OTA
    if "-" in args.version:
        hflags |= HFLAG_PRERELEASE

    manifest = {
        "schema": 1,
        "name": args.name,
        "version": args.version,
        "license": "GPL-3.0-or-later WITH KRX-Syscall-Exception",
        "git_commit": _git_short_commit(),
        "targets": [t.slug for t in targets],
    }

    mac_key = _parse_hex_key(args.mac_key, 16)
    enc_key = _parse_hex_key(args.encrypt_key, 16)
    lms_obj = json.loads(Path(args.lms_key).read_text()) if args.lms_key else None

    if enc_key and not use_solid:
        for p in parts:
            p.encrypt_key = enc_key

    out_bytes = build_kfw(
        kind=c_kind,
        fw_version=pack_semver(args.version),
        sec_cnt=args.sec_cnt,
        krx_abi_ver=pack_abi(args.krx_abi or detect_workspace_abi()),
        header_flags=hflags,
        targets=targets,
        boot_policies=boot_policies,
        protect_ranges=protect_ranges,
        parts=parts,
        manifest=manifest,
        solid_comp=comp,
        solid_encrypt_key=enc_key if use_solid else None,
        mac_key=mac_key,
        lms_key_obj=lms_obj,
        signer=args.signer,
    )

    if lms_obj is not None and args.lms_key:
        save_lms_key_atomic(args.lms_key, lms_obj)

    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(out_bytes)

    raw_total = sum(len(p.data) for p in parts)
    pct = (len(out_bytes) * 100.0 / raw_total) if raw_total else 0.0
    _log_ok(f"Packed {_BOLD}{out_path}{_RESET} ({len(out_bytes):,} B vs {raw_total:,} B raw = {pct:.1f}%)")
    return 0


def cmd_inspect(args: argparse.Namespace) -> int:
    data = Path(args.file).read_bytes()
    p = parse_kfw(data, verify_root=not args.no_verify)
    print(f"\n  ⚡ {_BOLD}{_CYAN}KFW Container{_RESET} — {args.file} ({len(data):,} B, body={p.body_size:,} B)")
    print("  ══════════════════════════════════════════════════════════════════")
    print(f"  Spec:       v{p.version >> 4}.{p.version & 0xF} | Kind: {_BOLD}{KIND_NAMES.get(p.kind, p.kind)}{_RESET}")
    print(f"  Version:    {format_semver(p.fw_version)} | sec_cnt={p.sec_cnt} | KRX-ABI={format_abi(p.krx_abi_ver)}")
    print(f"  Root Ascon: {_GRAY}{p.root_digest.hex()}{_RESET}")
    print(f"  Flags:      0x{p.flags:08X} | TLVs: {p.tlv_size} B | Parts: {len(p.parts)}")
    if p.targets:
        print("  Targets:")
        for i, t in enumerate(p.targets):
            print(f"    [{i}] {t.slug:<12} id=0x{t.target_id:08X} flash=0x{t.flash_base:08X}+{t.flash_max:,} B sram={t.sram_kb} KB")
    if p.protect_ranges:
        print("  Protected Fences:")
        for pr in p.protect_ranges:
            strict = " [STRICT]" if (pr.flags & 2) else ""
            print(f"    target={pr.target_idx} bus={pr.bus} 0x{pr.start:08X}..0x{pr.end:08X}{strict}")
    print("  Parts:")
    for pt in p.parts:
        kname = PART_KIND_NAMES.get(pt["kind"], f"0x{pt['kind']:02X}")
        loc = f"solid+0x{pt['offset']:X}" if pt["in_solid"] else f"@0x{pt['offset']:X} ({pt['stored_size']:,} B, {COMP_NAMES.get(pt['comp'], pt['comp'])})"
        extra = ""
        if pt["index"] in p.part_paths:
            extra = f" -> {p.part_paths[pt['index']]['path']} ({POLICY_NAMES.get(pt['slot_or_policy'], pt['slot_or_policy'])})"
        elif pt["index"] in p.part_addrs:
            _, _, laddr, _, _ = p.part_addrs[pt["index"]]
            extra = f" -> 0x{laddr:08X} (slot {pt['slot_or_policy']})"
        print(f"    [{pt['index']}] {kname:<13} raw={pt['raw_size']:>7,} B  {loc}  ascon128={pt['digest'].hex()[:16]}..{extra}")
    if p.auth_tlvs:
        print(f"  Auth Trailer ({len(p.auth_tlvs)} record(s)):")
        for tag, val in p.auth_tlvs:
            aname = "Ascon-128-MAC" if tag == TLV_AUTH_ASCON_MAC else ("LMS-Ascon PQ" if tag == TLV_AUTH_LMS_ASCON else f"0x{tag:04X}")
            print(f"    • {aname} (key_id={val[:4].hex()}, signer={val[4:8].rstrip(b'\\x00').decode('ascii', 'replace')})")
    if p.manifest:
        print(f"  Manifest:   {json.dumps(p.manifest, sort_keys=True)}")
    return 0


def cmd_verify(args: argparse.Namespace) -> int:
    data = Path(args.file).read_bytes()
    p = parse_kfw(data, verify_root=True)
    mac_key = _parse_hex_key(args.mac_key, 16)
    lms_pk = _parse_hex_key(args.lms_pubkey, 32)
    dec_key = _parse_hex_key(args.decrypt_key, 16)
    trust_store = load_trust_store(args.trust_store) if args.trust_store else None
    auths = verify_auth(
        p,
        mac_key=mac_key,
        lms_pubkey=lms_pk,
        trust_store=trust_store,
        channel=args.channel,
        require_auth=args.require_auth,
    )
    pts = extract_all_parts(p, decrypt_key=dec_key)
    _log_ok(f"Verified {_BOLD}{args.file}{_RESET} — Ascon-Hash256 root + {len(pts)} part digest(s) + anti-brick checks passed.")
    for a in auths:
        _log_info(f"Auth: {a}")
    return 0


def cmd_sign(args: argparse.Namespace) -> int:
    path = Path(args.file)
    data = path.read_bytes()
    p = parse_kfw(data, verify_root=True)
    trailer = bytearray()
    if args.mac_key:
        trailer.extend(make_mac_auth_tlv(p.root_digest, _parse_hex_key(args.mac_key, 16), args.signer))
    if args.lms_key:
        kp = Path(args.lms_key)
        kobj = json.loads(kp.read_text())
        trailer.extend(encode_tlv(TLV_AUTH_LMS_ASCON, lms_sign(kobj, p.root_digest, args.signer)))
        save_lms_key_atomic(kp, kobj)
    if not trailer:
        raise KfwError("Supply --mac-key and/or --lms-key")
    out_path = Path(args.output) if args.output else path
    out_path.write_bytes(data + bytes(trailer))
    _log_ok(f"Appended {len(trailer)} B auth trailer to {_BOLD}{out_path}{_RESET} (body & root_digest unchanged).")
    return 0


def cmd_trust(args: argparse.Namespace) -> int:
    store_path = Path(args.store) if args.store else default_trust_store_path()
    if args.trust_cmd == "list":
        if not store_path.is_file():
            _log_info(f"No trust store at {store_path} yet.")
            return 0
        store = load_trust_store(store_path)
        keys = store.get("keys", [])
        if not keys:
            _log_info(f"Trust store {store_path} contains 0 keys.")
            return 0
        _cli_ui.table(
            ("Key ID", "Signer", "Status", "Channels", "Algorithm", "Public Key"),
            (
                (
                    k.get("key_id_hex", "?"),
                    k.get("signer", "?"),
                    k.get("status", "active"),
                    ",".join(k.get("channels", ["*"])),
                    k.get("alg", "lms-ascon"),
                    str(k.get("pubkey_hex", ""))[:16] + "…",
                )
                for k in keys
            ),
        )
        return 0
    if args.trust_cmd == "add":
        store = load_trust_store(store_path) if store_path.is_file() else {"schema": 1, "keys": []}
        if args.key_file:
            kobj = json.loads(Path(args.key_file).read_text(encoding="utf-8"))
            pk_hex = str(kobj["pubkey_hex"]).lower()
            signer = args.signer or str(kobj.get("signer", "krel"))
            alg = str(kobj.get("alg", "lms-ascon"))
        elif args.pubkey:
            pk = _parse_hex_key(args.pubkey, 32)
            assert pk is not None
            pk_hex = pk.hex()
            signer = args.signer or "krel"
            alg = "lms-ascon"
        else:
            raise KfwError("Supply --key-file (JSON) or --pubkey (64-char hex)")
        key_id_hex = ascon_hash256(bytes.fromhex(pk_hex))[:4].hex()
        channels = [c.strip() for c in (args.channels or "stable,beta,nightly").split(",") if c.strip()]
        keys = [k for k in store["keys"] if str(k.get("pubkey_hex", "")).lower() != pk_hex]
        keys.append({
            "key_id_hex": key_id_hex,
            "signer": signer[:4],
            "alg": alg,
            "pubkey_hex": pk_hex,
            "channels": channels,
            "status": "active",
            "description": args.description or "",
        })
        store["keys"] = keys
        save_trust_store(store_path, store)
        _log_ok(f"Added trusted key {key_id_hex} ({signer[:4]}, channels={','.join(channels)}) to {store_path}")
        return 0
    if args.trust_cmd == "revoke":
        store = load_trust_store(store_path)
        needle = args.key_id.strip().lower()
        found = False
        for k in store["keys"]:
            if str(k.get("key_id_hex", "")).lower() == needle or str(k.get("pubkey_hex", "")).lower().startswith(needle):
                k["status"] = "revoked"
                found = True
        if not found:
            raise KfwError(f"Key '{args.key_id}' not found in {store_path}")
        save_trust_store(store_path, store)
        _log_ok(f"Revoked key {args.key_id} in {store_path}")
        return 0
    raise KfwError(f"Unknown trust subcommand: {args.trust_cmd}")


def cmd_keygen(args: argparse.Namespace) -> int:
    if args.type == "mac":
        key = secrets.token_bytes(16)
        out = {"alg": "ascon-128-mac", "key_hex": key.hex(), "key_id_hex": ascon_hash256(key)[:4].hex()}
    else:
        out = lms_keygen(args.signer, height=args.height)
    if args.output:
        Path(args.output).write_text(json.dumps(out, indent=2) + "\n")
        _log_ok(f"Wrote {out['alg']} key to {_BOLD}{args.output}{_RESET} (key_id={out['key_id_hex']})")
        if "pubkey_hex" in out:
            _log_info(f"Public key (32 B): {out['pubkey_hex']}")
    else:
        print(json.dumps(out, indent=2))
    return 0


def cmd_extract(args: argparse.Namespace) -> int:
    data = Path(args.file).read_bytes()
    p = parse_kfw(data, verify_root=True)
    dec_key = _parse_hex_key(args.decrypt_key, 16)
    pts = extract_all_parts(p, decrypt_key=dec_key)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    for part in p.parts:
        pidx = part["index"]
        if part["kind"] == PART_SOLID_ARENA:
            continue
        if part["kind"] == PART_FIRMWARE:
            tgt = p.targets[part["target_idx"]]
            if args.target and args.target.lower() not in tgt.slug.lower():
                continue
            fw = pts[pidx]
            dest = out_dir / f"firmware_{tgt.slug}.bin"
            dest.write_bytes(xor_quansheng_wire(fw) if args.xor else fw)
            print(f"  Extracted {dest} ({dest.stat().st_size:,} B)")
        elif pidx in p.part_paths:
            rel = p.part_paths[pidx]["path"].lstrip("/")
            dest = out_dir / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(pts[pidx])
            print(f"  Extracted {dest} ({len(pts[pidx]):,} B)")
    return 0


def cmd_selftest(_args: argparse.Namespace) -> int:
    # 1. Cross-check Ascon vectors bit-for-bit against `src/lib/crypto/src/ascon.rs`
    assert ascon_hash256(b"") == bytes.fromhex(
        "7346bc14f036e87ae03d0997913088f5f68411434b3cf8b54fa796a80d251f91"
    ), "AsconHash256 empty vector mismatch!"
    _, tag0 = ascon128_encrypt(bytes(16), bytes(16), b"", b"")
    assert tag0 == bytes.fromhex("42213f50a811d2d1d7e4092aa2a42ba4"), "Ascon128 zero vector mismatch!"
    _, tag_inc = ascon128_encrypt(bytes(range(16)), bytes(range(16)), b"", b"")
    assert tag_inc == bytes.fromhex("e355159f292911f794cb1432a0103a8a"), "Ascon128 inc vector mismatch!"
    for n in (0, 1, 7, 8, 9, 15, 16, 33):
        msg = bytes((i * 13 + 5) & 0xFF for i in range(n))
        ct, tg = ascon128_encrypt(b"\x5A" * 16, b"\xA5" * 16, b"KFW-AAD", msg)
        assert ascon128_decrypt(b"\x5A" * 16, b"\xA5" * 16, b"KFW-AAD", ct, tg) == msg

    # 2. PackBits RLE roundtrip
    sample = b"AAAAABBCDEFGGGGGGGGGG" + bytes(range(64))
    assert packbits_decode(packbits_encode(sample), len(sample)) == sample

    # 3. Synthetic valid ARMv6-M firmware + KRX module + Solid Arena + MAC + PQ LMS-Ascon + Encryption
    fake_fw = bytearray(struct.pack("<IIII", 0x20004000, 0x08002841, 0x08002851, 0x08002861))
    fake_fw.extend(b"\x00\xBF" * 4096)
    fake_fw_bytes = bytes(fake_fw)
    fake_mod = b"\xC0\xDE\x01\xFE" + b"\x00" * 124

    t0, bp0, prs0 = _make_target_entries("uv-k1", 0)
    mac_key = bytes(range(16))
    enc_key = bytes(range(16, 32))
    lms_k = lms_keygen("test", height=4)
    lms_pk = bytes.fromhex(lms_k["pubkey_hex"])

    kfw_blob = build_kfw(
        kind=KIND_BUNDLE,
        fw_version=pack_semver("0.2.0-rc.1"),
        sec_cnt=42,
        targets=[t0],
        boot_policies=[bp0],
        protect_ranges=prs0,
        parts=[
            PartInput(kind=PART_FIRMWARE, data=fake_fw_bytes, target_idx=0, in_solid=True),
            PartInput(kind=PART_KRX_MODULE, data=fake_mod, dest_path="/apps/dfu.mod", in_solid=True, post_flash=True),
        ],
        manifest={"name": "Kern Test", "version": "0.2.0-rc.1"},
        solid_encrypt_key=enc_key,
        mac_key=mac_key,
        lms_key_obj=lms_k,
    )

    parsed = parse_kfw(kfw_blob, verify_root=True)
    auths = verify_auth(parsed, mac_key=mac_key, lms_pubkey=lms_pk, require_auth=True)
    assert len(auths) == 2
    pts = extract_all_parts(parsed, decrypt_key=enc_key)
    assert pts[1] == fake_fw_bytes and pts[2] == fake_mod

    # 4. Tamper detection: flipping 1 bit in body must fail root_digest
    bad = bytearray(kfw_blob)
    bad[100] ^= 0x01
    try:
        parse_kfw(bytes(bad), verify_root=True)
        raise AssertionError("Tampered body was not rejected!")
    except KfwError:
        pass

    # 5. Anti-brick: protected QS-1 range overlap must be rejected
    try:
        build_kfw(
            kind=KIND_FIRMWARE, fw_version=pack_semver("0.1.0"),
            targets=[t0], boot_policies=[bp0], protect_ranges=prs0,
            parts=[PartInput(kind=PART_FIRMWARE, data=fake_fw_bytes, target_idx=0, load_addr=0x08002700)],
        )
        raise AssertionError("QS-1 factory page overlap was not rejected!")
    except KfwError:
        pass

    # 6. Anti-brick: path traversal onto /sys/calib.dat must be rejected
    try:
        validate_kernfs_path("/sys/calib.dat", allow_calib=False)
        raise AssertionError("Calibration overwrite path was not rejected!")
    except KfwError:
        pass
    try:
        validate_kernfs_path("/apps/../sys/calib.dat", allow_calib=False)
        raise AssertionError("Path traversal was not rejected!")
    except KfwError:
        pass

    # 7. Anti-brick: corrupted ARM vector table (bad SP) must be rejected
    try:
        bad_fw = struct.pack("<IIII", 0x00001234, 0x08002841, 0x08002851, 0x08002861) + b"\x00" * 64
        validate_arm_vectors(bad_fw, 0x08002800, 16)
        raise AssertionError("Invalid ARM SP was not rejected!")
    except KfwError:
        pass

    # 8. Real KRX ABI v5 inspection + Trust Store verification, unpinned rejection, revocation, & manifest signing
    k_body_id = struct.pack("<HBBB", 0, 1, 2, 3) + bytes(35)
    k_body_exec = bytes(26) + bytes([0x11]) + bytes(1)
    k_body_rt = bytes([5, 0, 0, 0, 0, 0, 0, 0]) + struct.pack("<I", 0x0014_0620)
    k_secs = (
        struct.pack("<HH", 0x0001, 4 + len(k_body_id)) + k_body_id
        + struct.pack("<HH", 0x0002, 4 + len(k_body_exec)) + k_body_exec
        + struct.pack("<HH", 0x0009, 4 + len(k_body_rt)) + k_body_rt
    )
    k_hdr_len = 24 + len(k_secs)
    k_pre = struct.pack("<4sBBHHHIII", b"\xC0\xDE\x01\xFE", 1, 5, 3, k_hdr_len, 0, 0, zlib.crc32(k_secs) & 0xFFFFFFFF, 0)
    k_info = inspect_krx_binary(k_pre + k_secs)
    assert k_info["min_abi"] == 0x0500, f"Expected ABI 5.0 (0x0500), got {k_info['min_abi']:#x}"
    assert k_info["profile"] == 0x11 and k_info["caps"] == 0x0014_0620

    # Unpinned self-signed LMS must be rejected when require_auth=True
    try:
        verify_auth(parsed, require_auth=True)
        raise AssertionError("Unpinned self-signed LMS-Ascon was not rejected under require_auth=True!")
    except KfwError:
        pass

    # Active trust-store entry must verify, and revoked entry must fail
    t_store = {
        "schema": 1,
        "keys": [{
            "key_id_hex": lms_k["key_id_hex"],
            "signer": "test",
            "alg": lms_k["alg"],
            "pubkey_hex": lms_k["pubkey_hex"],
            "channels": ["stable"],
            "status": "active",
        }],
    }
    ts_auths = verify_auth(parsed, trust_store=t_store, channel="stable", require_auth=True)
    assert ts_auths and "trusted=" in ts_auths[0]

    # Manifest signing & verification roundtrip
    man_bytes = json.dumps({"version": "0.2.0-rc.1", "sec_cnt": 42}, sort_keys=True, separators=(",", ":")).encode()
    man_sig = sign_manifest_bytes(man_bytes, lms_key_obj=lms_k, mac_key=mac_key, signer="test")
    m_auths = verify_manifest_bytes(man_bytes, man_sig, trust_store=t_store, channel="stable", require_auth=True)
    assert len(m_auths) == 1

    # Revoked key must be rejected immediately
    t_store["keys"][0]["status"] = "revoked"
    try:
        verify_auth(parsed, trust_store=t_store, require_auth=True)
        raise AssertionError("Revoked LMS key was not rejected!")
    except KfwError as exc:
        assert "REVOKED" in str(exc)

    _log_ok("All .kfw selftests passed (Ascon-Hash256, Ascon-AEAD128, Ascon-128-MAC, PQ LMS-Ascon, Trust Store, Manifest Sig, KRX ABI v5, Solid ARMTHUMB+LZMA2, RLE, anti-brick guards).")
    return 0


# =============================================================================
# 8. SAFE TWO-PHASE UPGRADE & UPDATE CHECKER
# =============================================================================

def _fetch_bytes_or_file(src: str, timeout: float = 20.0) -> tuple[bytes, str]:
    """Read raw bytes from a local path or http(s):// URL."""
    if src.startswith(("http://", "https://")):
        import urllib.request
        req = urllib.request.Request(src, headers={"User-Agent": "KernKfw/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.read(), src
    p = Path(src)
    if not p.is_file():
        raise KfwError(f"File not found: {p}")
    return p.read_bytes(), str(p)


def check_for_updates(
    source: str,
    *,
    current_version: str | None = None,
    current_sec_cnt: int = 0,
    channel: str = "stable",
    trust_store: dict[str, Any] | None = None,
    lms_pubkey: bytes | None = None,
    mac_key: bytes | None = None,
    require_auth: bool = False,
) -> dict[str, Any]:
    """Inspect a signed `release-manifest.json` (URL/file) or GitHub `owner/repo` release feed."""
    if not source.startswith(("http://", "https://")) and not Path(source).is_file() and "/" in source:
        import urllib.request
        api_url = f"https://api.github.com/repos/{source.strip().strip('/')}/releases?per_page=10"
        req = urllib.request.Request(
            api_url,
            headers={"Accept": "application/vnd.github+json", "User-Agent": "KernKfw/1.0"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            releases = json.loads(resp.read().decode("utf-8"))
        for rel in releases:
            if channel == "stable" and rel.get("prerelease"):
                continue
            assets = {str(a.get("name", "")): a for a in rel.get("assets", [])}
            if "release-manifest.json" in assets:
                return check_for_updates(
                    str(assets["release-manifest.json"]["browser_download_url"]),
                    current_version=current_version,
                    current_sec_cnt=current_sec_cnt,
                    channel=channel,
                    trust_store=trust_store,
                    lms_pubkey=lms_pubkey,
                    mac_key=mac_key,
                    require_auth=require_auth,
                )
            kfw_assets = [a for n, a in assets.items() if n.lower().endswith(".kfw")]
            if kfw_assets:
                tag = str(rel.get("tag_name", "")).lstrip("v")
                newer = True
                if current_version:
                    try:
                        newer = pack_semver(tag) > pack_semver(current_version)
                    except KfwError:
                        newer = tag != current_version
                return {
                    "source": source,
                    "channel": "prerelease" if rel.get("prerelease") else "stable",
                    "version": tag,
                    "sec_cnt": 0,
                    "update_available": newer,
                    "manifest_verified": False,
                    "auth": [],
                    "assets": [
                        {"filename": a["name"], "url": a["browser_download_url"], "size": int(a.get("size", 0))}
                        for a in kfw_assets
                    ],
                }
        raise KfwError(f"No matching '{channel}' releases found on GitHub repo {source}")

    raw, resolved = _fetch_bytes_or_file(source)
    manifest = json.loads(raw.decode("utf-8"))
    auth_notes: list[str] = []
    manifest_verified = False
    sig_src = source + ".sig"
    sig_raw: bytes | None = None
    try:
        sig_raw, _ = _fetch_bytes_or_file(sig_src, timeout=10.0)
    except Exception:
        sig_raw = None

    if sig_raw is not None:
        sig_env = json.loads(sig_raw.decode("utf-8"))
        auth_notes = verify_manifest_bytes(
            raw,
            sig_env,
            lms_pubkey=lms_pubkey,
            mac_key=mac_key,
            trust_store=trust_store,
            channel=manifest.get("channel") or channel,
            require_auth=require_auth,
        )
        manifest_verified = bool(auth_notes) and all("UNPINNED" not in a for a in auth_notes)
    elif require_auth:
        raise KfwError(f"Release manifest signature missing: {sig_src}")

    ver = str(manifest.get("version", "0.0.0"))
    sec_cnt = int(manifest.get("sec_cnt", 0))
    newer = True
    if current_version:
        try:
            newer = (pack_semver(ver) > pack_semver(current_version)) or (sec_cnt > current_sec_cnt)
        except KfwError:
            newer = ver != current_version
    return {
        "source": resolved,
        "channel": str(manifest.get("channel", channel)),
        "version": ver,
        "sec_cnt": sec_cnt,
        "krx_abi": str(manifest.get("krx_abi", "")),
        "update_available": newer,
        "manifest_verified": manifest_verified,
        "auth": auth_notes,
        "assets": manifest.get("assets", []),
    }


def backup_radio_over_rigwire(client: Any, out_path: Path) -> Path:
    """Pull calibration + /pref/*.cfg over RigWire (v1 or v2) into an Ascon-verified `.kfw` backup."""
    paths = ["/sys/calib.dat", "/sys/calib.sig"]
    try:
        for ent in client.ls("/pref"):
            if not ent.get("is_dir"):
                paths.append(f"/pref/{ent['name']}")
    except Exception:
        pass

    parts: list[PartInput] = []
    for rpath in paths:
        try:
            st = client.stat(rpath)
            if not st.get("exists") or st.get("is_dir") or int(st.get("size", 0)) <= 0:
                continue
            blob = client.pull_bytes(rpath, quiet=True)
            pkind = PART_CALIBRATION if rpath == "/sys/calib.dat" else PART_KERNFS_FILE
            parts.append(PartInput(kind=pkind, data=blob, dest_path=rpath, in_solid=True))
        except Exception:
            continue

    if not parts:
        raise KfwError("Could not read any calibration or preference files for backup")
    t0, bp0, prs0 = _make_target_entries("uv-k1", 0)
    kfw_blob = build_kfw(
        kind=KIND_BACKUP,
        fw_version=pack_semver("1.0.0"),
        krx_abi_ver=pack_abi(detect_workspace_abi()),
        header_flags=HFLAG_PRESERVE_BOOTLOADER | HFLAG_REPRODUCIBLE,
        targets=[t0],
        boot_policies=[bp0],
        protect_ranges=prs0,
        parts=parts,
        manifest={"schema": 1, "name": "Pre-Upgrade Backup", "files": [p.dest_path for p in parts]},
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(kfw_blob)
    return out_path


def provision_kfw_parts_over_rigwire(
    client: Any,
    parsed: ParsedKfw,
    plaintexts: dict[int, bytes],
    *,
    dry_run: bool = False,
) -> dict[str, list[str]]:
    """Apply Phase B `POST_FLASH` KRX modules & KernFS files over RigWire per part policy (CRC-32 delta)."""
    report: dict[str, list[str]] = {"installed": [], "skipped": [], "deleted": [], "verified": []}
    for part in parsed.parts:
        pidx = part["index"]
        if part["kind"] not in (PART_KRX_MODULE, PART_KERNFS_FILE, PART_OPT_KRX, PART_OPT_FILE):
            continue
        meta = parsed.part_paths.get(pidx)
        if not meta:
            continue
        rpath = meta["path"]
        policy = part["slot_or_policy"]
        data = plaintexts[pidx]
        want_crc = zlib.crc32(data) & 0xFFFFFFFF

        st = client.stat(rpath)
        exists = bool(st.get("exists")) and not bool(st.get("is_dir"))
        same = exists and int(st.get("size", -1)) == len(data) and (int(st.get("crc32", 0)) & 0xFFFFFFFF) == want_crc

        if policy == POLICY_DELETE:
            if exists:
                if not dry_run:
                    client.rm(rpath)
                report["deleted"].append(rpath)
            else:
                report["skipped"].append(rpath)
            continue
        if policy == POLICY_VERIFY_ONLY:
            if not same:
                raise KfwError(f"Verify-only part mismatch on radio: {rpath}")
            report["verified"].append(rpath)
            continue
        if policy in (POLICY_IF_MISSING, POLICY_PRESERVE_USER) and exists:
            report["skipped"].append(rpath)
            continue
        if policy == POLICY_UPGRADE_ONLY and not exists:
            report["skipped"].append(rpath)
            continue
        if same:
            report["skipped"].append(rpath)
            continue

        if not dry_run:
            client.push_bytes(data, rpath)
            post = client.stat(rpath)
            if not post.get("exists") or int(post.get("size", -1)) != len(data) or (int(post.get("crc32", 0)) & 0xFFFFFFFF) != want_crc:
                raise KfwError(f"Post-install CRC-32 verification failed for {rpath}")
        report["installed"].append(rpath)
    return report


def _wait_for_port(vid_pid: str, preferred_port: str | None, timeout_s: float) -> str:
    import time
    from serial.tools import list_ports  # type: ignore[import-not-found]
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        ports = list(list_ports.comports())
        for p in ports:
            if p.vid is not None and p.pid is not None and f"{p.vid:04x}:{p.pid:04x}".lower() == vid_pid:
                return p.device
        if preferred_port and Path(preferred_port).exists():
            return preferred_port
        time.sleep(0.2)
    raise KfwError(f"Timed out waiting for USB device ({vid_pid}) after {timeout_s:.0f}s")


def cmd_check_update(args: argparse.Namespace) -> int:
    trust_store = None
    ts_path = Path(args.trust_store) if args.trust_store else default_trust_store_path()
    if ts_path.is_file():
        trust_store = load_trust_store(ts_path)
    info = check_for_updates(
        args.source,
        current_version=args.current_version,
        current_sec_cnt=args.current_sec_cnt,
        channel=args.channel,
        trust_store=trust_store,
        lms_pubkey=_parse_hex_key(args.lms_pubkey, 32),
        mac_key=_parse_hex_key(args.mac_key, 16),
        require_auth=args.require_auth,
    )
    if args.json:
        print(json.dumps(info, indent=2))
        return 0
    _cli_ui.header("Kern Update Check", f"Channel: {info['channel']} · Source: {info['source']}")
    _log_info(f"Latest version: {_BOLD}{info['version']}{_RESET} (sec_cnt={info['sec_cnt']}, ABI={info.get('krx_abi') or 'n/a'})")
    _log_info(f"Manifest trust: {'TRUSTED (' + '; '.join(info['auth']) + ')' if info['manifest_verified'] else 'unsigned / unpinned'}")
    if info["update_available"]:
        _log_ok("Update available!")
    else:
        _log_info("Already up to date.")
    for a in info.get("assets", []):
        print(f"    • {a.get('filename', '?')} ({int(a.get('size', 0)):,} B) {a.get('url', '')}")
    return 0


def cmd_upgrade(args: argparse.Namespace) -> int:
    import time
    import rigwire as _rigwire  # noqa: E402
    import flasher as _flasher  # noqa: E402

    raw, resolved = _fetch_bytes_or_file(args.file)
    parsed = parse_kfw(raw, verify_root=True)
    mac_key = _parse_hex_key(args.mac_key, 16)
    lms_pk = _parse_hex_key(args.lms_pubkey, 32)
    dec_key = _parse_hex_key(args.decrypt_key, 16)
    trust_store = None
    if args.trust_store:
        trust_store = load_trust_store(args.trust_store)
    elif default_trust_store_path().is_file() and not lms_pk:
        trust_store = load_trust_store(default_trust_store_path())

    auths = verify_auth(
        parsed,
        mac_key=mac_key,
        lms_pubkey=lms_pk,
        trust_store=trust_store,
        channel=args.channel,
        require_auth=args.require_auth,
    )
    plaintexts = extract_all_parts(parsed, decrypt_key=dec_key)

    ver_str = format_semver(parsed.fw_version)
    _cli_ui.header(f"Kern Safe Upgrade — v{ver_str}", f"Container: {resolved} (sec_cnt={parsed.sec_cnt}, ABI={format_abi(parsed.krx_abi_ver)})")
    for a in auths:
        _log_info(f"Auth: {a}")

    fw_parts = [p for p in parsed.parts if p["kind"] == PART_FIRMWARE]
    mod_parts = [p for p in parsed.parts if p["kind"] in (PART_KRX_MODULE, PART_KERNFS_FILE, PART_OPT_KRX, PART_OPT_FILE)]
    chosen_fw: bytes | None = None
    chosen_tgt: TargetSpec | None = None
    if fw_parts and not args.skip_flash:
        chosen_fw, chosen_tgt, _ = select_firmware_for_target(
            args.file if not args.file.startswith(("http://", "https://")) else _write_temp_kfw(raw),
            args.target,
            mac_key=mac_key,
            lms_pubkey=lms_pk,
            decrypt_key=dec_key,
        )
        _log_info(f"Phase A image: {chosen_tgt.slug} ({len(chosen_fw):,} B, vectors & fences verified)")
    if mod_parts and not args.skip_modules:
        _log_info(f"Phase B payloads: {len(mod_parts)} module/asset file(s) queued for CRC-32 delta sync")

    if args.dry_run:
        _log_ok("Dry-run pre-flight passed — no changes written to radio.")
        return 0

    port = args.port
    # Pre-flight RigWire check
    wire_online = False
    try:
        cli_port = port or _wait_for_port("36b7:ffff", port, 2.0)
        cli = _rigwire.RigWireClient(cli_port, timeout=1.0)
        try:
            cli.hello()
            wire_online = True
            _log_ok(f"Connected over RigWire on {cli_port}")
            telem = cli.get_system_telemetry()
            batt_mv = int(telem.get("battery_mv", 0))
            min_mv = max((bp.min_battery_mv for bp in parsed.boot_policies), default=0)
            if batt_mv > 0 and min_mv > 0 and batt_mv < min_mv and not args.ignore_battery:
                raise KfwError(f"Battery voltage {batt_mv} mV is below safe flash threshold {min_mv} mV (charge radio or pass --ignore-battery)")
            if batt_mv > 0:
                _log_info(f"Battery check OK: {batt_mv} mV (min {min_mv} mV)")

            if (parsed.flags & HFLAG_REQUIRE_CALIB_BACKUP) and not args.skip_backup and chosen_fw is not None:
                bdir = Path(args.backup_dir) if args.backup_dir else (Path.home() / ".local" / "share" / "kern" / "backups")
                ts = time.strftime("%Y%m%d-%H%M%S")
                serial_tag = str(telem.get("serial_str") or "radio").replace(" ", "_")
                bpath = backup_radio_over_rigwire(cli, bdir / f"kern-backup-{serial_tag}-{ts}.kfw")
                _log_ok(f"Saved pre-upgrade calibration/config backup -> {bpath}")

            if chosen_fw is not None:
                _log_info("Phase A: Rebooting radio into bootloader via RigWire...")
                cli.reboot(mode=1)
        finally:
            cli.close()
    except KfwError:
        raise
    except Exception as exc:
        if chosen_fw is None:
            raise KfwError(f"RigWire connection required for module-only update: {exc}") from exc
        _cli_ui.warn(f"RigWire not online ({exc}); expecting radio already in bootloader (PTT+Power-On)...")

    if chosen_fw is not None:
        if wire_online:
            time.sleep(0.8)
        bl_port = _wait_for_port("36b7:0001", port, 15.0)
        _log_info(f"Bootloader detected on {bl_port}; flashing {len(chosen_fw):,} B (applying wire XOR-16 on wire)...")
        proto = _flasher.Protocol(bl_port)
        proto.connect()
        try:
            _flasher.flash_process(proto, chosen_fw)
        finally:
            proto.close()
        _log_ok("Phase A firmware flash complete.")

    if mod_parts and not args.skip_modules:
        _log_info("Phase B: Waiting for upgraded radio to reconnect over RigWire...")
        if chosen_fw is not None:
            time.sleep(1.5)
        rw_port = _wait_for_port("36b7:ffff", port, 20.0)
        cli2 = _rigwire.RigWireClient(rw_port, timeout=1.5)
        try:
            cli2.hello()
            _log_ok(f"Reconnected over RigWire on {rw_port}")
            rep = provision_kfw_parts_over_rigwire(cli2, parsed, plaintexts, dry_run=False)
            _log_ok(
                f"Phase B sync complete: {len(rep['installed'])} installed, "
                f"{len(rep['skipped'])} unchanged, {len(rep['deleted'])} deleted."
            )
            for p_inst in rep["installed"]:
                print(f"    • updated {p_inst}")
        finally:
            cli2.close()

    _log_ok("Upgrade to Kern OS completed safely!")
    return 0


def _write_temp_kfw(raw: bytes) -> Path:
    import tempfile
    p = Path(tempfile.gettempdir()) / f"kern-upgrade-{os.getpid()}.kfw"
    p.write_bytes(raw)
    return p


def main() -> int:
    ap = argparse.ArgumentParser(description="Kern Firmware Container (.kfw) Tool")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_pack = sub.add_parser("pack", help="Pack a .kfw container")
    p_pack.add_argument("output", help="Output .kfw path")
    p_pack.add_argument("--name", default="Kern OS", help="Package display name")
    p_pack.add_argument("--version", default="0.1.0", help="SemVer (e.g. 0.1.0 or 0.2.0-rc.1)")
    p_pack.add_argument("--sec-cnt", type=int, default=0, help="Monotonic anti-rollback counter")
    p_pack.add_argument("--krx-abi", default=None, help="Exported KRX-ABI MAJOR.MINOR (default: auto from src/abi)")
    p_pack.add_argument("--kind", choices=[v.lower() for v in KIND_NAMES.values()], help="Explicit container kind")
    p_pack.add_argument("--fw", action="append", help="Firmware image as TARGET:PATH (repeatable, e.g. uv-k1:target/firmware.bin)")
    p_pack.add_argument("--mod", action="append", help="KRX module as PATH[:/apps/name.mod[:policy]] (repeatable)")
    p_pack.add_argument("--file", action="append", help="KernFS file as LOCAL:DEST[:policy] (repeatable)")
    p_pack.add_argument("--notes", help="Path to UTF-8 Markdown release notes")
    p_pack.add_argument("--compression", choices=list(COMP_NAMES.values()), help="Override compression codec")
    p_pack.add_argument("--no-solid", action="store_true", help="Store parts independently instead of Solid Arena")
    p_pack.add_argument("--mcu-ota", action="store_true", help="MCU-streamable mode (PackBits RLE, <= 4 KB RAM)")
    p_pack.add_argument("--encrypt-key", help="16-byte hex key (or file) for Ascon-AEAD128 encryption")
    p_pack.add_argument("--mac-key", help="16-byte hex key (or file) for Ascon-128-MAC auth")
    p_pack.add_argument("--lms-key", help="Path to LMS-Ascon private key JSON (updates next_leaf atomically)")
    p_pack.add_argument("--signer", default="krel", help="4-char ASCII signer tag")
    p_pack.set_defaults(func=cmd_pack)

    p_insp = sub.add_parser("inspect", help="Inspect a .kfw header, targets, policy, and parts in O(1)")
    p_insp.add_argument("file", help="Path to .kfw file")
    p_insp.add_argument("--no-verify", action="store_true", help="Skip root Ascon-Hash256 verification")
    p_insp.set_defaults(func=cmd_inspect)

    p_ver = sub.add_parser("verify", help="Verify Ascon digests, auth records, vectors, and fences")
    p_ver.add_argument("file", help="Path to .kfw file")
    p_ver.add_argument("--mac-key", help="16-byte hex key (or file) for Ascon-128-MAC")
    p_ver.add_argument("--lms-pubkey", help="32-byte hex public key (or file) for LMS-Ascon")
    p_ver.add_argument("--trust-store", help="Path to trusted-keys.json trust ring")
    p_ver.add_argument("--channel", help="Require signer authorization for release channel (stable/beta/nightly)")
    p_ver.add_argument("--decrypt-key", help="16-byte hex key (or file) for encrypted parts")
    p_ver.add_argument("--require-auth", action="store_true", help="Fail unless a trusted auth record verifies")
    p_ver.set_defaults(func=cmd_verify)

    p_sign = sub.add_parser("sign", help="Append Ascon-128-MAC or PQ LMS-Ascon auth block without mutating body")
    p_sign.add_argument("file", help="Input .kfw file")
    p_sign.add_argument("-o", "--output", help="Output .kfw path (default: overwrite in place)")
    p_sign.add_argument("--mac-key", help="16-byte hex key (or file)")
    p_sign.add_argument("--lms-key", help="Path to LMS-Ascon private key JSON")
    p_sign.add_argument("--signer", default="krel", help="4-char ASCII signer tag")
    p_sign.set_defaults(func=cmd_sign)

    p_kg = sub.add_parser("keygen", help="Generate a new LMS-Ascon or Ascon-128-MAC key")
    p_kg.add_argument("--type", choices=["lms", "mac"], default="lms", help="Key type (default: lms)")
    p_kg.add_argument("--height", type=int, default=LMS_DEFAULT_H, help="LMS Merkle height 1..12 (default: 8 = 256 releases)")
    p_kg.add_argument("--signer", default="krel", help="4-char ASCII signer tag")
    p_kg.add_argument("-o", "--output", help="Output JSON key file")
    p_kg.set_defaults(func=cmd_keygen)

    p_tr = sub.add_parser("trust", help="Manage trusted LMS-Ascon release signing keys (keys/trusted-keys.json)")
    p_tr.add_argument("--store", help="Path to trust store JSON (default: keys/trusted-keys.json)")
    tr_sub = p_tr.add_subparsers(dest="trust_cmd", required=True)
    tr_sub.add_parser("list", help="List trusted & revoked signing keys")
    p_tr_add = tr_sub.add_parser("add", help="Trust a new LMS-Ascon public key")
    p_tr_add.add_argument("--key-file", help="LMS-Ascon key JSON file (only pubkey is exported)")
    p_tr_add.add_argument("--pubkey", help="64-char hex 32-byte LMS-Ascon public key")
    p_tr_add.add_argument("--signer", help="4-char ASCII signer tag")
    p_tr_add.add_argument("--channels", default="stable,beta,nightly", help="Comma-separated authorized channels")
    p_tr_add.add_argument("--description", default="", help="Human-readable key description")
    p_tr_rev = tr_sub.add_parser("revoke", help="Revoke a compromised or retired key by key_id or pubkey prefix")
    p_tr_rev.add_argument("key_id", help="8-char key_id hex or pubkey prefix")
    p_tr.set_defaults(func=cmd_trust)

    p_up = sub.add_parser("upgrade", help="Safe 2-phase radio upgrade (verify + auto-backup + Phase A flash + Phase B RigWire sync)")
    p_up.add_argument("file", help="Path or http(s):// URL to .kfw container")
    p_up.add_argument("-p", "--port", help="Serial port (auto-detected if omitted)")
    p_up.add_argument("-t", "--target", help="Target slug for multi-target bundles (e.g. uv-k1, uv-k10pro)")
    p_up.add_argument("--trust-store", help="Path to trusted-keys.json trust ring")
    p_up.add_argument("--channel", help="Require signer authorization for channel")
    p_up.add_argument("--lms-pubkey", help="32-byte hex LMS-Ascon public key")
    p_up.add_argument("--mac-key", help="16-byte hex Ascon-128-MAC key")
    p_up.add_argument("--decrypt-key", help="16-byte hex Ascon-AEAD128 decrypt key")
    p_up.add_argument("--require-auth", action="store_true", help="Refuse unsigned or untrusted .kfw containers")
    p_up.add_argument("--backup-dir", help="Directory for pre-upgrade calibration/config .kfw backups")
    p_up.add_argument("--skip-backup", action="store_true", help="Skip automatic pre-upgrade calibration backup")
    p_up.add_argument("--skip-flash", action="store_true", help="Skip Phase A MCU flash (sync modules/assets only)")
    p_up.add_argument("--skip-modules", action="store_true", help="Skip Phase B RigWire module/asset sync")
    p_up.add_argument("--ignore-battery", action="store_true", help="Bypass min_battery_mv safety check")
    p_up.add_argument("--dry-run", action="store_true", help="Validate container and trust without touching radio")
    p_up.set_defaults(func=cmd_upgrade)

    p_cu = sub.add_parser("check-update", help="Check a signed release-manifest.json or GitHub repo for firmware updates")
    p_cu.add_argument("source", help="URL/path to release-manifest.json or GitHub 'owner/repo'")
    p_cu.add_argument("--current-version", help="Currently installed SemVer (e.g. 0.1.0)")
    p_cu.add_argument("--current-sec-cnt", type=int, default=0, help="Currently installed sec_cnt")
    p_cu.add_argument("--channel", default="stable", help="Release channel (stable/beta/nightly)")
    p_cu.add_argument("--trust-store", help="Path to trusted-keys.json")
    p_cu.add_argument("--lms-pubkey", help="Pinned 32-byte LMS-Ascon public key")
    p_cu.add_argument("--mac-key", help="16-byte hex Ascon-128-MAC key")
    p_cu.add_argument("--require-auth", action="store_true", help="Require trusted manifest signature")
    p_cu.add_argument("--json", action="store_true", help="Emit JSON output")
    p_cu.set_defaults(func=cmd_check_update)

    p_ext = sub.add_parser("extract", help="Extract verified firmware/modules from a .kfw container")
    p_ext.add_argument("file", help="Input .kfw file")
    p_ext.add_argument("-o", "--out", default="kfw_out", help="Output directory")
    p_ext.add_argument("--target", help="Filter firmware by target slug")
    p_ext.add_argument("--xor", action="store_true", help="Apply Quansheng XOR-16 wire obfuscation to extracted firmware")
    p_ext.add_argument("--decrypt-key", help="16-byte hex key (or file) for encrypted parts")
    p_ext.set_defaults(func=cmd_extract)

    p_st = sub.add_parser("selftest", help="Run built-in crypto, compression, and anti-brick test suite")
    p_st.set_defaults(func=cmd_selftest)

    args = ap.parse_args()
    try:
        return args.func(args)
    except KfwError as e:
        _log_err(str(e))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
