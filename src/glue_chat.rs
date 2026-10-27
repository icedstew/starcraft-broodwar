//! The network lobby screen (rez\gluChat.bin, screen 3: 0x4b9e70, event proc 0x4b9c20, idle
//! proc 0x4b9b10; gluChat.cpp / gluChatSlot.cpp): the game's slots (0x4513f0 rows 30 .. 77,
//! four controls each: the row's text, the owner combo, the race combo and the download /
//! ping box), the chat list (11) and its entry (10, Send 9: Enter), the game's description
//! (0x4b9480), Ok (7, the creator's start) and Cancel (8). The protocol under it is
//! glue_lobby.rs.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state.

use super::{screen, state, Glue, Open};
use crate::dlg::flag;
use crate::setup::slot;
use crate::storage::Storage;
use crate::todo_port;

/// The controls of gluChat.bin.
mod ids {
    pub const MAP_PREVIEW: i16 = 6;
    pub const OK: i16 = 7;
    pub const CANCEL: i16 = 8;
    pub const SEND: i16 = 9;
    pub const ENTRY: i16 = 10;
    pub const LIST: i16 = 11;
    /// The description: the password heading (12) and text (16), the name (17), the game
    /// type (18), its subtype heading (14) and text (19), the map (20), its size (21) and the
    /// speed (22).
    pub const PASSWORD_LABEL: i16 = 12;
    pub const SUB_LABEL: i16 = 14;
    pub const PASSWORD: i16 = 16;
    pub const NAME: i16 = 17;
    pub const TYPE: i16 = 18;
    pub const SUB: i16 = 19;
    pub const MAP: i16 = 20;
    pub const SIZE: i16 = 21;
    pub const SPEED: i16 = 22;
    /// The start countdown's heading and seconds (0x4b9300).
    pub const COUNT_LABEL: i16 = 23;
    pub const COUNT: i16 = 24;
    /// The first slot row's first control; a row is 4 controls.
    pub const ROW: i16 = 30;
}

/// One row (0x68ecb0, 0xb0 bytes): what it shows (+0: 2 a force heading, 4 / 5 another's /
/// the local human, 6 a computer, 8 open, 9 closed; 0 empty) and its slot (+0x50).
#[derive(Clone, Copy, Default)]
pub(super) struct Row {
    kind: u8,
    slot: u8,
}

/// The lobby screen's globals.
#[derive(Default)]
pub(super) struct ChatScreen {
    rows: [Row; 12],
    /// The dialog's idle proc (+0x4a = 0x4b9b10) is set.
    idle: bool,
    /// The countdown's seconds shown last (0x4b9300).
    shown_count: Option<u32>,
}

/// The race combo's rows: (race, network.tbl string, 1-based) as 0x51aaa0 pairs them.
const RACES: [(u8, usize); 4] = [(0, 7), (1, 8), (2, 9), (6, 10)];
/// The owner combo's row kinds (0x5154e8 +0): 0 the player, 2 Open (gluAll 0x85), 3 Closed
/// (0x86), 4 Computer (0x84).
const OWNERS: [(u32, usize); 3] = [(2, 0x85), (3, 0x86), (4, 0x84)];

impl Glue {
    /// 0x4b9e70: rez\gluChat.bin with the event proc 0x4b9c20 (event 10 0x4b9810 and the slot
    /// rows 0x4513f0, the map preview 0x4b9ac0; event 0 0x4b9770: the creator's Ok, the
    /// description, the slide-in, the lobby start 0x4b9bf0).
    pub(super) fn open_chat(&mut self, files: &Storage) {
        self.lobby.dirty = false;
        self.lobby.error = None;
        self.chat = Default::default();
        let dlg = self.open_glue_dialog(files, "rez/gluChat.bin");
        self.dlg = Some((dlg, Open::Chat));
        self.chat_rows_init(files);
        if self.save.is_none() && self.melee() {
            todo_port!("0x4a5d10", "the lobby's minimap preview (rez\\minimappreview.bin)");
        }
        self.chat_show(ids::MAP_PREVIEW, false);
        // 0x4b9770: a joiner has no Ok (5 its picture, 7).
        if !self.lobby_host {
            self.chat_show(5, false);
            self.chat_show(ids::OK, false);
        }
        self.chat_show(ids::COUNT_LABEL, false);
        self.chat_show(ids::COUNT, false);
        self.chat_4b9480();
        self.show_palette();
        self.slide_in(0x51a490, 5, 0);
        // 0x4b9bf0: the lobby starts (0x4d4130); failed, the dialog ends with 0x22d.
        if !self.lobby_4d4130(files) {
            self.chat_4b9830(files, None, 0x22d);
            return;
        }
        self.chat.idle = true;
    }

