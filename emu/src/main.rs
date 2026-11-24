//! `kern-emu`: In-house high-speed hardware emulator for Kern OS.
//!
//! Exposes:
//! - `--serial-port 4446`: Native RigWire TCP endpoint (100% compatible with Kern Dock & `rigwire.py`)
//! - `--ctl-port 4445`: Line-delimited JSON-RPC hardware control endpoint (instant GRAM read,
//!   physical keypad matrix injection, battery/RF telemetry overrides, warm/cold reset, turbo mode)

mod board;
mod cpu;
mod soc;

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use board::{FlashInitMode, key_name_to_bit};
use cpu::CortexM0Plus;
use soc::Py32Soc;

enum CtlReq {
    Status,
    Framebuffer,
    PressKey { key: String, hold_ms: u64 },
    SetKey { key: String, down: bool },
    ReleaseAll,
    Reset { warm: bool },
    SetBatteryMv { mv: u16 },
    SetRf { rssi_dbm: i16, carrier: bool },
    SetTurbo { turbo: bool },
    SerialLog,
    Quit,
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0xF) as usize] as char);
    }
    s
}

/// Minimal zero-dependency extractor for flat JSON string/int/bool fields.
fn json_str<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let idx = line.find(&pat)?;
    let rest = line[idx + pat.len()..].trim_start().strip_prefix(':')?.trim_start();
    let inner = rest.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(&inner[..end])
}

