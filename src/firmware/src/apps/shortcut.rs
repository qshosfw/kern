//! Lean Contextual Key Shortcuts & Microkernel IPC Commands.
//!
//! Stores user bindings as compact 16-bit IDs (`0` = None, `1..=10` = Builtin,
//! `>= 0x8000` = 15-bit FNV-1a hash of `krx_path` + optional `action_id`), enabling
//! 64-byte `KeyConfig v3` blocks with up to 3 contextual override rules.

use crate::apps::launcher::catalog::{PackageCatalog, PackageCatalogEntry};
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO};
use crate::config::keys::KeyConfig;
#[cfg(test)]
use gfx::gui::modules::menu::OptionSheet;
use gfx::gui::modules::menu::ShortString;
use kernel::ipc::SysEvent;
use kernel::runtime::KrxExecType;

pub const KEY_SHORTCUT_COUNT: usize = 19;
pub const F_DRAWER_START_SLOT: usize = 5;

/// Packed trigger slot labels shared by settings lists, rule picker, and F-key drawer.
pub const SLOT_OPTIONS: &str = "S1\0S1+\0S2\0S2+\0M+\0F+1\0F+2\0F+3\0F+4\0F+5\0F+6\0F+7\0F+8\0F+9\0F+0\0F+*\0F+M\0F+S1\0F+S2\0";

/// Packed context scope labels shared by settings selector and shortcut engine.
#[cfg(test)]
pub const CTX_OPTIONS: &str = "Global\0VFO\0Home\0Prefs\0App\0";

#[inline(never)]
pub fn slot_label(slot: usize) -> &'static str {
    utils::format::nth_cstr(
        SLOT_OPTIONS.as_bytes(),
        slot.min(KEY_SHORTCUT_COUNT - 1) as u8,
    )
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
#[repr(u8)]
pub enum Shortcut {
    None = 0,
    Flashlight = 1,
    LockKeypad = 2,
    Sleep = 3,
    CloseApp = 4,
    LaunchApp(u8) = 5,
    LaunchPendingPkg = 6,
}

impl From<Shortcut> for SysEvent {
    #[inline]
    fn from(val: Shortcut) -> Self {
        let (command, arg) = match val {
            Shortcut::None => (0, 0),
            Shortcut::Flashlight => (1, 0),
            Shortcut::LockKeypad => (2, 0),
            Shortcut::Sleep => (3, 0),
            Shortcut::CloseApp => (4, 0),
            Shortcut::LaunchApp(app_id) => (5, app_id as u16),
            Shortcut::LaunchPendingPkg => (6, 0),
        };
        SysEvent {
            target: 0,
            command,
            arg,
        }
    }
}

impl From<SysEvent> for Shortcut {
    #[inline]
    fn from(evt: SysEvent) -> Self {
        match evt.command {
            1 => Shortcut::Flashlight,
            2 => Shortcut::LockKeypad,
            3 => Shortcut::Sleep,
            4 => Shortcut::CloseApp,
            5 => Shortcut::LaunchApp(evt.arg as u8),
            6 => Shortcut::LaunchPendingPkg,
            _ => Shortcut::None,
        }
    }
}

pub const TARGET_NONE: u16 = 0;
pub const TARGET_FLASHLIGHT: u16 = 1;
pub const TARGET_SETTINGS: u16 = 2;
pub const TARGET_LOCK_KEYPAD: u16 = 3;
pub const TARGET_HOME: u16 = 4;
pub const TARGET_SLEEP: u16 = 5;

pub struct BuiltinTarget {
    pub cmd: Shortcut,
    /// `abi::icon` system glyph shown beside the target.
    pub icon_id: u8,
}

utils::static_bytes!(
    static BUILTIN_LABELS = b"Flashlight\0Settings\0Lock Keys\0Home\0Sleep\0None\0Missing\0";
);

