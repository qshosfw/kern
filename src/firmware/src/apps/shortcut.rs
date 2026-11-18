//! Lean Contextual Key Shortcuts & Microkernel IPC Commands.
//!
//! Stores user bindings as compact 16-bit IDs (`0` = None, `1..=10` = Builtin,
//! `>= 0x8000` = 15-bit FNV-1a hash of `krx_path` + optional `action_id`), enabling
//! 64-byte `KeyConfig v3` blocks with up to 3 contextual override rules.

use crate::apps::launcher::catalog::{PackageCatalog, PackageCatalogEntry};
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO};
use crate::config::keys::KeyConfig;
use gfx::gui::modules::menu::{OptionSheet, ShortString};
use kernel::ipc::SysEvent;
use kernel::proc::krx::{KrxActionDecl, KrxExecType};

pub const KEY_SHORTCUT_COUNT: usize = 19;
pub const F_DRAWER_START_SLOT: usize = 5;

/// Packed trigger slot labels shared by settings lists, rule picker, and F-key drawer.
pub static SLOT_OPTIONS: &str =
    "S1\0S1+\0S2\0S2+\0M+\0F+1\0F+2\0F+3\0F+4\0F+5\0F+6\0F+7\0F+8\0F+9\0F+0\0F+*\0F+M\0F+S1\0F+S2\0";

/// Packed context scope labels shared by rule summary, rule editor, and F-key drawer.
pub static CTX_OPTIONS: &str = "Off\0VFO\0Home\0Prefs\0App\0";

#[inline(never)]
pub fn slot_label(slot: usize) -> &'static str {
    utils::format::nth_cstr(SLOT_OPTIONS.as_bytes(), slot.min(KEY_SHORTCUT_COUNT - 1) as u8)
}

#[inline(never)]
pub fn ctx_label(ctx: u8) -> &'static str {
    utils::format::nth_cstr(CTX_OPTIONS.as_bytes(), ctx.min(4))
}

#[inline(always)]
pub fn drawer_badge(slot: usize) -> &'static str {
    let s = slot_label(slot);
    if slot >= F_DRAWER_START_SLOT && s.len() > 2 {
        unsafe { core::str::from_utf8_unchecked(&s.as_bytes()[2..]) }
    } else {
        s
    }
}

/// Active screen/application context scopes for contextual shortcut overrides.
pub const CTX_GLOBAL: u8 = 0;
pub const CTX_VFO: u8 = 1;
pub const CTX_HOME: u8 = 2;
pub const CTX_SETTINGS: u8 = 3;
pub const CTX_APP: u8 = 4;

#[inline(always)]
pub const fn active_context(app_id: usize) -> u8 {
    match app_id {
        APP_ID_VFO => CTX_VFO,
        APP_ID_LAUNCHER => CTX_HOME,
        APP_ID_SETTINGS => CTX_SETTINGS,
        _ => CTX_APP,
    }
}

/// Internal Microkernel IPC Command Enum (`SysEvent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    None,
    Flashlight,
    Monitor,
    Settings,
    LockKeypad,
    ToggleVfo,
    ToggleTxPower,
    MuteAudio,
    Sleep,
    Reboot,
    LaunchApp(u8),
    PlayBeep { freq_hz: u16, duration_ms: u16 },
    CloseApp,
}

impl Shortcut {
    pub fn to_cmd_arg(self) -> (u8, u16) {
        match self {
            Shortcut::None => (0, 0),
            Shortcut::Flashlight => (1, 0),
            Shortcut::Monitor => (2, 0),
            Shortcut::Settings => (5, 0),
            Shortcut::LockKeypad => (6, 0),
            Shortcut::ToggleVfo => (7, 0),
            Shortcut::ToggleTxPower => (8, 0),
            Shortcut::MuteAudio => (9, 0),
            Shortcut::Sleep => (13, 0),
            Shortcut::Reboot => (14, 0),
            Shortcut::LaunchApp(app_id) => (15, app_id as u16),
            Shortcut::PlayBeep {
                freq_hz,
                duration_ms,
            } => (16, ((freq_hz / 10) << 6) | ((duration_ms / 10).min(63))),
            Shortcut::CloseApp => (17, 0),
        }
    }
}

impl From<Shortcut> for SysEvent {
    fn from(val: Shortcut) -> Self {
        let (command, arg) = val.to_cmd_arg();
        SysEvent {
            target: 0,
            command,
            arg,
        }
    }
}

