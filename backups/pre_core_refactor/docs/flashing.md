# Flashing, Calibration Protection & Recovery Manual

Safety is paramount when working with bare-metal radio firmware.

This manual outlines the **calibration data protection protocol**, **flashing procedures**, and **unbrick recovery workflows** for deploying `kern` to physical Quansheng UV-K1 / UV-K5 v3 hardware.

---

## 1. Safety Rule #1: Factory Calibration Backup

Before flashing any custom firmware or modifying Flash memory, **you MUST back up your radio's factory EEPROM / SPI Flash calibration block**.

The factory calibration section stores critical RF parameters specifically tuned for your radio's individual analog components:
- Crystal Oscillator Frequency Offset (26MHz reference tuning)
- VHF & UHF Power Amplifier Bias Voltages (Low / Mid / High power calibration)
- RSSI Meter Calibration Curves (-120dBm to -50dBm)
- Squelch Threshold Levels (SQL 0 - 9)
- Battery Voltage Measurement Multipliers

> [!CAUTION]
> If your calibration data is lost or overwritten with garbage data, your radio's output power may drop significantly, transmit spurious harmonics, or overheat the PA transistors.

### 1.1. Backup Procedure using `tools/flash.py`

```bash
# Read entire EEPROM / SPI Flash calibration sector (first 64 KB) to a local file
python3 tools/flash.py --port /dev/ttyUSB0 --backup-calib my_radio_calib.bin
```

Keep `my_radio_calib.bin` in a secure location. It can be restored at any time.

---

## 2. Flashing Procedure (`tools/flash.py`)

`kern` binaries are deployed via serial bootloader or DFU mode over the radio's programming jack (UART) or USB-C port.

### 2.1. Standard Serial Flash Workflow

1. Turn off the radio.
2. Connect the programming cable to `/dev/ttyUSB0` (Linux) or `COMx` (Windows).
3. Put radio into bootloader mode by holding **PTT + Side Key 1** while turning on power knob.
4. Execute the Python flash tool:
   ```bash
   python3 tools/flash.py --port /dev/ttyUSB0 --file target/thumbv6m-none-eabi/release/firmware.bin
   ```
5. The tool performs a handshake, uploads 128-byte pages, verifies page CRC, and reboots the device upon completion.

---

## 3. Recovery Workflows (Unbricking)

If a bad flash occurs (e.g., cable disconnected mid-write or invalid firmware header), `kern` provides multi-tiered recovery options:

### Tier 1: Soft Recovery (Visual DFU Mode)
If main application fails its CRC32 check, the `kern` bootloader automatically launches **Visual DFU Recovery Mode** on the LCD display.
- Connect serial cable and run `tools/flash.py --recover` to re-flash main application.

### Tier 2: Bootloader Key Combination
If the application hangs or loops:
- Turn off radio.
- Hold **PTT + Top Side Key** on boot to force the `kern` DFU bootloader to bypass application jump and wait for a fresh firmware binary over serial.

### Tier 3: SWD Hardware Programmer (Unbrick Emergency)
If the bootloader itself is corrupted:
- Connect an ST-Link V2 or J-Link debugger to internal SWD pads on radio PCB:
  - `SWCLK` -> PA14
  - `SWDIO` -> PA13
  - `GND`   -> Board Ground
  - `3.3V`  -> Target VCC
- Flash bootloader binary directly using OpenOCD or PyOCD:
  ```bash
  openocd -f interface/stlink.cfg -f target/py32f071.cfg \
          -c "program bootloader.bin 0x08000000 verify reset exit"
  ```
