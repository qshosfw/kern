# Safety Guarantees, Calibration & Recovery

This document outlines the memory safety invariants, RF hardware safeguards, and bootloader recovery behavior enforced by Kern.

---

## 1. Memory & Runtime Safety

- **Zero Dynamic Allocation**: `alloc` is disabled across the entire workspace. All kernel and service buffers are statically allocated, arena-backed, or bounded with `heapless`.
- **Stack Watermarking**: Unused stack regions are painted at boot so live and minimum-free stack margins can be audited (and viewed via `/apps/sysinfo.krx`).
- **Typed Format Identifiers**: Persistent records and boot slots use fixed FourCC / `FILE_ID_*` / `BOOT_RECORD_IDENT` identifiers and CRC-32 validation to detect uninitialized or corrupted state.
- **Contained KRX Faults**: Exceptions triggered inside userland KRX applications are caught, recorded to the boot slot, and surfaced cleanly via the `/apps/panic.mod` crash inspector and safe-boot menu.

---

## 2. RF Hardware & Calibration Protection

- **Calibration Integrity**: Hardware calibration parameters (`/sys/calib.dat`, paired with `/sys/calib.sig` Ascon-128 MAC) are validated before use. Missing or corrupted calibration falls back to safe defaults rather than driving invalid PA/oscillator values.
- **Back Up First**: Always keep a full backup of your radio's factory calibration region before flashing custom firmware.
- **Transmit Compliance**: Operators are solely responsible for holding a valid license and complying with local amateur radio band limits and power regulations.

---

## 3. Bootloader & Unbrick Recovery

- **Stock Bootloader Preserved**: Kern links application firmware at `0x0800_2800` (`LENGTH = 118K` on UV-K1 / UV-K5 v3), leaving the 10 KB stock factory bootloader at `0x0800_0000..0x0800_27FF` untouched.
- **DFU Entry & Stall Guard**:
  - Holding **PTT + Power On** enters the stock factory bootloader directly regardless of application Flash contents.
  - Holding **PTT + Side Key 2** at boot (or triggering `Settings → System → DFU Update`) launches `/apps/dfu.mod`, which quiesces RF/PTT/flashlight GPIOs, hands off cleanly to the factory bootloader, and guards against accidental reboot if a flash write stalls mid-transfer.
