# Safety Guarantees & Calibration Guidelines

This document outlines the safety guarantees, memory isolation rules, and hardware calibration safeguards enforced by `kern`.

---

## 1. Zero-Heap Memory Safety

1. **No Dynamic Allocation**: `alloc` is completely disabled. The firmware operates entirely on statically allocated memory buffers and a bump-allocated kernel arena.
2. **Stack Overflow Prevention**: Stack regions are painted with `0xDEADBEEF`. A background task watermarks stack usage and halts execution if unused space drops below 128 bytes.
3. **Struct Magic Guarding**: All key kernel structures maintain a `0x4B45524E` magic header tag to catch dangling or corrupted pointers.

---

## 2. RF Hardware Protection

1. **Calibration Checksums**: SPI Flash calibration data is validated via CRC32 + HMAC at boot. Corrupted parameters trigger safe default fallback values.
2. **Out-of-Band Lockouts**: Transmit capability is hard-coded to respect amateur radio band boundaries unless override keys are supplied.
3. **Thermal Protection**: Power amplifier bias voltage and transmit duration are limited by software timers to prevent thermal destruction of the RF PA MOSFET.

---

## 3. Unbrick Warranties

1. The first 10KB of internal Flash (`0x08000000`) is reserved for the `kern` Visual DFU Bootloader.
2. Even if the application image is corrupted, holding **PTT + Side Key** on boot forces the bootloader to start DFU mode for easy serial recovery.
