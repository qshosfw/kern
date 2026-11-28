# Kern SDK

Minimal, zero-clutter SDKs for **on-device `.krx` applications** (`abi::VERSION = 5`) and **cross-platform `RigWire v2` host integrations**.

## Layout

```text
sdk/
├── krx.ld             # Shared Cortex-M0+ linker script for .krx / .mod binaries
├── c/
│   ├── krx.h          # C89/C99/C++ on-device .krx syscall SDK
│   ├── krx_ops.h      # Auto-generated .krx opcodes (`./kern abi`)
│   └── rigwire.h      # Single-header ANSI C89 RigWire v2 + Ascon-128 + Table codec
├── rust/              # Rust #![no_std] on-device .krx SDK (`kern-sdk`)
├── zig/
│   └── krx.zig        # Zig freestanding Cortex-M0+ on-device .krx SDK
├── luau/
│   └── krx.d.luau     # Strict Luau (--!strict) .krx types (`luau-lsp` & `tools/luau_krx.py`)
└── host/
    ├── rigwire.js     # ES5 UMD JS (WebSerial/WebBluetooth/WebUSB, KaiOS 2.5/3.0, Node, QuickJS)
    ├── rigwire.py     # Zero-dep Python 2.7 / 3.x (Linux, XNU, Windows, Nokia N900, Termux)
    └── RigWire.java   # Zero-dep Java 1.4 / CLDC 1.1 J2ME (Symbian) / Android / Desktop JVM
```

## Documentation & Verification

- Normative wire, table, language & platform spec: [`docs/kern-open-standards.md`](file:///home/drew/Repositories/kern/docs/kern-open-standards.md)
- Run cross-language SDK & compiler verification: `python3 tools/test_rigwire_sdk.py`