    /// 0x4513f0 (event 10): the rows' controls: the text (30 + 4k, 0xb8 wide), the owner combo
    /// (+1, a combo 0xa4 x 0x19), the race combo (+2, 100 x 0x19), all hidden until a row is
    /// shown (0x452250).
    fn chat_rows_init(&mut self, files: &Storage) {
        let p = self.palette_id();
        self.look(files, p);
        let skin = &self.looks[p].as_ref().unwrap().skin;
        let Some((d, _)) = self.dlg.as_mut() else { return };
        let bottom = d.this.rect.3;
        for r in 0..12 {
            let base = ids::ROW + 4 * r;
            for (k, w) in [(1, 0xa4), (2, 100)] {
                if let Some(c) = d.get_mut(base + k) {
                    c.ty = 13;
                    c.rect.2 = c.rect.0 + w;
                    c.rect.3 = c.rect.1 + 0x19;
                    c.flags |= 0x10;
                    skin.list_init(c);
                    if let Some(l) = c.list.as_mut() {
                        l.parent_bottom = bottom;
                    }
                }
            }
            for k in 0..4 {
                if let Some(c) = d.get_mut(base + k) {
                    c.flags &= !flag::VISIBLE;
                }
            }
        }
    }

    fn chat_ctl(&mut self, id: i16) -> Option<&mut crate::dlg::Ctl> {
        match self.dlg.as_mut() {
            Some((d, Open::Chat)) => d.get_mut(id),
            _ => None,
        }
    }

    /// 0x4186a0 (`on`) / 0x418700 on control `id`.
    fn chat_show(&mut self, id: i16, on: bool) {
        if let Some(c) = self.chat_ctl(id) {
            if on {
                c.flags |= flag::VISIBLE;
            } else {
                c.flags &= !flag::VISIBLE;
            }
        }
    }

    fn chat_text(&mut self, id: i16, t: Vec<u8>) {
        if let Some(c) = self.chat_ctl(id) {
            c.text = t;
        }
    }

    /// 0x4b9480: the game's description: the name, the type and subtype (0x4aac90 / 0x4ab6c0),
    /// the map, its size ("%ux%u", gluAll 0x20) and the speed; no password heading.
    fn chat_4b9480(&mut self) {
        let d = self.create.game_desc.clone();
        if d.len() < 0x8d {
            return;
        }
        let size = format!("{}x{}", u16::from_le_bytes([d[0x20], d[0x21]]), u16::from_le_bytes([d[0x22], d[0x23]])).into_bytes();
        let (gt, b1, sub) = (d[0x28], d[0x29], u16::from_le_bytes([d[0x2a], d[0x2b]]));
        let (type_name, sub_label, sub_text) = self.join_type_texts(gt, b1, sub, d[0x25]);
        let speed = (d[0x26] < 7).then(|| self.glu(0x55 + d[0x26] as usize).into_bytes()).unwrap_or_default();
        self.chat_text(ids::NAME, cz(&d[4..0x1c]).to_vec());
        self.chat_text(ids::MAP, cz(&d[0x4d..0x6d]).to_vec());
        self.chat_text(ids::TYPE, type_name);
        self.chat_text(ids::SUB_LABEL, sub_label);
        self.chat_text(ids::SUB, sub_text);
        self.chat_text(ids::SIZE, size);
        self.chat_text(ids::SPEED, speed);
        self.chat_show(ids::PASSWORD_LABEL, false);
        self.chat_show(ids::PASSWORD, false);
    }

    /// 0x4b9830 (`err` a gluAll string, `result`): the dialog ends with `result` (0x22c the
    /// game starts, 0x22d back), an error shown first.
    fn chat_4b9830(&mut self, files: &Storage, err: Option<usize>, result: i32) {
        self.chat.idle = false;
        if let Some(e) = err {
            let msg = self.glu(e);
            self.open_popup(files, &msg);
        }
        self.end_dialog(result);
    }

