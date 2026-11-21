# 🔀 KERN Multiboot & Golden OS Manager (`/apps/multiboot.mod`)

A standalone, borderless **P0 Structured Paged (`FLAG_SYSTEM_MENU`)** KRX module for managing firmware boot slots and a CRC-32 deduplicated Golden OS backup on Quansheng UV-K1 / UV-K5 v3 (`PY32F071xB` + `PY25Q16` 2 MB SPI Flash).

Works seamlessly from **Settings → System → Multiboot**, from **Recovery → Multiboot**, or directly from **DFU `[MENU] Tools`** when installed without `/apps/recovery.mod`.

---

## ✨ Features

- **CRC-32 Deduplicated Golden Backup (`Backup Kern OS`):** Snapshots active internal App Flash (`0x0800_2800..`) to `/boot/kern_golden.bin` with read-back CRC-32 verification; skips writes when the backup already matches active flash to eliminate SPI flash wear.
- **One-Tap Golden Restore (`Restore Golden`):** Restores `/boot/kern_golden.bin` with battery guard (`>= 6.80 V` or USB-C charging), 4-point ARM vector validation, 1.0 s Hold-`MENU` confirmation, and 100% SRAM-resident Page-0-Last programming.
- **Boot Slot Switcher (`Boot Slots`):** Browses `/boot/*.kfw`, `/boot/*.uf2`, and `/boot/*.bin`, inspects format/size/target bounds, warns if no Golden OS backup exists before switching to a guest image, and switches safely via the SRAM Page-0-Last flasher.
- **Live Slot Status (`Slot Status`):** Displays active OS size & CRC-32, Golden OS sync state & CRC-32, vector health, and live battery voltage.

---

## 🔧 Build & Sideload

```bash
./kern pkg multiboot
./kern wire sideload packages/multiboot/multiboot.mod --force
```