/// Pure OS built-in system shortcuts ordered 1..=5 matching `TARGET_FLASHLIGHT..=TARGET_SLEEP`.
pub static BUILTIN_TARGETS: [BuiltinTarget; 5] = [
    BuiltinTarget {
        cmd: Shortcut::Flashlight,
        icon_id: ::abi::icon::APP_TORCH,
    },
    BuiltinTarget {
        cmd: Shortcut::LaunchApp(APP_ID_SETTINGS as u8),
        icon_id: ::abi::icon::APP_SETTINGS,
    },
    BuiltinTarget {
        cmd: Shortcut::LockKeypad,
        icon_id: ::abi::icon::SECURITY,
    },
    BuiltinTarget {
        cmd: Shortcut::LaunchApp(APP_ID_LAUNCHER as u8),
        icon_id: ::abi::icon::HOME,
    },
    BuiltinTarget {
        cmd: Shortcut::Sleep,
        icon_id: ::abi::icon::SLEEP,
    },
];

#[inline(never)]
pub const fn hash_app_action(path: &str, action_id: u16) -> u16 {
    ::abi::hash_app_action(path.as_bytes(), action_id)
}

#[inline(always)]
pub const fn hash_app(path: &str) -> u16 {
    hash_app_action(path, 0)
}

/// Resolve effective 16-bit target for `slot` under `cur_ctx`, returning `(id, overridden)`.
#[inline(never)]
pub fn resolve_slot_override(keys: &KeyConfig, slot: usize, cur_ctx: u8) -> (u16, bool) {
    if slot >= KEY_SHORTCUT_COUNT {
        return (TARGET_NONE, false);
    }
    if cur_ctx != CTX_GLOBAL {
        for r in &keys.ctx_rules {
            if r.ctx == cur_ctx && r.slot as usize == slot {
                return (r.action, true);
            }
        }
    }
    (keys.actions[slot], false)
}

/// Resolve the effective 16-bit target for `slot` under active context `cur_ctx`.
#[inline(always)]
pub fn resolve_slot(keys: &KeyConfig, slot: usize, cur_ctx: u8) -> u16 {
    resolve_slot_override(keys, slot, cur_ctx).0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedShortcut<'a> {
    None,
    Builtin(Shortcut),
    App { path: &'a str, action_id: u16 },
}

#[inline(always)]
fn builtin_icon(icon_id: u8) -> &'static [u8; 8] {
    gfx::icons::system_icon(icon_id).unwrap_or(gfx::icons::ICON_FOLDER)
}

#[cfg(test)]
#[inline(never)]
pub fn format_label_into(out: &mut ShortString, title: &str, suffix: &str) {
    out.set_str(title);
    if !suffix.is_empty() {
        out.push_byte(b':');
        out.push_str(suffix);
    }
}

#[inline(never)]
fn find_dynamic<'a>(
    id: u16,
    cat: &'a PackageCatalog,
) -> Option<(&'a PackageCatalogEntry, u16, &'a str)> {
    for e in cat.entries.as_slice() {
        if e.is_valid && e.exec_type == KrxExecType::App && hash_app(e.path.as_str()) == id {
            return Some((e, 0, ""));
        }
    }
    for act in cat.actions.as_slice() {
        if let Some(e) = cat.entries.get(act.pkg_idx as usize) {
            if e.is_valid && hash_app_action(e.path.as_str(), act.action_id) == id {
                return Some((e, act.action_id, act.label_str()));
            }
        }
    }
    None
}

#[inline(never)]
pub fn resolve<'a>(id: u16, cat: &'a PackageCatalog) -> ResolvedShortcut<'a> {
    if id == TARGET_NONE {
        return ResolvedShortcut::None;
    }
    if (id & 0x8000) == 0 {
        return BUILTIN_TARGETS
            .get((id as usize).wrapping_sub(1))
            .map_or(ResolvedShortcut::None, |b| ResolvedShortcut::Builtin(b.cmd));
    }
    match find_dynamic(id, cat) {
        Some((e, act, _)) => ResolvedShortcut::App {
            path: e.path.as_str(),
            action_id: act,
        },
        None => ResolvedShortcut::None,
    }
}

