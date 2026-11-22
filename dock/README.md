# Kern Dock

Native **GTK4 + Libadwaita 1.8** hardware companion, OS tray service, embedded terminal workbench, IPC bridge, and plugin host for **RigWire** & open Kern-standard radios.

## Architecture

`dock/` is a standalone, strictly layered PEP 621 package (`src/kern_dock/`):

- **`core/`** — Thread-safe typed `EventBus`, schema-validated atomic TOML `ConfigManager` (`~/.config/kern-dock/config.toml`), hardened `SecurityController` + `AuditLog`, and declarative `CommandRegistry` / `PageRegistry` / `TrayMenuRegistry`.
- **`device/`** — GUdev/pyserial `PortMonitor`, pipelined OS-agnostic `DockLink` over RigWire (`link_mode = "rigwire" | "stock_bl"` + dynamic `firmware_name`), offline demo radio, `.kfw`/`.bin` pre-flight verifier & stock bootloader flasher, `.kfw`/`.zip`/raw backup engine, declarative keyboard & rotary-encoder bridge, and T9 planner.
- **`audio/`** — BK4829 DDS-accurate PC tone synthesizer (`synth.py`) matching 26 MHz register 0x71 pitch quantization for headless mode, plus Kenwood 2-pin sound-card audio router (`kenwood.py`).
- **`bridges/`** — Built-in **KISS TNC** (`127.0.0.1:8001`) and **Hamlib `rigctld`** (`127.0.0.1:4532`) TCP bridges.
- **`render/`** — Vectorized NumPy + Cairo ST7565 128×64 LCD engine (`render/lcd.py`), PNG/GIF studio recorder (`render/capture.py`), and declarative photorealistic handset renderer (`render/handset/`) featuring **12 distinct handset & developer bench models**:
  - `k1` (`UV-K1(8)` metallic), `k1-polymer` (`UV-K1 Matte Black`), `k1-clear` (`UV-K1(8) Transparent`)
  - `k5v3` (`UV-K5 v3 / UV-K6`), `k5-orange` (`UV-K5(8) Safety Orange`), `k5-black` (`UV-K5 Diagonal Grille`), `k5-99` (`UV-K5(99) Charcoal`), `k5-99-red` (`UV-K5(99) Red`)
  - `uv5r-plus` (`UV-5R Plus`), `k10-pro` (`UV-K10 Pro`), `k1-naked` (`UV-K1 Open Shell`), `pcb-r51` (`R51-V2.1 Bare PCB`)
- **`system/`** — Native OS integration:
  - Bespoke scalable GTK4 SVG radio icons (`assets/icons/org.kernos.Dock*.svg`)
  - Pure GLib/Gio D-Bus System Tray (`org.kde.StatusNotifierItem` + `com.canonical.dbusmenu` — zero GTK3 dependency)
  - Hardened `0600` local Unix socket JSON-RPC + Session D-Bus (`org.kernos.Dock`)
  - Desktop `.desktop` & `hicolor` icon installer + external OS terminal launcher + desktop notifications
- **`plugins/`** — Declarative, capability-scoped plugin API (`PluginManifest`, `PluginContext`) auto-discovering built-ins (`band-presets`, `radio-bridges`, `table-inspector`, `telemetry-monitor`), workspace drop-ins (`dock/plugins/*.py`), user plugins (`~/.config/kern-dock/plugins/*.py`), and Python entry points (`kern_dock.plugins`).
- **`ui/`** — UI-agnostic `DockRuntime` + adaptive GTK4 + Libadwaita 1.8 tabbed shell (`Home`, `Radio`, `Files` with async GVFS/WebDAV mount, `Flasher`, `Apps` with 1-bit pixel icons & bounded header reads, `Backups`, Colored Hex Inspector, lazy-spawned `Vte-3.91` Terminal + RigWire console, `Capture`, Mini-Dock `Ctrl+M`, Command Palette `Ctrl+P`).

## CLI Usage

```bash
./kern dock                     # Launch GTK4 Hardware Dock
./kern dock --demo              # Launch in offline demo radio mode
./kern dock --mini              # Launch in compact floating Mini-Dock mode
./kern dock --hardened          # Launch directly in Hardened Mode
./kern dock --version           # Print Kern Dock version
./kern dock ctl status          # Query running Dock over local IPC socket
./kern dock ctl freq 145.500    # Tune running radio from terminal / scripts
./kern dock install-desktop     # Install .desktop launcher & GTK4 SVG radio icon
./kern dock models [--json]     # List all 12 handset & bench models
./kern dock plugins [--json]    # List discovered plugins & capabilities
./kern dock test                # Run full headless automated test suite
./kern dock build               # Bytecode check & test suite
```