    /// 0x4b9b10 (the idle proc, every pass): the lobby's poll (0x4d4340): 0x4b refreshes the
    /// slots (0x452250); 0x53 the game starts (0x22c); a failed lobby leaves (0x4d3860) with
    /// its message (0x22d); else the slots redraw when they changed (0x5999d4).
    pub(super) fn chat_tick(&mut self, files: &Storage) {
        if !self.chat.idle || !matches!(self.dlg, Some((_, Open::Chat))) || self.popup.is_some() {
            return;
        }
        let code = self.lobby_4d4340(files);
        self.chat_lines();
        if code == 0x53 {
            self.chat_4b9830(files, None, 0x22c);
            return;
        }
        if let Some(e) = self.lobby.error.take() {
            self.lobby_4d3860();
            self.chat_4b9830(files, Some(e), 0x22d);
            return;
        }
        if code == 0x4b || self.lobby.dirty {
            self.lobby.dirty = false;
            self.chat_452250();
        }
        self.chat_countdown();
    }

    /// 0x4b8f10: the lines waiting go to the chat list (64 rows kept; color 2 own, 6 a
    /// player, 4 the rest; flag 0x10), scrolled to the last.
    fn chat_lines(&mut self) {
        let lines = std::mem::take(&mut self.lobby.chat_out);
        let Some(c) = self.chat_ctl(ids::LIST) else { return };
        for (t, kind) in lines {
            while c.list.as_ref().is_some_and(|l| l.items.len() > 0x3f) {
                c.list_remove(0);
            }
            let color = match kind {
                0 => 2,
                1 => 6,
                _ => 4,
            };
            let row = c.list_add(&t, color | 0x10);
            if row != 0xff {
                c.list_scroll_to(row);
            }
        }
    }

    /// 0x4b9300 (each countdown turn): "Starting in: %d seconds" (gluAll 0xca / 0x82).
    fn chat_countdown(&mut self) {
        if self.setup.g66fbfa < 5 {
            return;
        }
        // 4 turns a second (one per 250 ms).
        let secs = self.lobby.countdown.div_ceil(4);
        if self.chat.shown_count == Some(secs) {
            return;
        }
        self.chat.shown_count = Some(secs);
        let unit = self.glu(if secs == 1 { 0xca } else { 0x82 });
        self.chat_show(ids::COUNT_LABEL, true);
        self.chat_show(ids::COUNT, true);
        self.chat_text(ids::COUNT, format!("{secs} {unit}").into_bytes());
        if self.lobby.locked {
            self.chat_show(ids::CANCEL, false);
            self.chat_show(ids::OK, false);
        }
    }

    /// 0x452250: the rows from the slot table: grouped by force (0x451fb0: Use Map Settings
    /// and team games, a heading per force) or the slots in order (0x451d40, melee).
    fn chat_452250(&mut self) {
        let grouped = !self.melee() || self.team_game();
        let mut rows: Vec<Row> = Vec::new();
        let kind = |s: &crate::setup::Slot, local: bool| match s[slot::TYPE] {
            2 => Some(if local { 5 } else { 4 }),
            5 | 1 => Some(6),
            6 => Some(8),
            8 => Some(9),
            _ => None,
        };
        let me = self.setup.local_slot[0] as usize;
        if grouped {
            for f in 1..=4u8 {
                let slots: Vec<usize> = (0..8).filter(|&k| self.setup.slots[k][slot::FORCE] == f && kind(&self.setup.slots[k], k == me).is_some()).collect();
                if slots.is_empty() {
                    continue;
                }
                rows.push(Row { kind: 2, slot: f });
                for k in slots {
                    rows.push(Row { kind: kind(&self.setup.slots[k], k == me).unwrap(), slot: k as u8 });
                }
            }
        } else {
            for k in 0..8 {
                if let Some(kd) = kind(&self.setup.slots[k], k == me) {
                    rows.push(Row { kind: kd, slot: k as u8 });
                }
            }
        }
        rows.truncate(12);
        rows.resize(12, Row::default());
        let old = self.chat.rows;
        self.chat.rows.copy_from_slice(&rows);
        for (r, row) in rows.iter().enumerate() {
            let changed = old[r].kind != row.kind || old[r].slot != row.slot;
            self.chat_row(r as i16, *row, changed);
        }
    }