#[inline(never)]
pub fn describe_into(
    id: u16,
    cat: &PackageCatalog,
    starred: bool,
    out: &mut ShortString,
) -> &'static [u8; 8] {
    out.set_str(if starred { "*:" } else { "" });
    let mut lbl_idx = 5u8;
    if id != TARGET_NONE {
        if (id & 0x8000) == 0 {
            let i = (id as usize).wrapping_sub(1);
            if let Some(b) = BUILTIN_TARGETS.get(i) {
                out.push_str(utils::format::nth_cstr(&BUILTIN_LABELS, i as u8));
                return builtin_icon(b.icon_id);
            }
        } else if let Some((e, _, suffix)) = find_dynamic(id, cat) {
            out.push_str(e.title.as_str());
            if !suffix.is_empty() {
                out.push_byte(b':');
                out.push_str(suffix);
            }
            return e.resolved_icon_8();
        } else {
            lbl_idx = 6;
        }
    }
    out.push_str(utils::format::nth_cstr(&BUILTIN_LABELS, lbl_idx));
    if lbl_idx == 6 {
        gfx::icons::ICON_FOLDER
    } else {
        gfx::icons::RADIO_OFF
    }
}

#[cfg(test)]
#[inline(never)]
pub fn assign_rule(keys: &mut KeyConfig, cur_ctx: u8, slot: usize, target_id: u16) -> u8 {
    if slot >= KEY_SHORTCUT_COUNT {
        return 0;
    }
    if cur_ctx == 0 {
        if keys.actions[slot] != target_id {
            keys.actions[slot] = target_id;
            return 1;
        }
        return 0;
    }
    let clear = keys.actions[slot] == target_id;
    let mut free_idx = usize::MAX;
    for i in 0..keys.ctx_rules.len() {
        let r = &mut keys.ctx_rules[i];
        if r.ctx == cur_ctx && r.slot as usize == slot {
            r.ctx = if clear { 0 } else { cur_ctx };
            r.action = if clear { 0 } else { target_id };
            return 1;
        }
        if free_idx == usize::MAX && r.ctx == 0 {
            free_idx = i;
        }
    }
    if clear {
        return 0;
    }
    if free_idx < keys.ctx_rules.len() {
        keys.ctx_rules[free_idx] = crate::config::keys::CtxRule {
            ctx: cur_ctx,
            slot: slot as u8,
            action: target_id,
        };
        1
    } else {
        2
    }
}

#[cfg(test)]
pub fn describe(id: u16, cat: &PackageCatalog) -> (ShortString, &'static [u8; 8]) {
    let mut s = ShortString::empty();
    let icon = describe_into(id, cat, false, &mut s);
    (s, icon)
}

#[cfg(test)]
pub const MAX_PICKER_ITEMS: usize = 28;

#[cfg(test)]
#[repr(C)]
struct PickerScratch {
    icons: [&'static [u8; 8]; MAX_PICKER_ITEMS],
    ids: [u16; MAX_PICKER_ITEMS],
    labels: [ShortString; MAX_PICKER_ITEMS],
    count: usize,
}

#[cfg(test)]
const _: () = assert!(
    core::mem::size_of::<PickerScratch>()
        <= crate::data::arena::TRANSIENT_ARENA_BYTES - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
);

#[cfg(test)]
#[inline(always)]
fn picker_scratch() -> &'static mut PickerScratch {
    unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr() as *mut PickerScratch)
    }
}

#[cfg(test)]
#[inline(never)]
fn push_entry(id: u16, icon: &'static [u8; 8], title: &str, suffix: &str) {
    let ps = picker_scratch();
    let n = ps.count;
    if n < MAX_PICKER_ITEMS {
        ps.ids[n] = id;
        ps.icons[n] = icon;
        format_label_into(&mut ps.labels[n], title, suffix);
        ps.count = n + 1;
    }
}