impl From<SysEvent> for Shortcut {
    fn from(evt: SysEvent) -> Self {
        match evt.command {
            1 => Shortcut::Flashlight,
            2 => Shortcut::Monitor,
            5 => Shortcut::Settings,
            6 => Shortcut::LockKeypad,
            7 => Shortcut::ToggleVfo,
            8 => Shortcut::ToggleTxPower,
            9 => Shortcut::MuteAudio,
            13 => Shortcut::Sleep,
            14 => Shortcut::Reboot,
            15 => Shortcut::LaunchApp(evt.arg as u8),
            16 => Shortcut::PlayBeep {
                freq_hz: (evt.arg >> 6) * 10,
                duration_ms: (evt.arg & 0x3F) * 10,
            },
            17 => Shortcut::CloseApp,
            _ => Shortcut::None,
        }
    }
}

pub const TARGET_NONE: u16 = 0;
pub const TARGET_FLASHLIGHT: u16 = 1;
pub const TARGET_MONITOR: u16 = 2;
pub const TARGET_SETTINGS: u16 = 3;
pub const TARGET_LOCK_KEYPAD: u16 = 4;
pub const TARGET_TOGGLE_VFO: u16 = 5;
pub const TARGET_TX_POWER: u16 = 6;
pub const TARGET_MUTE: u16 = 7;
pub const TARGET_RADIO: u16 = 8;
pub const TARGET_LAUNCHER: u16 = 9;
pub const TARGET_SLEEP: u16 = 10;

pub struct BuiltinTarget {
    pub label: &'static str,
    pub cmd: Shortcut,
    pub icon_idx: u8,
}

/// Ordered 1..=10 matching `TARGET_FLASHLIGHT..=TARGET_SLEEP` for $O(1)$ indexing.
pub const BUILTIN_TARGETS: [BuiltinTarget; 10] = [
    BuiltinTarget { label: "Flashlight", cmd: Shortcut::Flashlight, icon_idx: 6 },
    BuiltinTarget { label: "Monitor", cmd: Shortcut::Monitor, icon_idx: 0xFF },
    BuiltinTarget { label: "Settings", cmd: Shortcut::Settings, icon_idx: 4 },
    BuiltinTarget { label: "Lock Keys", cmd: Shortcut::LockKeypad, icon_idx: 25 },
    BuiltinTarget { label: "VFO A/B", cmd: Shortcut::ToggleVfo, icon_idx: 2 },
    BuiltinTarget { label: "TX Power", cmd: Shortcut::ToggleTxPower, icon_idx: 7 },
    BuiltinTarget { label: "Mute", cmd: Shortcut::MuteAudio, icon_idx: 21 },
    BuiltinTarget { label: "Radio", cmd: Shortcut::LaunchApp(APP_ID_VFO as u8), icon_idx: 3 },
    BuiltinTarget { label: "Launcher", cmd: Shortcut::LaunchApp(APP_ID_LAUNCHER as u8), icon_idx: 24 },
    BuiltinTarget { label: "Sleep", cmd: Shortcut::Sleep, icon_idx: 22 },
];

pub const fn hash_app_action(path: &str, action_id: u16) -> u16 {
    let bytes = path.as_bytes();
    let mut h = 2_166_136_261u32;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(16_777_619);
        i += 1;
    }
    if action_id != 0 {
        h ^= action_id as u32;
        h = h.wrapping_mul(16_777_619);
    }
    ((h ^ (h >> 15)) as u16) | 0x8000
}

#[inline(always)]
pub const fn hash_app(path: &str) -> u16 {
    hash_app_action(path, 0)
}

/// Resolve the effective 16-bit target for `slot` under active context `cur_ctx`.
#[inline(never)]
pub fn resolve_slot(keys: &KeyConfig, slot: usize, cur_ctx: u8) -> u16 {
    if slot >= KEY_SHORTCUT_COUNT {
        return TARGET_NONE;
    }
    if cur_ctx != CTX_GLOBAL {
        for r in &keys.ctx_rules {
            if r.ctx == cur_ctx && r.slot as usize == slot {
                return r.action;
            }
        }
    }
    keys.actions[slot]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedShortcut<'a> {
    None,
    Builtin(Shortcut),
    App { path: &'a str, action_id: u16 },
}

enum TargetMeta<'a> {
    None,
    Missing,
    Builtin(&'static BuiltinTarget),
    Dynamic(&'a PackageCatalogEntry, u16, &'a str),
}

#[inline(always)]
fn builtin_icon(icon_idx: u8) -> &'static [u8; 8] {
    if icon_idx == 0xFF {
        gfx::icons::ICON_ANTENNA
    } else {
        gfx::icons::SYSTEM_ICONS_8
            .get(icon_idx as usize)
            .unwrap_or(gfx::icons::ICON_FOLDER)
    }
}

