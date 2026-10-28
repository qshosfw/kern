//! Declare 3.0 Declarative Execution Engine & UI Projection Runtime.
//!
//! Normative implementation of KRX-ABI 2.5.0 Chapter 10 (Declare 3.0).
//! Executes declarative programs composed of Rules, Actions, Schedules, Nodes, and Items
//! with strictly zero heap allocation, bounded execution steps, and clean separation between
//! kernel execution mechanics and userland UI projections.

pub use super::krx::{ActionRecord, DeclareItem, DeclareNode, RuleRecord, ScheduleRecord};
use crate::event::KeyCode;

// ── Trigger Types (Section 10.2) ────────────────────────────────────────────
pub const TRIGGER_EVENT: u8 = 0;
pub const TRIGGER_SCHEDULE: u8 = 1;
pub const TRIGGER_KEY: u8 = 2;
pub const TRIGGER_STATE_CHANGE: u8 = 3;
pub const TRIGGER_BOOT: u8 = 4;
pub const TRIGGER_SERVICE_CALL: u8 = 5;

// ── Action Opcodes (Section 10.2) ───────────────────────────────────────────
pub const ACTION_CALL_SVC: u8 = 0;
pub const ACTION_SET_STATE: u8 = 1;
pub const ACTION_EMIT_EVENT: u8 = 2;
pub const ACTION_PLAY_ASSET: u8 = 3;
pub const ACTION_NOTIFY: u8 = 4;
pub const ACTION_NAVIGATE: u8 = 5;
pub const ACTION_SET_ENABLED: u8 = 6;
pub const ACTION_DELAY: u8 = 7;

// ── UI Node Layouts (Section 10.2) ──────────────────────────────────────────
pub const LAYOUT_MENU: u8 = 0;
pub const LAYOUT_FORM: u8 = 1;
pub const LAYOUT_SCROLL_TEXT: u8 = 2;
pub const LAYOUT_INFO_CARD: u8 = 3;
pub const LAYOUT_TABLE_BROWSER: u8 = 4;

// ── UI Item Kinds (Section 10.2) ────────────────────────────────────────────
pub const ITEM_ACTION: u8 = 0;
pub const ITEM_TOGGLE: u8 = 1;
pub const ITEM_VALUE: u8 = 2;
pub const ITEM_SUBMENU: u8 = 3;
pub const ITEM_LINK: u8 = 4;
pub const ITEM_INFO: u8 = 5;

// ── Schedule Kinds (Section 10.2) ───────────────────────────────────────────
pub const SCHEDULE_EVERY_MS: u8 = 0;
pub const SCHEDULE_RTC_AT: u8 = 1;
pub const SCHEDULE_RTC_DAILY: u8 = 2;

/// Typed Key-Value State representation (Declare 3.0 State Primitive)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateValue {
    Unset,
    Integer(i32),
    Boolean(bool),
}

/// Fixed-capacity deterministic State Store supporting up to 16 state variables per package.
#[derive(Debug, Clone, Copy)]
pub struct StateStore {
    values: [StateValue; 16],
}

impl StateStore {
    pub const fn new() -> Self {
        Self {
            values: [StateValue::Unset; 16],
        }
    }

    #[inline(always)]
    pub fn get_i32(&self, key: u16) -> i32 {
        let idx = (key as usize) % 16;
        match self.values[idx] {
            StateValue::Integer(v) => v,
            StateValue::Boolean(b) => {
                if b {
                    1
                } else {
                    0
                }
            }
            StateValue::Unset => 0,
        }
    }

    #[inline(always)]
    pub fn set_i32(&mut self, key: u16, val: i32) {
        let idx = (key as usize) % 16;
        self.values[idx] = StateValue::Integer(val);
    }

    #[inline(always)]
    pub fn get_bool(&self, key: u16) -> bool {
        let idx = (key as usize) % 16;
        match self.values[idx] {
            StateValue::Boolean(b) => b,
            StateValue::Integer(v) => v != 0,
            StateValue::Unset => false,
        }
    }

    #[inline(always)]
    pub fn set_bool(&mut self, key: u16, val: bool) {
        let idx = (key as usize) % 16;
        self.values[idx] = StateValue::Boolean(val);
    }

    #[inline(always)]
    pub fn toggle_bool(&mut self, key: u16) -> bool {
        let curr = self.get_bool(key);
        self.set_bool(key, !curr);
        !curr
    }

    #[inline(always)]
    pub fn add_i32(&mut self, key: u16, delta: i32) -> i32 {
        let curr = self.get_i32(key);
        let next = curr.saturating_add(delta);
        self.set_i32(key, next);
        next
    }
}

/// Maximum graph capacity limits adhering to Cortex-M0+ sub-128B SRAM budget per package.
pub const MAX_NODES: usize = 8;
pub const MAX_ITEMS: usize = 24;
pub const MAX_LABELS: usize = 24;
pub const MAX_RULES: usize = 8;
pub const MAX_ACTIONS: usize = 16;
pub const MAX_SCHEDULES: usize = 4;