fn json_i64(line: &str, key: &str) -> Option<i64> {
    let pat = format!("\"{key}\"");
    let idx = line.find(&pat)?;
    let rest = line[idx + pat.len()..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn json_bool(line: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{key}\"");
    let idx = line.find(&pat)?;
    let rest = line[idx + pat.len()..].trim_start().strip_prefix(':')?.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn handle_ctl_client(stream: TcpStream, tx: Sender<(CtlReq, Sender<String>)>) {
    let mut writer = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(raw) = line else { break };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cmd = json_str(trimmed, "cmd").unwrap_or("status");
        let req = match cmd {
            "status" => CtlReq::Status,
            "framebuffer" | "fb" => CtlReq::Framebuffer,
            "press_key" | "key" => CtlReq::PressKey {
                key: json_str(trimmed, "key").unwrap_or("menu").to_string(),
                hold_ms: json_i64(trimmed, "hold_ms").unwrap_or(120).max(20) as u64,
            },
            "set_key" => CtlReq::SetKey {
                key: json_str(trimmed, "key").unwrap_or("menu").to_string(),
                down: json_bool(trimmed, "down").unwrap_or(false),
            },
            "release_all" => CtlReq::ReleaseAll,
            "reset" => CtlReq::Reset {
                warm: json_bool(trimmed, "warm").unwrap_or(false),
            },
            "set_battery_mv" => CtlReq::SetBatteryMv {
                mv: json_i64(trimmed, "mv").unwrap_or(7600).clamp(5800, 8500) as u16,
            },
            "set_rf" => CtlReq::SetRf {
                rssi_dbm: json_i64(trimmed, "rssi_dbm").unwrap_or(-120) as i16,
                carrier: json_bool(trimmed, "carrier").unwrap_or(false),
            },
            "set_turbo" => CtlReq::SetTurbo {
                turbo: json_bool(trimmed, "turbo").unwrap_or(false),
            },
            "serial_log" | "logs" => CtlReq::SerialLog,
            "quit" | "stop" => CtlReq::Quit,
            other => {
                let _ = writeln!(writer, "{{\"ok\":false,\"error\":\"unknown cmd {other}\"}}");
                continue;
            }
        };

        let (rtx, rrx) = mpsc::channel();
        if tx.send((req, rtx)).is_err() {
            break;
        }
        if let Ok(resp) = rrx.recv_timeout(Duration::from_secs(3))
            && writeln!(writer, "{resp}").is_err()
        {
            break;
        }
    }
}

struct Config {
    firmware: PathBuf,
    flash: Option<PathBuf>,
    ephemeral: bool,
    flash_mode: FlashInitMode,
    serial_port: u16,
    ctl_port: u16,
    boot_key: Option<String>,
    boot_key_ms: u64,
    turbo: bool,
}

fn parse_args() -> Result<Config, String> {
    let mut firmware: Option<PathBuf> = None;
    let mut flash: Option<PathBuf> = None;
    let mut ephemeral = false;
    let mut flash_mode = FlashInitMode::Normal;
    let mut serial_port: u16 = 4446;
    let mut ctl_port: u16 = 4445;
    let mut boot_key: Option<String> = None;
    let mut boot_key_ms: u64 = 1500;
    let mut turbo = false;

    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--firmware" | "--elf" | "--bin" => {
                firmware = Some(PathBuf::from(args.next().ok_or("missing path after --firmware")?));
            }
            "--flash" => {
                flash = Some(PathBuf::from(args.next().ok_or("missing path after --flash")?));
            }
            "--ephemeral" => ephemeral = true,
            "--flash-mode" => {
                let m = args.next().ok_or("missing mode after --flash-mode")?;
                flash_mode = match m.as_str() {
                    "normal" => FlashInitMode::Normal,
                    "blank" | "blank-calib" => FlashInitMode::BlankWithCalib,
                    "blank-no-calib" | "uncalibrated" => FlashInitMode::BlankNoCalib,
                    "corrupt" => FlashInitMode::Corrupt,
                    other => return Err(format!("unknown --flash-mode '{other}'")),
                };
            }
            "--serial-port" | "--port" => {
                serial_port = args
                    .next()
                    .ok_or("missing port")?
                    .parse()
                    .map_err(|_| "invalid --serial-port")?;
            }
            "--ctl-port" => {
                ctl_port = args
                    .next()
                    .ok_or("missing port")?
                    .parse()
                    .map_err(|_| "invalid --ctl-port")?;
            }
            "--boot-key" => {
                boot_key = Some(args.next().ok_or("missing key after --boot-key")?);
            }
            "--boot-key-ms" => {
                boot_key_ms = args
                    .next()
                    .ok_or("missing ms")?
                    .parse()
                    .map_err(|_| "invalid --boot-key-ms")?;
            }
            "--turbo" => turbo = true,
            "-h" | "--help" => {
                println!(
                    "kern-emu [options]\n  --firmware <elf|bin>\n  --flash <flash.img>\n  --ephemeral\n  --flash-mode <normal|blank|blank-no-calib|corrupt>\n  --serial-port <port> (default 4446)\n  --ctl-port <port>    (default 4445)\n  --boot-key <key> [--boot-key-ms <ms>]\n  --turbo"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let fw = firmware.ok_or("missing --firmware <elf|bin>")?;
    Ok(Config {
        firmware: fw,
        flash,
        ephemeral,
        flash_mode,
        serial_port,
        ctl_port,
        boot_key,
        boot_key_ms,
        turbo,
    })
}

fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kern-emu error: {e}");
            std::process::exit(2);
        }
    };

    let fw_bytes = match fs::read(&cfg.firmware) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("kern-emu: failed to read firmware {}: {e}", cfg.firmware.display());
            std::process::exit(1);
        }
    };

    let mut soc = match Py32Soc::new(&fw_bytes, cfg.flash.as_deref(), cfg.ephemeral, cfg.flash_mode) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kern-emu: failed to initialize SoC: {e}");
            std::process::exit(1);
        }
    };
    let mut cpu = CortexM0Plus::new(&mut soc);

    if let Some(ref k) = cfg.boot_key
        && let Some(bit) = key_name_to_bit(k)
    {
        soc.keypad.press_for_ms(bit, cfg.boot_key_ms, 0);
    }

    let serial_listener = match TcpListener::bind(("127.0.0.1", cfg.serial_port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kern-emu: failed to bind RigWire port {}: {e}", cfg.serial_port);
            std::process::exit(1);
        }
    };
    let _ = serial_listener.set_nonblocking(true);

    let ctl_listener = match TcpListener::bind(("127.0.0.1", cfg.ctl_port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kern-emu: failed to bind control port {}: {e}", cfg.ctl_port);
            std::process::exit(1);
        }
    };

    let (ctl_tx, ctl_rx): (Sender<(CtlReq, Sender<String>)>, Receiver<(CtlReq, Sender<String>)>) =
        mpsc::channel();
    thread::spawn(move || {
        for incoming in ctl_listener.incoming().flatten() {
            let tx = ctl_tx.clone();
            thread::spawn(move || handle_ctl_client(incoming, tx));
        }
    });

    let mut serial_client: Option<TcpStream> = None;
    let mut rx_buf = [0u8; 512];
    let mut turbo = cfg.turbo;
    let mut running = true;
    let started_wall = Instant::now();
    let mut last_pace_wall = Instant::now();
    let mut last_flush_wall = Instant::now();

    while running {
        // 1. Accept / service RigWire TCP connection (non-blocking)
        if let Ok((sock, _)) = serial_listener.accept() {
            let _ = sock.set_nodelay(true);
            let _ = sock.set_nonblocking(true);
            // Discard any stale unread boot banner bytes before fresh RigWire client starts
            soc.uart_tx_fifo.clear();
            serial_client = Some(sock);
        }

        if let Some(ref mut sock) = serial_client {
            let mut drop_sock = false;
            match sock.read(&mut rx_buf) {
                Ok(0) => drop_sock = true,
                Ok(n) => {
                    soc.uart_rx_fifo.extend(&rx_buf[..n]);
                    cpu.halted_wfi = false;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => drop_sock = true,
            }
            if !drop_sock && !soc.uart_tx_fifo.is_empty() {
                let out: Vec<u8> = soc.uart_tx_fifo.drain(..).collect();
                if sock.write_all(&out).is_err() {
                    drop_sock = true;
                }
            }
            if drop_sock {
                serial_client = None;
            }
        } else if soc.uart_tx_fifo.len() > 8192 {
            soc.uart_tx_fifo.clear();
        }

        // 2. Service JSON-RPC hardware control requests
        while let Ok((req, reply)) = ctl_rx.try_recv() {
            let resp = match req {
                CtlReq::Status => {
                    let wall_s = started_wall.elapsed().as_secs_f64().max(0.001);
                    let mips = (cpu.total_insns as f64 / 1_000_000.0) / wall_s;
                    format!(
                        "{{\"ok\":true,\"powered\":true,\"running\":true,\"turbo\":{},\"pc\":{},\"lr\":{},\"ipsr\":{},\"primask\":{},\"sysclk_hz\":{},\"virtual_ms\":{},\"mips\":{:.1},\"backlight\":{},\"flashlight\":{},\"audio_pa\":{},\"led\":\"{}\",\"tuned_hz\":{},\"panel\":{{\"display\":{},\"contrast\":{},\"invert\":{},\"frame_seq\":{}}}}}",
                        turbo,
                        cpu.pc,
                        cpu.lr,
                        cpu.xpsr & 0x3F,
                        cpu.primask,
                        soc.sysclk_hz,
                        soc.virtual_us / 1000,
                        mips,
                        soc.backlight_on(),
                        soc.flashlight_on(),
                        soc.audio_pa_on(),
                        soc.bk4829.status_led(),
                        soc.bk4829.tuned_freq_hz(),
                        soc.lcd.display_on,
                        soc.lcd.contrast,
                        soc.lcd.inverted,
                        soc.lcd.frame_seq
                    )
                }
                CtlReq::Framebuffer => {
                    let fb = soc.lcd.visible_framebuffer();
                    let lit: u32 = fb.iter().map(|b| b.count_ones()).sum();
                    format!(
                        "{{\"ok\":true,\"display\":{},\"contrast\":{},\"invert\":{},\"backlight\":{},\"lit_pixels\":{},\"hex\":\"{}\"}}",
                        soc.lcd.display_on,
                        soc.lcd.contrast,
                        soc.lcd.inverted,
                        soc.backlight_on(),
                        lit,
                        hex_encode(&fb)
                    )
                }
                CtlReq::PressKey { key, hold_ms } => match key_name_to_bit(&key) {
                    Some(bit) => {
                        soc.keypad.press_for_ms(bit, hold_ms, soc.virtual_us / 1000);
                        cpu.halted_wfi = false;
                        "{\"ok\":true}".to_string()
                    }
                    None => format!("{{\"ok\":false,\"error\":\"unknown key {}\"}}", json_escape(&key)),
                },
                CtlReq::SetKey { key, down } => match key_name_to_bit(&key) {
                    Some(bit) => {
                        soc.keypad.set_held(bit, down);
                        cpu.halted_wfi = false;
                        "{\"ok\":true}".to_string()
                    }
                    None => format!("{{\"ok\":false,\"error\":\"unknown key {}\"}}", json_escape(&key)),
                },
                CtlReq::ReleaseAll => {
                    soc.keypad.release_all();
                    "{\"ok\":true}".to_string()
                }
                CtlReq::Reset { warm } => {
                    if warm {
                        soc.warm_reset_peripherals();
                    } else {
                        soc.sram.fill(0);
                        soc.rtc_bkp = [0; 32];
                        soc.rtc_bkp[1] = 1 << 5;
                        soc.rcc_csr = 1 << 27;
                        soc.lcd.reset();
                    }
                    cpu.reset(&mut soc);
                    "{\"ok\":true}".to_string()
                }
                CtlReq::SetBatteryMv { mv } => {
                    soc.set_battery_mv(mv);
                    "{\"ok\":true}".to_string()
                }
                CtlReq::SetRf { rssi_dbm, carrier } => {
                    soc.bk4829.set_rf_signal(rssi_dbm, carrier);
                    "{\"ok\":true}".to_string()
                }
                CtlReq::SetTurbo { turbo: t } => {
                    turbo = t;
                    last_pace_wall = Instant::now();
                    "{\"ok\":true}".to_string()
                }
                CtlReq::SerialLog => {
                    let items: Vec<String> = soc
                        .serial_lines
                        .iter()
                        .map(|s| format!("\"{}\"", json_escape(s)))
                        .collect();
                    format!("{{\"ok\":true,\"lines\":[{}]}}", items.join(","))
                }
                CtlReq::Quit => {
                    running = false;
                    "{\"ok\":true}".to_string()
                }
            };
            let _ = reply.send(resp);
        }

        if !running {
            break;
        }

        // 3. Execute CPU slice (~0.25 ms bursts when RigWire client connected so UART frame timers stay real-time)
        let client_connected = serial_client.is_some();
        let effective_turbo = turbo && !client_connected;
        let has_wire_traffic = !soc.uart_rx_fifo.is_empty() || soc.usart1_rx_dr.is_some();
        let slice_us: u64 = if effective_turbo {
            5_000
        } else if has_wire_traffic {
            250
        } else {
            1_000
        };
        let target_us = soc.virtual_us.wrapping_add(slice_us);
        let mut guard = 0u32;
        while soc.virtual_us < target_us && guard < 300_000 {
            cpu.step(&mut soc, effective_turbo);
            guard += 1;
            if has_wire_traffic && !soc.uart_tx_fifo.is_empty() && (guard & 0x3F) == 0 {
                // Flush response bytes out to TCP socket immediately!
                break;
            }
        }
        soc.keypad.tick_ms(soc.virtual_us / 1000);

        // 4. Periodic SPI Flash flush (every 500 ms if dirty)
        if last_flush_wall.elapsed() >= Duration::from_millis(500) {
            soc.spiflash.flush();
            last_flush_wall = Instant::now();
        }

        // 5. Real-time pacing when not in effective turbo mode (keeps idle host CPU < 1.5%)
        if !effective_turbo && !has_wire_traffic {
            let elapsed_wall = last_pace_wall.elapsed();
            let slice_dur = Duration::from_micros(slice_us);
            if elapsed_wall < slice_dur {
                thread::sleep(slice_dur - elapsed_wall);
            }
            last_pace_wall = Instant::now();
        } else {
            last_pace_wall = Instant::now();
        }
    }

    soc.spiflash.flush();
}