#[inline(never)]
pub fn format_label(title: &str, suffix: &str) -> ShortString {
    let mut s = ShortString::from_str(title);
    if !suffix.is_empty() {
        s.push_byte(b':');
        s.push_str(suffix);
    }
    s
}

#[inline(never)]
fn find_target<'a>(id: u16, cat: &'a PackageCatalog) -> TargetMeta<'a> {
    if id == TARGET_NONE {
        return TargetMeta::None;
    }
    if (id & 0x8000) == 0 {
        return BUILTIN_TARGETS
            .get((id as usize).wrapping_sub(1))
            .map_or(TargetMeta::None, TargetMeta::Builtin);
    }
    for e in cat.entries.as_slice() {
        if e.is_valid && e.exec_type == KrxExecType::App && hash_app(e.path.as_str()) == id {
            return TargetMeta::Dynamic(e, 0, "");
        }
    }
    for act in cat.actions.as_slice() {
        if let Some(e) = cat.entries.get(act.pkg_idx as usize) {
            if e.is_valid && hash_app_action(e.path.as_str(), act.action_id) == id {
                return TargetMeta::Dynamic(e, act.action_id, act.label_str());
            }
        }
    }
    TargetMeta::Missing
}

#[inline(never)]
pub fn resolve<'a>(id: u16, cat: &'a PackageCatalog) -> ResolvedShortcut<'a> {
    match find_target(id, cat) {
        TargetMeta::Builtin(b) => ResolvedShortcut::Builtin(b.cmd),
        TargetMeta::Dynamic(e, act, _) => ResolvedShortcut::App {
            path: e.path.as_str(),
            action_id: act,
        },
        _ => ResolvedShortcut::None,
    }
}

#[inline(never)]
pub fn describe(id: u16, cat: &PackageCatalog) -> (ShortString, &'static [u8; 8]) {
    match find_target(id, cat) {
        TargetMeta::None => (ShortString::from_str("None"), gfx::icons::RADIO_OFF),
        TargetMeta::Missing => (ShortString::from_str("Missing"), gfx::icons::ICON_FOLDER),
        TargetMeta::Builtin(b) => (ShortString::from_str(b.label), builtin_icon(b.icon_idx)),
        TargetMeta::Dynamic(e, _, suffix) => (
            format_label(e.title.as_str(), suffix),
            crate::apps::settings::apps::package_icon_8(e.title.as_str(), e.icon_id),
        ),
    }
}

pub const MAX_PICKER_ITEMS: usize = 28;

#[repr(C)]
struct PickerScratch {
    icons: [&'static [u8; 8]; MAX_PICKER_ITEMS],
    ids: [u16; MAX_PICKER_ITEMS],
    labels: [ShortString; MAX_PICKER_ITEMS],
    count: usize,
}

const _: () = assert!(
    core::mem::size_of::<PickerScratch>()
        <= crate::data::arena::TRANSIENT_ARENA_BYTES - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
);

#[inline(always)]
fn picker_scratch() -> &'static mut PickerScratch {
    unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr() as *mut PickerScratch)
    }
}

#[inline(never)]
fn push_picker(
    ps: &mut PickerScratch,
    n: &mut usize,
    id: u16,
    title: &str,
    suffix: &str,
    icon: &'static [u8; 8],
) {
    if *n < MAX_PICKER_ITEMS {
        ps.ids[*n] = id;
        ps.labels[*n] = format_label(title, suffix);
        ps.icons[*n] = icon;
        *n += 1;
    }
}

