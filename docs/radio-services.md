# Modular Radio Logic Architecture (`docs/radio-services.md`)

When analyzing traditional monolithic radio firmware (e.g., `k1-fw`), core logic like Squelch, Dual Watch, Scanning, and PTT handling are often tightly coupled into a massive, tangled state machine.

In `kern`, we maintain extreme minimalism while utilizing an **Interrupt-Driven, Cooperative Multitasking** execution model. While we do not run heavy desktop-style background "apps," the kernel efficiently multiplexes concurrent hardware tasks. For example, while the main loop is handling a Dual Watch sweep, hardware interrupts (UART RX, FSK FIFO watermarks, ADC) continuously collect data in the background, allowing `kern` to seamlessly decode incoming telemetry or serial packets without blocking the primary radio logic.

---

## 1. Deterministic Squelch

Traditional firmware polls hardware registers inside the main UI loop to determine if the radio should unmute. 

In `kern`, Squelch is a clean, isolated logic block that evaluates hardware metrics and triggers a state transition.

### 1.1. The Agnostic Squelch Pipeline
The Squelch logic does not care if it is receiving FM on the BK4819 or AM on the Si4732 HF mod. 
1. **Inputs**: RSSI (dBm), Noise Floor, SNR, and decoded Sub-Audible Tone (from the active PHY).
2. **Evaluation**: Combines traditional carrier squelch with `UniversalContact` identity validation.
3. **Action**: If the squelch opens, it transitions the radio state to `State::Receiving` and un-mutes the audio DAC.

---

## 2. Universal VFO & Scanner

In legacy firmware, "Dual Watch" (monitoring two frequencies) and "Scanning" (sweeping a band) are hardcoded, hardware-specific loops.

`kern` generalizes this into a minimal **Scanner**:
- The scanner operates on generic **Channels** (which abstract the frequency and PHY driver).
- While monitoring a channel, the kernel leverages hardware interrupts. If a carrier opens the squelch, or an FSK frame triggers the modem FIFO interrupt, the kernel immediately dispatches the appropriate asynchronous handler (e.g., FSK decoder or audio unmuting).
- If no valid signal is found, the scanner simply advances to the next channel based on its dwell timer.

**Benefit**: We get Cross-Band, Cross-Modulation, and Cross-Chip scanning with almost zero extra code. The radio can seamlessly sweep between VHF FM and HF SSB because the tuning logic is purely abstract.

---

## 3. Minimal PTT & Telemetry Hooks (ANI)

Traditional firmware hardcodes DTMF tones (ANI / PTT-ID) directly into the PTT button hardware interrupt.

`kern` uses a simple callback hook system for transmissions:
1. User presses PTT. The state transitions to `State::Transmitting`.
2. The core triggers a `TxStart` hook.
3. Only the currently active features execute:
   - If the active contact has a **Hermes Node Tag**, the PHY injects a fast FSK burst before opening the mic.
   - If **DVS (Dynamic Voice Scrambling)** is enabled for this contact, the scrambler PRNG begins hopping using DCS.
   - If no telemetry is needed, the radio simply keys the PA and transmits raw audio.

**Benefit**: Extreme modularity without runtime overhead. Features are invoked deterministically based on the active Contact configuration, not via heavy concurrent message buses.

---

## 4. Agnostic Tx Routing & Offsets

Handling repeater offsets (Shift +, Shift -) is usually a messy math calculation scattered throughout C firmware.

In `kern`, a **Channel** strictly defines an `rx_freq` and a `tx_freq`.
- Upon PTT, the radio simply tunes the PHY to `tx_freq`.
- If a user configures a **Cross-Band Repeater** mode, the firmware just listens on Channel A's `rx_freq`, and upon Squelch open, keys the PTT hook on Channel B's `tx_freq`.

Because audio routing and tuning are exposed as generic PHY traits, cross-band repeating requires virtually no specific hardware logic.