/// Populate icon-equipped `OptionSheet` inside transient tail scratch (`0 B` static `.bss`).
#[cfg(test)]
pub fn build_picker_sheet(
    title: &'static str,
    current_id: u16,
    catalog: &PackageCatalog,
) -> OptionSheet {
    let ps = picker_scratch();
    ps.count = 0;

    push_entry(
        TARGET_NONE,
        gfx::icons::RADIO_OFF,
        utils::format::nth_cstr(&BUILTIN_LABELS, 5),
        "",
    );
    for (i, b) in BUILTIN_TARGETS.iter().enumerate() {
        push_entry(
            (i + 1) as u16,
            builtin_icon(b.icon_id),
            utils::format::nth_cstr(&BUILTIN_LABELS, i as u8),
            "",
        );
    }
    for (pkg_idx, e) in catalog.entries.iter().enumerate() {
        if !e.is_valid || e.exec_type != KrxExecType::App {
            continue;
        }
        let icon = e.resolved_icon_8();
        let t = e.title.as_str();
        let p = e.path.as_str();
        push_entry(hash_app(p), icon, t, "");
        for act in catalog.actions.iter() {
            if act.pkg_idx as usize == pkg_idx && (act.scope_mask & ::abi::scope::SHORTCUT) != 0 {
                push_entry(hash_app_action(p, act.action_id), icon, t, act.label_str());
            }
        }
    }

    let n = ps.count;
    let mut selected = 0usize;
    for i in 0..n {
        if ps.ids[i] == current_id {
            selected = i;
            break;
        }
    }
    OptionSheet::new_dynamic(title, &ps.labels[..n], selected)
        .with_radio(true)
        .with_icons(&ps.icons[..n])
}

#[cfg(test)]
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

    #[test]
    fn sleep_shortcut_uses_moon_glyph() {
        let sleep = &BUILTIN_TARGETS[(TARGET_SLEEP - 1) as usize];
        assert!(matches!(sleep.cmd, Shortcut::Sleep));
        assert_eq!(
            builtin_icon(sleep.icon_id).as_ptr(),
            gfx::icons::ICON_MOON.as_ptr()
        );
    }

    #[test]
    fn shortcut_ipc_roundtrip() {
        for sc in [
            Shortcut::None,
            Shortcut::Flashlight,
            Shortcut::LockKeypad,
            Shortcut::Sleep,
            Shortcut::LaunchApp(42),
            Shortcut::CloseApp,
        ] {
            let evt: SysEvent = sc.into();
            assert_eq!(Shortcut::from(evt), sc);
        }
    }

    #[test]
    fn dynamic_shortcut_resolves_app_and_declared_action_with_icons() {
        use utils::format::InlineStr;
        let mut cat = PackageCatalog::new();
        let _ = cat.entries.push(PackageCatalogEntry {
            title: InlineStr::from_str("Tasks"),
            path: InlineStr::from_str("/apps/tasks.krx"),
            exec_type: KrxExecType::App,
            icon_16_off: 0,
            icon_id: 29,
            is_valid: true,
            icon_8: [0; 8],
        });
        let mut label = [0u8; 12];
        label[..5].copy_from_slice(b"Sched");
        let _ = cat.actions.push(CatalogAction {
            pkg_idx: 0,
            scope_mask: ::abi::scope::SHORTCUT,
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
        assert_eq!(icon.as_ptr(), gfx::icons::ICON_APP_DIAG.as_ptr());

        let sheet = build_picker_sheet("S1", id_act, &cat);
        assert_eq!(sheet.selected, sheet.options.len() - 1);
        assert_eq!(picker_target_at(sheet.selected), Some(id_act));
        assert_eq!(drawer_badge(5), "1");
        assert_eq!(drawer_badge(0), "S1");
    }
}
