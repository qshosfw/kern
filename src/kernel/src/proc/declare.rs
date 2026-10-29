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

// ── Table Identifiers for LAYOUT_TABLE_BROWSER ─────────────────────────────
pub const TABLE_ID_NONE: u16 = 0;
pub const TABLE_ID_DIR: u16 = 1;
pub const TABLE_ID_CHANNELS: u16 = 2;
pub const TABLE_ID_CONTACTS: u16 = 3;
pub const TABLE_ID_PROCS: u16 = 4;
pub const TABLE_ID_HITS: u16 = 5;
pub const TABLE_ID_ZONES: u16 = 6;

// ── UI Item Kinds (Section 10.2) ────────────────────────────────────────────
pub const ITEM_ACTION: u8 = 0;
pub const ITEM_TOGGLE: u8 = 1;
pub const ITEM_VALUE: u8 = 2;
pub const ITEM_SUBMENU: u8 = 3;
pub const ITEM_LINK: u8 = 4;
pub const ITEM_INFO: u8 = 5;

// ── UI Item Flags (Section 10.2) ────────────────────────────────────────────
pub const FLAG_DYNAMIC: u8 = 1 << 0;
pub const FLAG_READONLY: u8 = 1 << 1;
pub const FLAG_SEPARATOR: u8 = 1 << 2;
pub const FLAG_DISABLED: u8 = 1 << 3;

// ── Telemetry & Dynamic Data Provider Identifiers (Section 10.4) ─────────────
pub const DYN_BATTERY_MV: u16 = 0x0100;
pub const DYN_BATTERY_PCT: u16 = 0x0101;
pub const DYN_CPU_CLOCK: u16 = 0x0102;
pub const DYN_RAM_FREE: u16 = 0x0103;
pub const DYN_RAM_USED: u16 = 0x0104;
pub const DYN_STACK_MARGIN: u16 = 0x0105;
pub const DYN_STACK_PEAK: u16 = 0x0106;
pub const DYN_UPTIME: u16 = 0x0107;
pub const DYN_ACTIVE_PROCS: u16 = 0x0108;
pub const DYN_STORAGE_FREE: u16 = 0x0109;
pub const DYN_RADIO_RSSI: u16 = 0x010A;
pub const DYN_RADIO_FREQ_A: u16 = 0x010B;
pub const DYN_RADIO_FREQ_B: u16 = 0x010C;
pub const DYN_SWAP_USAGE: u16 = 0x010D;

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

/// Bounded navigation history frame for exact cursor and scroll restoration on Exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavFrame {
    pub node_idx: usize,
    pub selected_item: usize,
    pub scroll_top: usize,
}

