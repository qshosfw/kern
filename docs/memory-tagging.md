# Memory Tagging, Integrity & Fault Recovery Specification

`kern` operates on a target with **16 KB of SRAM** and no MMU (Memory Management Unit). Memory corruption, stack overflow, or pointer corruption on bare metal can lead to silent behavior errors or radio hardware misconfiguration.

To achieve industry-standard safety and transparency, `kern` implements a multi-layered **memory tagging, canary detection, and persistent crash diagnostics framework**.

---

## 1. Stack Painting & Runtime Watermarking

During startup, before branching to application logic, the reset handler paints the entire SRAM stack region with a distinctive 32-bit sentinel pattern (`0xDEADBEEF`):

```
SRAM High Address (0x20004000)
┌───────────────────────────────────────┐
│ Top of Stack (Initial SP)             │
├───────────────────────────────────────┤
│ Active Frame (Function calls)         │
├───────────────────────────────────────┤ ◄── Current Stack Pointer (SP)
│ Unused Stack (Painted: 0xDEADBEEF)    │
│                                       │
├───────────────────────────────────────┤ ◄── Stack Sentinel Guard Word
│ Stack Base Boundary (0x20003000)      │
└───────────────────────────────────────┘
SRAM Low Address
```

### 1.1. High-Watermark Detection Algorithm

A background kernel system task regularly checks the stack region during idle cycles to evaluate stack usage:

```rust
pub fn calculate_stack_watermark() -> usize {
    extern "C" {
        static mut _stack_bottom: u32;
        static mut _stack_top: u32;
    }
    
    let bottom_ptr = unsafe { &_stack_bottom as *const u32 };
    let top_ptr = unsafe { &_stack_top as *const u32 };
    let total_words = (top_ptr as usize - bottom_ptr as usize) / 4;
    
    let mut unused_words = 0;
    unsafe {
        let mut curr = bottom_ptr;
        while curr < top_ptr && *curr == 0xDEADBEEF {
            unused_words += 1;
            curr = curr.add(1);
        }
    }
    
    let used_bytes = (total_words - unused_words) * 4;
    used_bytes
}
```

If unused words drop below **128 bytes** (the Guard Zone), the kernel fires a stack overflow warning over KWP protocol and forces a safe task stack unwinding or system restart before memory below the stack base is corrupted.

---

## 2. Kernel Struct Magic Tags

Every key kernel state structure (Tasks, Drivers, Display Canvases, Ring Buffers) includes a 32-bit `magic` header field initialized with `0x4B45524E` (`"KERN"` ASCII):

```rust
#[repr(C)]
pub struct TaskControlBlock {
    pub magic: u32,         // Must ALWAYS be 0x4B45524E
    pub id: u8,
    pub priority: u8,
    pub state: TaskState,
    pub entry: fn(&mut AppContext),
}

impl TaskControlBlock {
    #[inline(always)]
    pub fn validate(&self) -> Result<(), KernelError> {
        if self.magic == 0x4B45524E {
            Ok(())
        } else {
            Err(KernelError::MemoryCorrupted)
        }
    }
}
```

Every public method on system structures validates `self.magic`. If memory corruption overwrites a structure's header, any subsequent call immediately halts execution and logs a `CorruptedPointer` fault instead of executing garbage function pointers.

---

## 3. Calibration Integrity & EEPROM Checksums

External SPI Flash / EEPROM houses critical factory calibration data (VHF/UHF PA bias, crystal oscillator frequency offset, RSSI calibration curve, squelch thresholds).

```
┌─────────────────────────────────────────────────────────────┐
│                 Calibration Block (SRAM)                    │
├─────────────┬─────────────┬──────────────────┬──────────────┤
│ Magic (u32) │ Version(u8) │ Tuning Data Array│ CRC32 (u32)  │
│ 0x43414C49  │ 0x01        │ [u8; 120]        │ IEEE 802.3   │
└─────────────┴─────────────┴──────────────────┴──────────────┘
```

1. **At Boot**: The kernel reads the calibration block into SRAM and computes its CRC32 checksum.
2. **On Validation Failure**: The kernel refrains from writing corrupt calibration values to the BK4819 RF transceiver (which could damage the PA or transmit out-of-band).
3. **Safe Fallback**: The radio falls back to safe hardcoded default calibration parameters and displays a `CALIBRATION CRC ERROR` warning icon on the status bar.

---

## 4. Persistent Crash Flight Recorder (`.flight_recorder`)

To record diagnosis of HardFaults, Watchdog resets, or memory tagging failures without needing a debugger connected, `kern` maintains an uninitialized 256-byte SRAM region that is preserved across soft resets.

```rust
#[link_section = ".flight_recorder"]
pub static mut FLIGHT_RECORDER: FlightBuffer = FlightBuffer::new();

#[repr(C)]
pub struct FlightBuffer {
    pub reset_reason: u32,
    pub fault_pc: u32,
    pub fault_lr: u32,
    pub fault_sp: u32,
    pub panic_line: u16,
    pub panic_file_hash: u32,
    pub stack_dump: [u32; 16],
    pub magic: u32, // 0x464C5452 ("FLTR")
}
```

### 4.1. HardFault Execution Path

1. Hardware enters `HardFault_Handler`.
2. Handler extracts stacked `PC`, `LR`, `PSR`, and `SP` registers.
3. Handler populates `FLIGHT_RECORDER` in SRAM.
4. Handler sets `reset_reason = 0x48415244` (`"HARD"`) and triggers a system reset via NVIC System Reset Request (`NVIC_SystemReset()`).
5. Upon reboot, `boot.rs` reads `FLIGHT_RECORDER`, formats a diagnostic report over UART/KWP protocol, and renders a crash report screen on the LCD if requested.