    /// One row's controls (0x451aa0 a human, 0x451940 a computer, 0x451760 open / closed,
    /// 0x450650 a force heading): the creator gets the owner combo of every slot but its own;
    /// a race combo is enabled for one's own slot and (the creator) the computers'; the last
    /// box shows the map download.
    fn chat_row(&mut self, r: i16, row: Row, changed: bool) {
        let base = ids::ROW + 4 * r;
        if row.kind == 0 {
            for k in 0..4 {
                self.chat_show(base + k, false);
            }
            return;
        }
        if row.kind == 2 {
            let name = cz(&self.setup.force_names[(row.slot as usize).saturating_sub(1) & 3]).to_vec();
            self.chat_text(base, name);
            self.chat_show(base, true);
            for k in 1..4 {
                self.chat_show(base + k, false);
            }
            return;
        }
        let k = row.slot as usize;
        let s = self.setup.slots[k];
        let name = match row.kind {
            4 | 5 => cz(&s[slot::NAME..slot::NAME + 0x19]).to_vec(),
            6 => self.glu(0x84).into_bytes(),
            8 => self.glu(0x85).into_bytes(),
            _ => self.glu(0x86).into_bytes(),
        };
        let owner_combo = self.lobby_host && row.kind != 5 && self.setup.g66fbfa == 4 && !self.lobby.locked;
        if changed || self.chat_ctl(base + 1).is_some_and(|c| c.list.as_ref().is_none_or(|l| l.items.is_empty())) {
            // 0x450d90: the owner combo's rows (the player first for a human).
            let mut items: Vec<(Vec<u8>, u32)> = Vec::new();
            if matches!(row.kind, 4 | 5) {
                items.push((name.clone(), 0));
            }
            for (v, sid) in OWNERS {
                items.push((self.glu(sid).into_bytes(), v));
            }
            let want = match row.kind {
                6 => 4,
                8 => 2,
                9 => 3,
                _ => 0,
            };
            self.chat_fill(base + 1, &items, want);
            let races: Vec<(Vec<u8>, u32)> = RACES.iter().map(|&(rc, n)| (crate::setup::tbl_str(&self.network_raw, n - 1).to_vec(), rc as u32)).collect();
            self.chat_fill(base + 2, &races, s[slot::RACE] as u32);
        } else {
            self.chat_select(base + 2, s[slot::RACE] as u32);
        }
        self.chat_text(base, name);
        self.chat_show(base, !owner_combo);
        self.chat_show(base + 1, owner_combo);
        let race_on = matches!(row.kind, 4 | 5 | 6);
        let race_edit = !self.lobby.locked && self.setup.g66fbfa == 4 && (row.kind == 5 || (self.lobby_host && row.kind == 6)) && (self.melee() || self.setup.user_selectable[k] != 0);
        self.chat_show(base + 2, race_on);
        if let Some(c) = self.chat_ctl(base + 2) {
            if race_edit {
                c.flags &= !flag::DISABLED;
            } else {
                c.flags |= flag::DISABLED;
            }
        }
        // 0x44fac0: the download (or the ping bars, Battle.net).
        let id = u32::from_le_bytes(s[slot::STORM..slot::STORM + 4].try_into().unwrap());
        let pct = if matches!(row.kind, 4 | 5) && id < 8 { self.lobby.dl_pct[id as usize] } else { 100 };
        let dl = matches!(row.kind, 4 | 5) && pct < 100;
        self.chat_text(base + 3, if dl { format!("{pct}%").into_bytes() } else { Vec::new() });
        self.chat_show(base + 3, dl);
    }

    /// A combo's rows (0x41b550 / 0x41b760) with `want`'s row selected.
    fn chat_fill(&mut self, id: i16, items: &[(Vec<u8>, u32)], want: u32) {
        let Some(c) = self.chat_ctl(id) else { return };
        c.list_clear();
        for (t, v) in items {
            let row = c.list_add(t, 0);
            if let Some(i) = c.list.as_mut().and_then(|l| l.items.get_mut(row as usize)) {
                i.2 = *v;
            }
        }
        self.chat_select(id, want);
    }

    fn chat_select(&mut self, id: i16, want: u32) {
        let Some(c) = self.chat_ctl(id) else { return };
        let row = c.list.as_ref().and_then(|l| l.items.iter().position(|i| i.2 == want));
        if let Some(r) = row {
            c.list_select(r as u8);
        }
    }

    /// A combo row picked (event 0xe code 0xb): the owner combo (0x450c50: Open 0x4526d0,
    /// Closed 0x452680, Computer 0x452720: a human there is booted first) or the race combo
    /// (0x450a60 -> 0x452370).
    pub(super) fn chat_combo(&mut self, id: i16, row: u8) {
        if !(ids::ROW..ids::ROW + 48).contains(&id) {
            return;
        }
        let r = ((id - ids::ROW) / 4) as usize;
        let which = (id - ids::ROW) % 4;
        let Row { kind, slot: k } = self.chat.rows[r];
        let v = self.chat_ctl(id).and_then(|c| c.list.as_ref()?.items.get(row as usize).map(|i| i.2));
        let Some(v) = v else { return };
        match which {
            1 => {
                let what = match v {
                    2 => 1,
                    3 => 2,
                    4 => 0,
                    _ => return,
                };
                if kind == 4 {
                    // 0x452720: the human in the slot is booted (0x470480).
                    let id = u32::from_le_bytes(self.setup.slots[k as usize][slot::STORM..slot::STORM + 4].try_into().unwrap());
                    self.lobby_boot(id);
                }
                self.lobby_470df0(k, what);
            }
            2 => self.lobby_452370(k, v as u8),
            _ => {}
        }
    }