/// Dynamic system or service command issued by Declare action execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclareCommand {
    None,
    CallService {
        interface_id: u16,
        method_id: u16,
        param: u16,
    },
    EmitEvent {
        event_code: u16,
        payload: u16,
    },
    PlayTone {
        freq_hz: u16,
        duration_ms: u16,
    },
    Notify {
        message_offset: u16,
    },
    LaunchApp {
        app_id: u8,
    },
}

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
    pub scroll_top: usize,
    pub table_row_count: u16,
    pub nav_stack: heapless::Vec<NavFrame, 4>,
    pub pending_command: DeclareCommand,
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
            scroll_top: 0,
            table_row_count: 0,
            nav_stack: heapless::Vec::new(),
            pending_command: DeclareCommand::None,
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
        self.scroll_top = 0;
        self.table_row_count = 0;
        self.nav_stack.clear();
        self.pending_command = DeclareCommand::None;
    }

    #[inline(always)]
    pub fn take_command(&mut self) -> DeclareCommand {
        let cmd = self.pending_command;
        self.pending_command = DeclareCommand::None;
        cmd
    }

    #[inline(always)]
    pub fn adjust_scroll_for_selection(&mut self, visible_count: usize) {
        if self.selected_item < self.scroll_top {
            self.scroll_top = self.selected_item;
        } else if self.selected_item >= self.scroll_top + visible_count {
            self.scroll_top = self.selected_item + 1 - visible_count;
        }
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
                    let val = (act.b as i16) as i32;
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
                        let _ = self.nav_stack.push(NavFrame {
                            node_idx: self.current_node_idx,
                            selected_item: self.selected_item,
                            scroll_top: self.scroll_top,
                        });
                        self.current_node_idx = target_node;
                        self.selected_item = 0;
                        self.scroll_top = 0;
                    }
                }
                ACTION_CALL_SVC => {
                    self.pending_command = DeclareCommand::CallService {
                        interface_id: act.a,
                        method_id: act.b,
                        param: act.c,
                    };
                }
                ACTION_PLAY_ASSET => {
                    self.pending_command = DeclareCommand::PlayTone {
                        freq_hz: act.a,
                        duration_ms: act.b,
                    };
                }
                ACTION_EMIT_EVENT => {
                    self.pending_command = DeclareCommand::EmitEvent {
                        event_code: act.a,
                        payload: act.b,
                    };
                }
                ACTION_NOTIFY => {
                    self.pending_command = DeclareCommand::Notify {
                        message_offset: act.a,
                    };
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

        let item_count = if node.layout == LAYOUT_TABLE_BROWSER && node.item_count == 0 {
            self.table_row_count as usize
        } else {
            node.item_count as usize
        };

        match code {
            KeyCode::Up | KeyCode::Key2 => {
                if self.selected_item > 0 {
                    self.selected_item -= 1;
                } else if item_count > 0 {
                    self.selected_item = item_count - 1;
                }
                self.adjust_scroll_for_selection(5);
                true
            }
            KeyCode::Down | KeyCode::Key8 => {
                if item_count > 0 {
                    self.selected_item = (self.selected_item + 1) % item_count;
                }
                self.adjust_scroll_for_selection(5);
                true
            }
            KeyCode::Key4 => {
                let item_idx = (node.first_item_offset as usize) + self.selected_item;
                if let Some(item) = self.items.get(item_idx).copied() {
                    match item.kind {
                        ITEM_VALUE => {
                            self.state.add_i32(item.value_ref, -1);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, item.value_ref);
                            true
                        }
                        ITEM_TOGGLE => {
                            self.state.toggle_bool(item.value_ref);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, item.value_ref);
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            }
            KeyCode::Key6 => {
                let item_idx = (node.first_item_offset as usize) + self.selected_item;
                if let Some(item) = self.items.get(item_idx).copied() {
                    match item.kind {
                        ITEM_VALUE => {
                            self.state.add_i32(item.value_ref, 1);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, item.value_ref);
                            true
                        }
                        ITEM_TOGGLE => {
                            self.state.toggle_bool(item.value_ref);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, item.value_ref);
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            }
            KeyCode::Menu => {
                if node.layout == LAYOUT_TABLE_BROWSER
                    && (node.first_item_offset as usize) >= self.items.len()
                {
                    if let Some(act) = self.actions.first() {
                        let op = act.op;
                        self.execute_action(0);
                        self.trigger_rules(TRIGGER_KEY, op as u16);
                        return true;
                    }
                }
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
                        ITEM_VALUE => {
                            let key = item.value_ref;
                            self.state.add_i32(key, 1);
                            self.trigger_rules(TRIGGER_STATE_CHANGE, key);
                        }
                        ITEM_SUBMENU => {
                            let target_node = item.target as usize;
                            if target_node < self.nodes.len() {
                                let _ = self.nav_stack.push(NavFrame {
                                    node_idx: self.current_node_idx,
                                    selected_item: self.selected_item,
                                    scroll_top: self.scroll_top,
                                });
                                self.current_node_idx = target_node;
                                self.selected_item = 0;
                                self.scroll_top = 0;
                            }
                        }
                        _ => {}
                    }
                }
                true
            }
            KeyCode::Exit => {
                if let Some(prev) = self.nav_stack.pop() {
                    self.current_node_idx = prev.node_idx;
                    self.selected_item = prev.selected_item;
                    self.scroll_top = prev.scroll_top;
                    true
                } else if self.current_node_idx > 0 {
                    self.current_node_idx = 0;
                    self.selected_item = 0;
                    self.scroll_top = 0;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_walker_navigation_and_scrolling() {
        let mut walker = DeclareWalker::new("TestApp");
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_MENU,
            item_count: 8,
            title_offset: 0,
            first_item_offset: 0,
        });
        for i in 0..8 {
            let _ = walker.items.push(DeclareItem {
                kind: ITEM_ACTION,
                flags: 0,
                label_offset: i as u16,
                target: 0,
                value_ref: 0,
                aux: 0,
                reserved: 0,
            });
        }

        assert_eq!(walker.selected_item, 0);
        assert_eq!(walker.scroll_top, 0);

        // Move down past visible 5 rows
        for _ in 0..5 {
            walker.handle_key(KeyCode::Down);
        }
        assert_eq!(walker.selected_item, 5);
        assert_eq!(walker.scroll_top, 1); // Scrolled to keep row 5 visible

        // Wrap around to top
        for _ in 0..3 {
            walker.handle_key(KeyCode::Down);
        }
        assert_eq!(walker.selected_item, 0);
        assert_eq!(walker.scroll_top, 0);

        // Wrap around upwards
        walker.handle_key(KeyCode::Up);
        assert_eq!(walker.selected_item, 7);
        assert_eq!(walker.scroll_top, 3);
    }

    #[test]
    fn test_walker_interactive_form_values_and_toggles() {
        let mut walker = DeclareWalker::new("FormApp");
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_FORM,
            item_count: 2,
            title_offset: 0,
            first_item_offset: 0,
        });
        // Item 0: ITEM_VALUE on key 10
        let _ = walker.items.push(DeclareItem {
            kind: ITEM_VALUE,
            flags: 0,
            label_offset: 0,
            target: 0,
            value_ref: 10,
            aux: 0,
            reserved: 0,
        });
        // Item 1: ITEM_TOGGLE on key 20
        let _ = walker.items.push(DeclareItem {
            kind: ITEM_TOGGLE,
            flags: 0,
            label_offset: 1,
            target: 0,
            value_ref: 20,
            aux: 0,
            reserved: 0,
        });

        // Test adjusting integer value with Key6 (Right / +1) and Key4 (Left / -1)
        assert_eq!(walker.state.get_i32(10), 0);
        walker.handle_key(KeyCode::Key6);
        assert_eq!(walker.state.get_i32(10), 1);
        walker.handle_key(KeyCode::Key6);
        assert_eq!(walker.state.get_i32(10), 2);
        walker.handle_key(KeyCode::Key4);
        assert_eq!(walker.state.get_i32(10), 1);

        // Move to toggle item and flip state with Menu key
        walker.handle_key(KeyCode::Down);
        assert_eq!(walker.selected_item, 1);
        assert!(!walker.state.get_bool(20));
        walker.handle_key(KeyCode::Menu);
        assert!(walker.state.get_bool(20));
        walker.handle_key(KeyCode::Menu);
        assert!(!walker.state.get_bool(20));
    }

    #[test]
    fn test_walker_submenu_and_nav_stack() {
        let mut walker = DeclareWalker::new("NavApp");
        // Root Node (0)
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_MENU,
            item_count: 1,
            title_offset: 0,
            first_item_offset: 0,
        });
        // Submenu Item pointing to Node 1
        let _ = walker.items.push(DeclareItem {
            kind: ITEM_SUBMENU,
            flags: 0,
            label_offset: 0,
            target: 1,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        });
        // Sub Node (1)
        let _ = walker.nodes.push(DeclareNode {
            node_id: 1,
            layout: LAYOUT_MENU,
            item_count: 1,
            title_offset: 1,
            first_item_offset: 1,
        });
        let _ = walker.items.push(DeclareItem {
            kind: ITEM_ACTION,
            flags: 0,
            label_offset: 2,
            target: 0,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        });

        // Enter submenu
        assert_eq!(walker.current_node_idx, 0);
        walker.handle_key(KeyCode::Menu);
        assert_eq!(walker.current_node_idx, 1);
        assert_eq!(walker.nav_stack.len(), 1);

        // Exit pops nav_stack and returns to Root Node
        assert!(walker.handle_key(KeyCode::Exit));
        assert_eq!(walker.current_node_idx, 0);
        assert!(walker.nav_stack.is_empty());

        // Context preservation verification:
        // Set root cursor and scroll position, enter submenu, and ensure Exit restores exact focus
        walker.selected_item = 5;
        walker.scroll_top = 3;
        let _ = walker.nav_stack.push(NavFrame {
            node_idx: walker.current_node_idx,
            selected_item: walker.selected_item,
            scroll_top: walker.scroll_top,
        });
        walker.current_node_idx = 1;
        walker.selected_item = 0;
        walker.scroll_top = 0;

        assert_eq!(walker.current_node_idx, 1);
        assert_eq!(walker.selected_item, 0);
        assert_eq!(walker.scroll_top, 0);

        assert!(walker.handle_key(KeyCode::Exit));
        assert_eq!(walker.current_node_idx, 0);
        assert_eq!(walker.selected_item, 5);
        assert_eq!(walker.scroll_top, 3);

        // Exit at Root Node returns false so app can exit to Home
        assert!(!walker.handle_key(KeyCode::Exit));
    }

    #[test]
    fn test_walker_action_commands_and_table_browser() {
        let mut walker = DeclareWalker::new("TableApp");
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_TABLE_BROWSER,
            item_count: 0, // Dynamic table
            title_offset: 0,
            first_item_offset: TABLE_ID_DIR,
        });
        let _ = walker.actions.push(ActionRecord {
            op: ACTION_CALL_SVC,
            flags: 0,
            a: 0x0004, // Storage
            b: 1,      // Read
            c: 0,
        });
        walker.table_row_count = 10;

        // Navigate down dynamic rows
        assert_eq!(walker.selected_item, 0);
        walker.handle_key(KeyCode::Down);
        assert_eq!(walker.selected_item, 1);

        // Press Menu to trigger table action
        walker.handle_key(KeyCode::Menu);
        assert_eq!(
            walker.take_command(),
            DeclareCommand::CallService {
                interface_id: 0x0004,
                method_id: 1,
                param: 0,
            }
        );
        // Command consumed
        assert_eq!(walker.take_command(), DeclareCommand::None);
    }
}
