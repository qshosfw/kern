#!/usr/bin/env python3
"""
KERN Realtime eXecutable (KRX) Binary Container Packaging Tool.
Normative implementation of KRX-ABI 2.5.0 / 3.2.0 (§1, §5, §6, §7, §8, §12).
Generates strictly aligned, CRC32-verified .krx binary packages for ARM Cortex-M0+.
"""

import argparse
import os
import struct
import sys
import zlib

KRX_MAGIC = bytes([0xC0, 0xDE, 0x01, 0xFE])
FORMAT_VERSION = 0x05  # 2.5.0

# KRX Classes
CLASS_NATIVE = 0
CLASS_PAGED = 1
CLASS_VM = 2
CLASS_DECLARE = 3

# KRX Profiles (raw codes)
PROFILE_N0_FIXED = 0x10
PROFILE_P0_PAGED = 0x11
PROFILE_N1_PIC = 0x12
PROFILE_P1_PAGED = 0x13
PROFILE_V0_VM = 0x20
PROFILE_D0_DECLARE = 0x30
PROFILE_F0_FLOW = 0x31

# Section IDs
SEC_IDENTITY = 0x0001
SEC_EXECUTION = 0x0002
SEC_CAP_IMPORTS = 0x0003
SEC_ASSETS = 0x0004
SEC_BLOCK_MAP = 0x0005
SEC_SERVICE_EXPORTS = 0x0006
SEC_CONFIG = 0x0007
SEC_SEAL = 0x0008
SEC_TYPE_LIFECYCLE = 0x0009
SEC_LEAF_MAP = 0x000A

# Capability Bitmask Constants
CAP_NONE = 0
CAP_DISPLAY = 1 << 0
CAP_KEYPAD = 1 << 1
CAP_AUDIO_OUT = 1 << 2
CAP_AUDIO_CAP = 1 << 3
CAP_RADIO_RX = 1 << 4
CAP_RADIO_TX = 1 << 5
CAP_STORAGE_RD = 1 << 6
CAP_STORAGE_WR = 1 << 7
CAP_BROADCAST_FM = 1 << 8
CAP_RAW_PERIPH = 1 << 9
CAP_SERIAL = 1 << 10
CAP_USB = 1 << 11
CAP_POWER = 1 << 12


def align4(data: bytearray) -> bytearray:
    pad = (4 - (len(data) % 4)) % 4
    if pad:
        data.extend(b"\x00" * pad)
    return data


