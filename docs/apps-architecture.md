# Core Applications Architecture (`docs/apps-architecture.md`)

`kern` ships with a fixed suite of core built-in applications: **Ham** (VFO/Dual Watch), **FM Broadcast**, **Memories** (Channels), **Contacts**, **Messenger**, **Settings**, **Files**, **Spectrum**, **Waterfall**, and **Scope**.

Because these apps are built directly into the kernel, we do not waste CPU cycles or Flash space abstracting them behind virtual filesystems (VFS) or complex sandboxes. They run at bare-metal speed with direct access to hardware drivers, but are organized via a clean, unified architectural model.

---

## 1. Direct Trait-Based App Supervisor

In traditional monolithic firmware (like `k1-fw`), the main loop is an enormous `switch` statement checking `if (current_app == APP_SCANNER)` to route UI rendering and key presses.

`kern` eliminates this spaghetti code by defining a single, zero-overhead Rust trait:

```rust
pub trait App {
    /// Called once when the app is launched.
    fn init(&mut self, hw: &mut HardwareContext);
    
    /// Called every tick for animations or continuous updates (e.g., Waterfall).
    fn update(&mut self, hw: &mut HardwareContext, dt_ms: u32);
    
    /// Called when the user presses a keypad button.
    fn on_key(&mut self, hw: &mut HardwareContext, key: KeyEvent);
    
    /// Renders the app's UI to the ST7565 LCD frame buffer.
    fn render(&self, display: &mut Display);
}
```

The kernel simply holds a pointer to the `ActiveApp`. Context switching is just swapping this pointer. Apps have direct, zero-overhead access to the `HardwareContext` (which contains the `PhyDriver` to tune the radio, or the `LittleFS` handle to write files).

---

## 2. Unified Hardware Event Dispatch

Instead of every app writing its own messy polling loop to check if the FSK modem received a byte, or if the UART has data, `kern` uses a central interrupt dispatcher.

1. A hardware interrupt fires (e.g., BK4819 FSK FIFO watermark, or a Rigwire UART packet completes).
2. The kernel ISR clears the hardware flag and constructs a lightweight `Event` enum (e.g., `Event::FskPacketReceived(&[u8])`).
3. The kernel immediately passes this event to the `ActiveApp.on_event()` method.

**Why this is elegant**: The `Messenger` app doesn't need to know *how* to read the BK4819 FIFO. It simply waits for an `FskPacketReceived` event from the kernel. This keeps the app logic tiny and focused purely on parsing the message, while retaining direct execution speed.

---

## 3. Direct In-Place Rendering (Zero-Bloat UI)

We avoid hoarding our precious 16 KB of SRAM with multiple "virtual framebuffers" or heavy React-style data-binding frameworks.

The `kern` UI model is extremely direct:
- There is exactly one 1 KB LCD frame buffer in SRAM.
- When an app needs to update the screen (e.g., the `Spectrum` app reading RSSI), it issues direct immediate-mode drawing commands: `display.draw_rect(...)`, `display.draw_text(...)`.
- A background DMA transfer pushes the buffer to the ST7565 LCD. 

It is fast, predictable, and requires zero dynamic memory allocation.

---

## 4. Zero-Copy Payload Routing

When the `Messenger` app receives a packet, traditional firmware copies the data from the hardware FIFO, into a driver buffer, then into a protocol parser, and finally into the UI string buffer.

`kern` uses a zero-copy pointer model:
1. The FSK hardware DMA writes the raw payload directly into an SRAM buffer.
2. The kernel passes the pointer (`&[u8]`) directly to the `Messenger` app via the `Event` dispatcher.
3. The `Messenger` app parses the string in-place and renders it to the screen.

Zero bytes of memory are copied during packet reception, keeping CPU utilization microscopic.

---

## 5. Isolated Hardware Routing (Secondary RF Chips)

Traditional monolithic firmware (like `k1-fw`) manages all RF chips (BK4819 for VHF/UHF, BK1080 for FM Broadcast, Si4732 for HF) inside a single, massive `radio.c` file. This results in horrific `if (current_radio == BK1080)` branching throughout the entire OS audio and tuning paths.

`kern` solves this via **Isolated App Hardware Contexts**.

When the **FM Broadcast** app launches (which uses the secondary BK1080 chip), it requests the specific `Bk1080Phy` driver object from the kernel. 
- The `Ham` (VFO) app only holds a reference to the `Bk4819Phy` driver.
- The `FM Broadcast` app holds a reference to the `Bk1080Phy` driver.
- The core audio routing and hardware states are never tangled together. If the user uninstalls the `FM Broadcast` app at compile time, the `Bk1080Phy` driver is completely stripped from the final binary, saving flash space.