/// Populate icon-equipped `OptionSheet` inside transient tail scratch (`0 B` static `.bss`).
#[inline(never)]
pub fn build_picker_sheet(
    title: &'static str,
    current_id: u16,
    catalog: &PackageCatalog,
) -> OptionSheet {
    let ps = picker_scratch();
    let mut n = 0usize;

    push_picker(ps, &mut n, TARGET_NONE, "None", "", gfx::icons::RADIO_OFF);
    for (idx, b) in BUILTIN_TARGETS.iter().enumerate() {
        push_picker(ps, &mut n, (idx + 1) as u16, b.label, "", builtin_icon(b.icon_idx));
    }
    for (pkg_idx, e) in catalog.entries.iter().enumerate() {
        if !e.is_valid || e.exec_type != KrxExecType::App {
            continue;
        }
        let icon = crate::apps::settings::apps::package_icon_8(e.title.as_str(), e.icon_id);
        push_picker(ps, &mut n, hash_app(e.path.as_str()), e.title.as_str(), "", icon);
        for act in catalog.actions.iter() {
            if act.pkg_idx as usize == pkg_idx && (act.scope_mask & KrxActionDecl::SCOPE_SHORTCUT) != 0
            {
                push_picker(
                    ps,
                    &mut n,
                    hash_app_action(e.path.as_str(), act.action_id),
                    e.title.as_str(),
                    act.label_str(),
                    icon,
                );
            }
        }
    }

    ps.count = n;
    let selected = ps.ids[..n].iter().position(|&id| id == current_id).unwrap_or(0);
    OptionSheet::new_dynamic(title, &ps.labels[..n], selected)
        .with_radio(true)
        .with_icons(&ps.icons[..n])
}

pub fn picker_target_at(idx: usize) -> Option<u16> {
    let ps = picker_scratch();
    if idx < ps.count {
        Some(ps.ids[idx])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::launcher::catalog::{CatalogAction, PackageCatalogEntry};
    use kernel::proc::krx::KrxClass;

    #[test]
    fn shortcut_ipc_roundtrip() {
        for sc in [
            Shortcut::None,
            Shortcut::Flashlight,
            Shortcut::Monitor,
            Shortcut::Settings,
            Shortcut::LockKeypad,
            Shortcut::ToggleVfo,
            Shortcut::ToggleTxPower,
            Shortcut::MuteAudio,
            Shortcut::Sleep,
            Shortcut::Reboot,
            Shortcut::LaunchApp(42),
            Shortcut::CloseApp,
        ] {
            let evt: SysEvent = sc.into();
            assert_eq!(Shortcut::from(evt), sc);
        }
    }

    #[test]
    fn dynamic_shortcut_resolves_app_and_declared_action_with_icons() {
        let mut cat = PackageCatalog::new();
        let mut title = heapless::String::<16>::new();
        let _ = title.push_str("Tasks");
        let mut path = heapless::String::<24>::new();
        let _ = path.push_str("/apps/tasks.krx");
        let _ = cat.entries.push(PackageCatalogEntry {
            title,
            path,
            size_bytes: 2048,
            app_class: KrxClass::Paged,
            exec_type: KrxExecType::App,
            lifecycle_flags: 0,
            icon_id: 29,
            caps: 0,
            is_valid: true,
        });
        let mut label = [0u8; 12];
        label[..5].copy_from_slice(b"Sched");
        let _ = cat.actions.push(CatalogAction {
            pkg_idx: 0,
            scope_mask: KrxActionDecl::SCOPE_SHORTCUT,
            action_id: 1,
            label,
        });

        let id_app = hash_app("/apps/tasks.krx");
        let id_act = hash_app_action("/apps/tasks.krx", 1);
        assert_ne!(id_app, id_act);
        assert_eq!(
            resolve(id_app, &cat),
            ResolvedShortcut::App {
                path: "/apps/tasks.krx",
                action_id: 0
            }
        );
        assert_eq!(
            resolve(id_act, &cat),
            ResolvedShortcut::App {
                path: "/apps/tasks.krx",
                action_id: 1
            }
        );

        let (desc, icon) = describe(id_act, &cat);
        assert_eq!(desc.as_str(), "Tasks:Sched");
        assert_eq!(icon.as_ptr(), gfx::icons::SYSTEM_ICONS_8[29 - 22].as_ptr());

        let sheet = build_picker_sheet("S1", id_act, &cat);
        assert_eq!(sheet.selected, sheet.options.len() - 1);
        assert_eq!(picker_target_at(sheet.selected), Some(id_act));
        assert_eq!(drawer_badge(5), "1");
        assert_eq!(drawer_badge(0), "S1");
    }
}