class KrxPackageBuilder:
    def __init__(self, name: str, app_class: int, profile: int):
        self.name = name
        self.app_class = app_class
        self.profile = profile
        self.version = (1, 0, 0)
        self.author = b"KERN-TEAM"
        self.min_abi = 1
        self.link_base = 0
        self.entry_point = 0
        self.min_stack = 128
        self.min_heap = 0
        self.page_size = 1024
        self.cache_blocks = 2
        self.got_offset = 0
        self.reloc_offset = 0
        self.reloc_count = 0
        self.caps = CAP_DISPLAY | CAP_KEYPAD
        self.payload = bytearray()
        self.cap_imports = []
        self.leaf_map = []
        self.block_map = []
        self.leaf_files = []
        self.block_files = []
        self.resident_payload_size = 0
        self.service_exports = []

    def set_identity(self, name: str, author: str = "KERN-TEAM", version=(1, 0, 0)):
        self.name = name
        self.author = author.encode("utf-8")[:12]
        self.version = version

    def set_execution(
        self,
        entry_point: int,
        link_base: int = 0,
        got_offset: int = 0,
        min_stack: int = 128,
        min_heap: int = 0,
        page_size: int = 1024,
        cache_blocks: int = 2,
    ):
        self.entry_point = entry_point
        self.link_base = link_base
        self.got_offset = got_offset
        self.min_stack = min_stack
        self.min_heap = min_heap
        self.page_size = page_size
        self.cache_blocks = cache_blocks

    def add_cap_import(self, interface_id: int, slot_idx: int, min_abi: int = 1, mandatory: bool = True):
        flags = 0x0001 if mandatory else 0x0000
        self.cap_imports.append((interface_id, min_abi, flags, slot_idx))

    def add_leaf(self, leaf_id: int, entry_offset: int, size: int, flash_offset: int, flags: int = 0):
        self.leaf_map.append((leaf_id, flags, size, entry_offset, flash_offset))

    def add_leaf_file(self, leaf_id: int, file_path: str, flags: int = 0):
        with open(file_path, "rb") as f:
            data = f.read()
        self.leaf_files.append((leaf_id, flags, data))

    def add_block(self, block_id: int, size: int, flash_offset: int, flags: int = 0):
        self.block_map.append((block_id, flags, size, flash_offset))

    def add_block_file(self, block_id: int, file_path: str, flags: int = 0):
        with open(file_path, "rb") as f:
            data = f.read()
        self.block_files.append((block_id, flags, data))

    def add_service_export(self, interface_id: int, vtable_offset: int, abi_version: int = 1, pinned: bool = False):
        flags = 0x0001 if pinned else 0x0000
        self.service_exports.append((interface_id, abi_version, vtable_offset, flags))

    def set_payload(self, payload_data: bytes):
        self.payload = bytearray(payload_data)

    def add_delta_relocs(self, reloc_offsets: list[int]):
        """Encode relocation offsets into delta relocation stream v2 with 16-bit escapes."""
        if not reloc_offsets:
            return
        relocs = sorted(reloc_offsets)
        stream = bytearray()
        prev = 0
        for off in relocs:
            delta = off - prev
            prev = off
            if delta < 0xFF:
                stream.append(delta)
            else:
                stream.append(0xFF)
                stream.extend(struct.pack("<H", delta))
        stream.append(0)  # Terminal byte

        self.reloc_offset = len(self.payload)
        self.reloc_count = len(relocs)
        self.payload.extend(stream)
        align4(self.payload)

    def build(self) -> bytes:
        string_pool = bytearray()

        def add_string(s: str) -> int:
            nonlocal string_pool
            offset = len(string_pool)
            string_pool.extend(s.encode("utf-8") + b"\x00")
            return offset

        name_str_offset = add_string(self.name)

        # Pre-compute total_header_len to resolve leaf / block flash offsets
        num_leaves = len(self.leaf_files) if self.leaf_files else len(self.leaf_map)
        num_blocks = len(self.block_files) if self.block_files else len(self.block_map)

        sec_sizes = [
            4 + 40, # Identity
            4 + 28, # Execution
            4 + 12, # TypeLifecycle
        ]
        if self.cap_imports:
            sec_sizes.append(4 + len(self.cap_imports) * 8)
        if num_leaves > 0:
            sec_sizes.append(4 + num_leaves * 16)
        if num_blocks > 0:
            sec_sizes.append(4 + num_blocks * 8)
        if self.service_exports:
            sec_sizes.append(4 + len(self.service_exports) * 16)

        sec_hdr_len = 0
        for sz in sec_sizes:
            sec_hdr_len += sz
            pad = (4 - (sz % 4)) % 4
            sec_hdr_len += pad

        str_pool_off = 20 + sec_hdr_len
        tot_hdr = str_pool_off + len(string_pool)
        h_pad = (4 - (tot_hdr % 4)) % 4
        tot_hdr += h_pad

        # Resolve leaf files into payload with exact flash offsets
        if self.leaf_files:
            root_data = bytes(self.payload)
            self.resident_payload_size = len(root_data)
            self.payload = bytearray(root_data)
            self.leaf_map = []
            cur_flash_off = tot_hdr + len(root_data)
            for leaf_id, flags, ldata in self.leaf_files:
                pad = (4 - (len(self.payload) % 4)) % 4
                self.payload.extend(b"\x00" * pad)
                cur_flash_off += pad
                self.leaf_map.append((leaf_id, flags, len(ldata), 0, cur_flash_off))
                self.payload.extend(ldata)
                cur_flash_off += len(ldata)

        # Resolve block files into payload with exact flash offsets
        if self.block_files:
            self.payload = bytearray()
            self.block_map = []
            cur_flash_off = tot_hdr
            for block_id, flags, bdata in self.block_files:
                pad = (4 - (len(self.payload) % 4)) % 4
                self.payload.extend(b"\x00" * pad)
                cur_flash_off += pad
                self.block_map.append((block_id, flags, len(bdata), cur_flash_off))
                self.payload.extend(bdata)
                cur_flash_off += len(bdata)

        sections = []

        # 1. Identity Section (40 bytes body)
        ident_body = bytearray()
        author_bytes = self.author.ljust(12, b"\x00")[:12]
        ident_body.extend(struct.pack("<H", 0))  # name_str offset (filled after sizing)
        ident_body.extend(bytes([self.version[0], self.version[1], self.version[2]]))
        ident_body.extend(struct.pack("<H", self.min_abi))
        ident_body.extend(author_bytes)
        ident_body.extend(struct.pack("<I", 0x67000000))  # timestamp
        ident_body.extend(struct.pack("<H", 1))           # icon_asset
        ident_body.extend(struct.pack("<H", 0))           # flags
        ident_body.extend(b"\x00" * 13)                   # reserved
        assert len(ident_body) == 40
        sections.append((SEC_IDENTITY, ident_body))

        # 2. Execution Section (28 bytes body)
        exec_body = bytearray()
        exec_body.extend(struct.pack("<H", self.min_stack))
        exec_body.extend(struct.pack("<H", self.min_heap))
        exec_body.extend(struct.pack("<H", self.page_size))
        exec_body.append(self.cache_blocks)
        exec_body.append(0)  # dma_burst_size
        exec_body.extend(struct.pack("<I", self.entry_point))
        payload_size = self.resident_payload_size if self.resident_payload_size > 0 else len(self.payload)
        exec_body.extend(struct.pack("<I", payload_size))
        exec_body.extend(struct.pack("<H", self.reloc_offset))
        exec_body.extend(struct.pack("<H", self.reloc_count))
        exec_body.extend(struct.pack("<H", self.got_offset))
        exec_body.extend(struct.pack("<H", 0))  # rt_pin_mask
        exec_body.extend(struct.pack("<H", 0))  # max_fault_latency_us
        exec_body.append(self.profile)          # reserved[0] = explicit profile
        exec_body.append(0)                     # reserved[1]
        assert len(exec_body) == 28
        sections.append((SEC_EXECUTION, exec_body))

        # 3. Type & Lifecycle Section (12 bytes body)
        type_body = bytearray()
        type_body.append(0)  # App
        type_body.append(0)  # isolation: Ring3
        type_body.append(1)  # stack_cost
        type_body.append(0)  # reserved
        type_body.extend(struct.pack("<I", self.caps))
        type_body.extend(struct.pack("<I", 0))  # reserved2
        assert len(type_body) == 12
        sections.append((SEC_TYPE_LIFECYCLE, type_body))

        # 4. CapImports Section (if present)
        if self.cap_imports:
            cap_body = bytearray()
            for iface_id, min_abi, flags, slot_idx in self.cap_imports:
                cap_body.extend(struct.pack("<HHHH", iface_id, min_abi, flags, slot_idx))
            sections.append((SEC_CAP_IMPORTS, cap_body))

        # 5. LeafMap Section (if present, 16 bytes per entry)
        if self.leaf_map:
            leaf_body = bytearray()
            for leaf_id, flags, size, entry_off, flash_off in self.leaf_map:
                leaf_body.append(leaf_id)
                leaf_body.append(flags)
                leaf_body.extend(struct.pack("<H", size))
                leaf_body.extend(struct.pack("<I", flash_off))
                leaf_body.extend(struct.pack("<H", entry_off))
                leaf_body.extend(struct.pack("<H", 0))  # ram_scratch_bytes
                leaf_body.extend(b"\x00" * 4)           # reserved
            assert len(leaf_body) % 16 == 0
            sections.append((SEC_LEAF_MAP, leaf_body))

        # 6. BlockMap Section (if present)
        if self.block_map:
            block_body = bytearray()
            for block_id, flags, size, flash_off in self.block_map:
                block_body.append(block_id)
                block_body.append(flags)
                block_body.extend(struct.pack("<H", size))
                block_body.extend(struct.pack("<I", flash_off))
            sections.append((SEC_BLOCK_MAP, block_body))

        # 7. ServiceExports Section (if present, 16 bytes per entry)
        if self.service_exports:
            export_body = bytearray()
            for iface_id, abi_ver, vtable_off, flags in self.service_exports:
                export_body.extend(struct.pack("<HHIH", iface_id, abi_ver, vtable_off, flags))
                export_body.extend(b"\x00" * 6)
            assert len(export_body) % 16 == 0
            sections.append((SEC_SERVICE_EXPORTS, export_body))

        # Calculate section headers offset
        # Preamble is 20 bytes (KRX_MAGIC..header_crc32)
        preamble_len = 20
        sec_header_len = 0
        for sec_id, sec_body in sections:
            sec_header_len += 4 + len(sec_body)
            pad = (4 - (len(sec_body) % 4)) % 4
            sec_header_len += pad

        str_pool_offset = preamble_len + sec_header_len
        # Fixup name_str offset in Identity Section
        ident_body[0:2] = struct.pack("<H", str_pool_offset + name_str_offset)

        total_header_len = str_pool_offset + len(string_pool)
        # Pad total header to 4-byte boundary
        hdr_pad = (4 - (total_header_len % 4)) % 4
        total_header_len += hdr_pad

        # Serialize Section Headers
        encoded_sections = bytearray()
        for sec_id, sec_body in sections:
            sec_len = 4 + len(sec_body)
            encoded_sections.extend(struct.pack("<HH", sec_id, sec_len))
            encoded_sections.extend(sec_body)
            align4(encoded_sections)

        header_content = encoded_sections + string_pool + (b"\x00" * hdr_pad)
        header_crc = zlib.crc32(header_content) & 0xFFFFFFFF
        payload_crc = zlib.crc32(self.payload) & 0xFFFFFFFF

        # Build 20-byte Preamble
        preamble = bytearray()
        preamble.extend(KRX_MAGIC)
        preamble.append(self.app_class)
        preamble.append(FORMAT_VERSION)
        preamble.extend(struct.pack("<H", len(sections)))
        preamble.extend(struct.pack("<H", total_header_len))
        preamble.extend(struct.pack("<H", 0))  # flags
        preamble.extend(struct.pack("<I", payload_crc))
        preamble.extend(struct.pack("<I", header_crc))
        assert len(preamble) == 20

        full_package = preamble + header_content + self.payload
        return bytes(full_package)


