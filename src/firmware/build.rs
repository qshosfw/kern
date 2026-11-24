use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let is_k10pro = env::var_os("CARGO_FEATURE_TARGET_K10PRO").is_some();
    let (flash_origin, flash_len, target_label) = if is_k10pro {
        ("0x08002000", "120K", "UV-K10 Pro / PY32")
    } else {
        ("0x08002800", "118K", "UV-K1 / PY32")
    };
    let memory_x = format!(
        "MEMORY\n{{\n  FLASH (rx) : ORIGIN = {}, LENGTH = {}\n  RAM (rwx)  : ORIGIN = 0x20000000, LENGTH = 16K\n}}\n",
        flash_origin, flash_len
    );

    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(memory_x.as_bytes())
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_TARGET_K10PRO");
    println!("cargo:rerun-if-changed=memory.x");

    // Capture Git commit hash
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "release".to_string());

    let git_branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "head".to_string());

    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false);

    let full_hash = if git_dirty {
        format!("{}-mod", git_hash)
    } else {
        git_hash
    };

    const MAX_REF_LEN: usize = 21;
    let suffix_len = 1 + full_hash.len();
    let branch_max = MAX_REF_LEN.saturating_sub(suffix_len);
    let git_ref = if branch_max > 0 {
        let b_trunc: String = git_branch.chars().take(branch_max).collect();
        format!("{}-{}", b_trunc, full_hash)
    } else {
        full_hash.chars().take(MAX_REF_LEN).collect()
    };

    // Calculate UTC Date and Time without external dependencies
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let days = (now / 86400) as i64;
    let time_of_day = (now % 86400) as u32;
    let hours = time_of_day / 3600;
    let mins = (time_of_day % 3600) / 60;
    let secs_rem = time_of_day % 60;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let (date_str, time_str, stamp_display) =
        if let Ok(out) = Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let parts: Vec<&str> = s.split(' ').collect();
                let d = parts.get(0).copied().unwrap_or("2026-10-05");
                let t = parts.get(1).copied().unwrap_or("00:00:00");
                (d.to_string(), t.to_string(), s)
            } else {
                (
                    format!("{:04}-{:02}-{:02}", y, m, d),
                    format!("{:02}:{:02}:{:02}", hours, mins, secs_rem),
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        y, m, d, hours, mins, secs_rem
                    ),
                )
            }
        } else {
            (
                format!("{:04}-{:02}-{:02}", y, m, d),
                format!("{:02}:{:02}:{:02}", hours, mins, secs_rem),
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    y, m, d, hours, mins, secs_rem
                ),
            )
        };

    println!("cargo:rustc-env=KERN_GIT_HASH={}", full_hash);
    println!("cargo:rustc-env=KERN_GIT_REF={}", git_ref);
    println!("cargo:rustc-env=KERN_BUILD_EPOCH={}", now);
    println!("cargo:rustc-env=KERN_BUILD_DATE={}", date_str);
    println!("cargo:rustc-env=KERN_BUILD_TIME={}", time_str);
    println!("cargo:rustc-env=KERN_BUILD_STAMP={}", stamp_display);
    println!(
        "cargo:rustc-env=KERN_ABOUT_TEXT=KERN OS\nBuild: {}\nGit: {}\nTarget: {}",
        date_str,
        full_hash,
        target_label
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