    /// 0x4b9910 (event 0xe code 2): Send (9) the entry's text (0x4b98d0); Ok (7, the
    /// creator) starts: an opponent needed (0x44f7b0 >= 2, gluAll 0x72; a team game two
    /// teams, 0xba), the map handed out (0x472a70, 0x73), then 0x452460; Cancel (8) leaves
    /// (0x4d3860) and ends the dialog.
    pub(super) fn chat_activate(&mut self, files: &Storage, id: i16) {
        match id {
            ids::SEND => {
                // 0x4b98d0: the entry empties (+0x3e the caret).
                let t = self.chat_ctl(ids::ENTRY).map(|c| {
                    if let Some(e) = c.edit.as_mut() {
                        e.caret = 0;
                        e.first = 0;
                    }
                    std::mem::take(&mut c.text)
                });
                if let Some(t) = t.filter(|t| !t.is_empty()) {
                    self.lobby_4707d0(&t);
                }
            }
            ids::OK => {
                if self.save.is_none() {
                    if self.chat_44f7b0() < 2 {
                        let m = self.glu(0x72);
                        self.open_popup(files, &m);
                        return;
                    }
                    if self.team_game() && self.chat_teams() < 2 {
                        let m = self.glu(0xba);
                        self.open_popup(files, &m);
                        return;
                    }
                }
                if self.lobby.transfer.as_ref().is_some_and(|t| !t.done()) {
                    let m = self.glu(0x73);
                    self.open_popup(files, &m);
                    return;
                }
                self.lobby_452460();
                self.chat_show(ids::OK, false);
            }
            ids::CANCEL => {
                self.lobby_4d3860();
                self.chat_4b9830(files, None, 8);
            }
            _ => {}
        }
    }

    /// 0x44f7b0: the players (humans and computers; in a team game one per force); 0 without
    /// a human.
    fn chat_44f7b0(&self) -> usize {
        let grouped = self.team_game() || self.setup.template[0] == 0xf;
        let (mut humans, mut others, mut top) = (0, 0, 0u8);
        for s in &self.setup.slots[..8] {
            let f = s[slot::FORCE];
            let count = |n: &mut usize, top: &mut u8| {
                if !grouped {
                    *n += 1;
                } else if *top < f {
                    *n += 1;
                    *top = f;
                }
            };
            match s[slot::TYPE] {
                2 => count(&mut humans, &mut top),
                1 | 5 => count(&mut others, &mut top),
                _ => {}
            }
        }
        if humans == 0 { 0 } else { humans + others }
    }

    /// 0x44f4e0 per force: the forces with a human.
    fn chat_teams(&self) -> usize {
        (1..=4u8).filter(|&f| self.setup.slots[..8].iter().any(|s| s[slot::FORCE] == f && s[slot::TYPE] == 2)).count()
    }

    /// 0x452720's boot: 'N' and the drop (0x470480).
    fn lobby_boot(&mut self, id: u32) {
        if id < 8 && id != self.setup.local_storm_id {
            for _ in 0..3 {
                crate::net::snet::send_message(id as u8, b"N\0");
            }
            crate::net::snet::drop_player(id, 0x4000_0004);
        }
    }

    /// After 0x41a080 in 0x4b9e70: 0x22c the game (state 1, the lobby chose: melee) or the
    /// ready room the lobby set; 8 / 0x22d back to the provider's game list (Battle.net 4,
    /// modem 0x14, else the join screen 10); anything else the main menu.
    pub(super) fn done_chat(&mut self, result: i32) {
        self.chat.idle = false;
        match result {
            0x22c => {
                if self.lobby.start_game {
                    self.lobby.start_game = false;
                    self.state = state::START;
                }
            }
            8 | 0x22d => {
                self.screen = match self.setup.net_provider {
                    0x424e_4554 => screen::BATTLE_NET,
                    0x4d44_4d58 | 0x4d4f_444d => screen::MODEM,
                    _ => screen::GAMESEL,
                };
                self.setup.g596814 = 0;
            }
            _ => self.screen = screen::MAIN,
        }
    }
}

/// The text before the first NUL.
fn cz(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}
