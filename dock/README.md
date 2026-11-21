# Kern Dock

Native **GTK4 + Libadwaita 1.8** hardware companion, OS tray service, embedded terminal workbench, IPC bridge, and plugin host for **Kern OS** amateur radios.

## Architecture

`dock/` is a standalone, strictly layered PEP 621 package (`src/kern_dock/`):

- **`core/`** — Thread-safe typed `EventBus`, schema-validated atomic TOML `ConfigManager` (`~/.config/kern-dock/config.toml`), hardened `SecurityController` + `AuditLog`, and declarative `CommandRegistry` / `PageRegistry` / `TrayMenuRegistry`.
- **`device/`** — GUdev/pyserial `PortMonitor`, pipelined `DockLink` over RigWire, offline demo radio, `.kfw`/`.bin` pre-flight verifier & stock bootloader flasher, declarative keyboard bridge, and bit-exact T9 multi-tap planner.
- **`render/`** — Vectorized NumPy + Cairo ST7565 128×64 LCD engine (`render/lcd.py`), PNG/GIF studio recorder (`render/capture.py`), and declarative photorealistic handset renderer (`render/handset/`) featuring 5 genuinely distinct real Quansheng chassis models:
  - `k1` — Quansheng UV-K1(8) *(compact Mini-Kong, 4-lug anodized metallic armor, cobalt knob/PTT, flush 4×4 keypad + 12 milled speaker slots)*
  - `k1-base` — Quansheng UV-K1 *(monolithic matte-black polymer shell, black knob/PTT)*
  - `k5v3` — Quansheng UV-K5 v3 / UV-K6 *(tall sculpted-waist chassis, brushed metallic upper armor with 4 Torx bosses, orange V-notch knob, 2 pill speaker bars)*
  - `k5` — Quansheng UV-K5 Original *(monolithic all-black body, diagonal slatted acoustic grille, black rotary knob)*
  - `uv5r-plus` — Quansheng UV-5R Plus *(UV-5R silhouette, brushed metallic U-frame, orange trim, full-width horizontal grille & DTMF keypad)*
- **`system/`** — Native OS integration:
  - Bespoke scalable GTK4 SVG radio icons (`assets/icons/org.kernos.Dock*.svg`)
  - Pure GLib/Gio D-Bus System Tray (`org.kde.StatusNotifierItem` + `com.canonical.dbusmenu` — zero GTK3 dependency)
  - Hardened `0600` local Unix socket JSON-RPC + Session D-Bus (`org.kernos.Dock`)
  - Desktop `.desktop` & `hicolor` icon installer + external OS terminal launcher + desktop notifications
- **`plugins/`** — Declarative, capability-scoped plugin API (`PluginManifest`, `PluginContext`) auto-discovering built-ins (`band-presets`, `telemetry-monitor`, `table-inspector`), workspace drop-ins (`dock/plugins/*.py`), user plugins (`~/.config/kern-dock/plugins/*.py`), and Python entry points (`kern_dock.plugins`).
- **`ui/`** — UI-agnostic `DockRuntime` + adaptive GTK4 + Libadwaita 1.8 shell (`Adw.OverlaySplitView`, `Adw.Breakpoint`, Compact Mini-Dock `Ctrl+M`, Command Palette `Ctrl+P`, embedded `Vte-3.91` OS Terminal, Drag-and-Drop flashing/upload).

## CLI Usage

```bash
./kern dock                     # Launch GTK4 Hardware Dock
./kern dock --demo              # Launch in offline demo radio mode
./kern dock --mini              # Launch in compact floating Mini-Dock mode
./kern dock --hardened          # Launch directly in Hardened Mode
./kern dock ctl status          # Query running Dock over local IPC socket
./kern dock ctl freq 145.500    # Tune running radio from terminal / scripts
./kern dock install-desktop     # Install .desktop launcher & GTK4 SVG radio icon
./kern dock models              # List declarative handset models
./kern dock plugins             # List discovered plugins & capabilities
./kern dock test                # Run full headless automated test suite
./kern dock build               # Bytecode check & test suite
```