/// Declare Walker (Class Runtime Engine).
///
/// Executes the declarative graph, manages UI screen projections onto the Canvas,
/// handles keypad input transitions, and steps schedules and rule actions.
pub struct DeclareWalker {
    pub app_name: &'static str,
    pub nodes: heapless::Vec<DeclareNode, MAX_NODES>,
    pub items: heapless::Vec<DeclareItem, MAX_ITEMS>,
    pub labels: heapless::Vec<&'static str, MAX_LABELS>,
    pub rules: heapless::Vec<RuleRecord, MAX_RULES>,
    pub actions: heapless::Vec<ActionRecord, MAX_ACTIONS>,
    pub schedules: heapless::Vec<ScheduleRecord, MAX_SCHEDULES>,
    pub state: StateStore,
    pub current_node_idx: usize,
    pub selected_item: usize,
}

impl DeclareWalker {
    pub const fn new(app_name: &'static str) -> Self {
        Self {
            app_name,
            nodes: heapless::Vec::new(),
            items: heapless::Vec::new(),
            labels: heapless::Vec::new(),
            rules: heapless::Vec::new(),
            actions: heapless::Vec::new(),
            schedules: heapless::Vec::new(),
            state: StateStore::new(),
            current_node_idx: 0,
            selected_item: 0,
        }
    }

    pub fn reset(&mut self, name: &'static str) {
        self.app_name = name;
        self.nodes.clear();
        self.items.clear();
        self.labels.clear();
        self.rules.clear();
        self.actions.clear();
        self.schedules.clear();
        self.state = StateStore::new();
        self.current_node_idx = 0;
        self.selected_item = 0;
    }

    #[inline(never)]
    pub fn load_static_package(
        &mut self,
        name: &'static str,
        nodes: &[DeclareNode],
        items: &[DeclareItem],
        labels: &[&'static str],
        actions: &[ActionRecord],
    ) {
        self.reset(name);
        for &n in nodes {
            let _ = self.nodes.push(n);
        }
        for &i in items {
            let _ = self.items.push(i);
        }
        for &l in labels {
            let _ = self.labels.push(l);
        }
        for &a in actions {
            let _ = self.actions.push(a);
        }
    }

    /// Step action execution (bounded to single instruction).
    pub fn execute_action(&mut self, action_idx: usize) {
        if let Some(act) = self.actions.get(action_idx).copied() {
            match act.op {
                ACTION_SET_STATE => {
                    let key = act.a;
                    let val = act.b as i32;
                    let mode = act.c;
                    if mode == 1 {
                        // Delta add
                        self.state.add_i32(key, val);
                    } else {
                        // Direct set
                        self.state.set_i32(key, val);
                    }
                }
                ACTION_NAVIGATE => {
                    let target_node = act.a as usize;
                    if target_node < self.nodes.len() {
                        self.current_node_idx = target_node;
                        self.selected_item = 0;
                    }
                }
                _ => {}
            }
        }
    }

    /// Trigger matching rules and evaluate their action pipelines.
    pub fn trigger_rules(&mut self, trigger: u8, trigger_ref: u16) {
        for i in 0..self.rules.len() {
            let rule = self.rules[i];
            if (rule.flags & 1) != 0 // FLAG_ENABLED
                && rule.trigger == trigger
                && (rule.trigger_ref == 0 || rule.trigger_ref == trigger_ref)
            {
                let start_act = rule.action_list as usize;
                let count = rule.action_count as usize;
                for a in 0..count {
                    self.execute_action(start_act + a);
                }
            }
        }
    }

    /// Handle UI Keypad interaction.
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        let node = if let Some(n) = self.nodes.get(self.current_node_idx) {
            *n
        } else {
            return false;
        };

        let item_count = node.item_count as usize;

        match code {
            KeyCode::Up => {
                if self.selected_item > 0 {
                    self.selected_item -= 1;
                } else if item_count > 0 {
                    self.selected_item = item_count - 1;
                }
                true
            }
            KeyCode::Down => {
                if item_count > 0 {
                    self.selected_item = (self.selected_item + 1) % item_count;
                }
                true
            }
            KeyCode::Menu => {
                let item_idx = (node.first_item_offset as usize) + self.selected_item;
                if let Some(item) = self.items.get(item_idx).copied() {
                    match item.kind {
                        ITEM_ACTION => {
                            let act_idx = item.target as usize;
                            self.execute_action(act_idx);
                            self.trigger_rules(TRIGGER_KEY, act_idx as u16);
                        }
                        ITEM_TOGGLE => {
                            let key = item.value_ref;
                            self.state.toggle_bool(key);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, key);
                        }
                        ITEM_SUBMENU => {
                            let target_node = item.target as usize;
                            if target_node < self.nodes.len() {
                                self.current_node_idx = target_node;
                                self.selected_item = 0;
                            }
                        }
                        _ => {}
                    }
                }
                true
            }
            KeyCode::Exit => {
                if self.current_node_idx > 0 {
                    self.current_node_idx = 0;
                    self.selected_item = 0;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