def main():
    parser = argparse.ArgumentParser(description="KRX Executable Packaging Tool")
    parser.add_argument("output", help="Output .krx file path")
    parser.add_argument("--name", default="App", help="Application display name")
    parser.add_argument("--author", default="KERN-TEAM", help="Author name")
    parser.add_argument(
        "--profile",
        choices=["n0", "n1", "p0", "p1", "d0"],
        default="n1",
        help="KRX execution profile",
    )
    parser.add_argument("--payload", help="Binary payload input file")
    parser.add_argument("--entry", type=lambda x: int(x, 0), default=0, help="Entry point offset")
    parser.add_argument("--link-base", type=lambda x: int(x, 0), default=0, help="Link base VMA")
    parser.add_argument("--got-offset", type=lambda x: int(x, 0), default=0, help="GOT offset")
    parser.add_argument("--min-stack", type=int, default=128, help="Minimum stack bytes")
    parser.add_argument("--min-heap", type=int, default=0, help="Minimum heap bytes (0 for no_std)")
    parser.add_argument("--page-size", type=lambda x: int(x, 0), default=1024, help="Page/block size")
    parser.add_argument("--cache-blocks", type=int, default=2, help="Number of cache blocks")
    parser.add_argument("--leaf", action="append", default=[], help="Add leaf overlay: <id>:<path>")
    parser.add_argument("--block", action="append", default=[], help="Add demand block: <id>:<path>")

    args = parser.parse_args()

    profile_map = {
        "n0": (CLASS_NATIVE, PROFILE_N0_FIXED),
        "n1": (CLASS_NATIVE, PROFILE_N1_PIC),
        "p0": (CLASS_PAGED, PROFILE_P0_PAGED),
        "p1": (CLASS_PAGED, PROFILE_P1_PAGED),
        "d0": (CLASS_DECLARE, PROFILE_D0_DECLARE),
    }

    app_class, profile_code = profile_map[args.profile]
    builder = KrxPackageBuilder(args.name, app_class, profile_code)
    builder.set_identity(args.name, args.author)
    builder.set_execution(
        entry_point=args.entry,
        link_base=args.link_base,
        got_offset=args.got_offset,
        min_stack=args.min_stack,
        min_heap=args.min_heap,
        page_size=args.page_size,
        cache_blocks=args.cache_blocks,
    )

    if args.payload and os.path.exists(args.payload):
        with open(args.payload, "rb") as f:
            builder.set_payload(f.read())
    else:
        # Default minimal NOP Thumb-1 payload: bx lr (0x4770)
        builder.set_payload(struct.pack("<H", 0x4770))

    for item in args.leaf:
        parts = item.split(":", 1)
        lid = int(parts[0], 0)
        lpath = parts[1]
        builder.add_leaf_file(lid, lpath)

    for item in args.block:
        parts = item.split(":", 1)
        bid = int(parts[0], 0)
        bpath = parts[1]
        builder.add_block_file(bid, bpath)

    package_bytes = builder.build()
    with open(args.output, "wb") as f:
        f.write(package_bytes)

    print(f"Packed KRX package: {args.output} ({len(package_bytes)} bytes, profile={args.profile.upper()})")


if __name__ == "__main__":
    main()
