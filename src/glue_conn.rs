//! The connection select screen (rez\gluConn.bin, screen 2: 0x4baf30, event proc 0x4bae20):
//! the network providers Storm offers (net/snet.rs: only 'UDPN', over our transport), the
//! selected one's name, most players and description. Ok initializes the provider
//! (0x4d3cc0) and goes on to the profile login (screen 5); Cancel back to the main menu.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. The Battle.net
//! parts (the gateway panel, controls 11 .. 14, its list proc 0x4ba740) are not ported; the
//! panel stays hidden.

use super::{screen, Glue, Open};
use crate::net::snet;
use crate::storage::Storage;
use crate::todo_port;

/// The controls of gluConn.bin.
mod ids {
    /// The provider list (proc 0x4bad10).
    pub const LIST: i16 = 5;
    /// The selected provider's name, "Maximum players: %d" (gluAll 0x68) and description.
    pub const NAME: i16 = 6;
    pub const PLAYERS: i16 = 7;
    pub const DESC: i16 = 8;
    pub const OK: i16 = 9;
    /// The Battle.net gateway panel (11), its title, list and hint (gluAll 0xb9, set by
    /// 0x4bab10).
    pub const GATEWAY_FIRST: i16 = 11;
    pub const GATEWAY_HINT: i16 = 13;
    pub const GATEWAY_LAST: i16 = 14;
}

/// 0x5129bc: providers whose description is a gluAll.tbl string instead of the provider's.
const DESC_STRINGS: [(u32, usize); 5] = [(0x424e_4554, 103), (0x4d4f_444d, 100), (0x4950_584e, 102), (snet::UDPN, 190), (0x5343_424c, 101)];

/// One row of the provider list 0x51a218 (0x118-byte nodes made by 0x4ba610).
struct Row {
    /// +0xc: the provider id.
    id: u32,
    /// +0x10: the most players (caps +0x10, at most 8).
    max_players: u32,
    /// +0x14: the name.
    name: Vec<u8>,
    /// +0x114: the description shown (the provider's +0x94, or a gluAll.tbl string).
    desc: Vec<u8>,
}

/// The screen's globals.
#[derive(Default)]
pub(super) struct Conn {
    /// 0x51a218: the providers, in Storm's order.
    rows: Vec<Row>,
    /// 0x6d5a28 / 0x6d5a2c: the selected provider and the one before it.
    sel: u32,
    prev: u32,
    /// 0x5999b8: the row selected when the list fills (no write to it in 1.16.1's code).
    row: u8,
    /// 0x5999bc: the screen Ok leads to.
    next: u8,
}

impl Glue {
    /// 0x4baf30: rez\gluConn.bin (flags |= 4, images 0x4d27a0) run by 0x41a080 with the event
    /// proc 0x4bae20; ended by Ok (9) the screen is 0x5999bc (Battle.net: 0x4a5f50 and
    /// 0x452a30 first), else the main menu.
    pub(super) fn open_conn(&mut self, files: &Storage) {
        let dlg = self.open_glue_dialog(files, "rez/gluConn.bin");
        self.dlg = Some((dlg, Open::Conn));
        self.conn_init(files);
        self.show_palette();
        // The list's event 0 (proc 0x4bad10 -> 0x4bacb0), then event 10 (0x4bad70).
        self.conn_list_init();
        // Event 10 (0x4bad70) slides in the list 0x5129ec: 1, 2, 3, 4 and the gateway panel
        // 11. In 1.16.1 Battle.net is listed (and selected) first and picking another provider
        // slides the panel out again (0x4dd540(0), 12 .. 14 hidden, `conn_select`); with no
        // Battle.net here the panel is left out and hidden from the start, as the real screen
        // ends up with a LAN provider selected.
        if let Some((dlg, _)) = self.dlg.as_mut() {
            for id in ids::GATEWAY_FIRST..=ids::GATEWAY_LAST {
                if let Some(c) = dlg.get_mut(id) {
                    c.flags &= !crate::dlg::flag::VISIBLE;
                }
            }
        }
        self.slide_in(0x5129ec, 4, 0);
    }

    /// 0x4bab10 (event 0xe code 0, before 0x4de150): no provider (0x59688c = -1, Ordinal 102),
    /// the name / players / gateway texts get their buffers (the gateway hint is gluAll
    /// 0xb9), Ordinal 105 lists the providers (0x4ba610), the title music starts (track 0x16,
    /// unless 0x6d5bbc) and the game types are read again (0x401be0, 0x4ab860).
    fn conn_init(&mut self, files: &Storage) {
        self.setup.net_provider = u32::MAX;
        snet::destroy();
        todo_port!("0x4bab10", "the Battle.net connect event 0x6d5ecc (ResetEvent) and 0x66ff30");
        let hint = self.glu(0xb9).into_bytes();
        if let Some(c) = self.dlg.as_mut().and_then(|(d, _)| d.get_mut(ids::GATEWAY_HINT)) {
            c.text = hint;
        }
        self.conn.rows.clear();
        for p in snet::enum_providers() {
            self.conn_add(p);
        }
        // 0x6d5bbc (set while Battle.net's own music plays) is 0 here.
        self.music_start(0x16);
        self.load_templates(files);
    }

    /// 0x4ba610 (Ordinal 105's callback): a row for provider `p` at the end of the list; the
    /// providers of 0x5129bc show a gluAll.tbl string as their description.
    fn conn_add(&mut self, p: snet::Provider) {
        let desc = match DESC_STRINGS.iter().find(|d| d.0 == p.id) {
            Some(&(_, n)) => self.glu(n).into_bytes(),
            None => p.desc.as_bytes().to_vec(),
        };
        self.conn.rows.push(Row { id: p.id, max_players: p.max_players.min(8), name: p.name.as_bytes().to_vec(), desc });
    }

    /// 0x4bacb0 (the list's event 0): Ok off until a provider is selected (flag 2), the list
    /// filled (0x4baa80).
    fn conn_list_init(&mut self) {
        self.disable(ids::OK, true);
        self.conn_fill();
    }

    /// 0x4baa80: a row per provider (0x41b760, 0x41b410 once), then the list's proc gets event
    /// 0xe code 0xb for row 0x5999b8 (select it, 0x4ba7e0) and 0x4ba7e0 runs again.
    fn conn_fill(&mut self) {
        let names: Vec<Vec<u8>> = self.conn.rows.iter().map(|r| r.name.clone()).collect();
        let Some(c) = self.dlg.as_mut().and_then(|(d, _)| d.get_mut(ids::LIST)) else { return };
        c.list.as_mut().unwrap().batch = true;
        for n in &names {
            c.list_add(n, 0);
        }
        c.list.as_mut().unwrap().batch = false;
        c.list_layout();
        let row = self.conn.row;
        self.list_event(ids::LIST, row);
        self.conn_select();
    }

    /// 0x4ba7e0 (the list's event 0xb, after 0x41baa0 selected): the provider of the selected
    /// row (0x4ba290 matches its name) becomes 0x6d5a28; Ok on, its name, "Maximum players"
    /// line (gluAll 0x68) and description shown. Picking or leaving Battle.net shows or
    /// hides its gateway panel (0x4dd540, controls 12 .. 14).
    pub(super) fn conn_select(&mut self) {
        let Some((dlg, _)) = self.dlg.as_ref() else { return };
        let Some(sel) = dlg.get(ids::LIST).and_then(|c| c.list.as_ref()).map(|l| l.sel) else { return };
        let Some(k) = (sel != 0xff).then_some(sel as usize).filter(|&k| k < self.conn.rows.len()) else { return };
        self.disable(ids::OK, false);
        self.conn.prev = self.conn.sel;
        self.conn.sel = self.conn.rows[k].id;
        let players = self.glu(0x68).replace("%d", &self.conn.rows[k].max_players.to_string()).into_bytes();
        let (name, desc) = (self.conn.rows[k].name.clone(), self.conn.rows[k].desc.clone());
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        for (id, text) in [(ids::NAME, name), (ids::DESC, desc), (ids::PLAYERS, players)] {
            if let Some(c) = dlg.get_mut(id) {
                c.text = text;
            }
        }
        if self.conn.sel == 0x424e_4554 || self.conn.prev == 0x424e_4554 {
            todo_port!("0x4ba7e0", "the Battle.net gateway panel (0x4dd540)");
        }
    }

    /// 0x4bae20 event 0xe code 2: Ok runs 0x4bade0 first (a refusal keeps the screen); then
    /// 0x4dd040 ends the dialog with the control's id.
    pub(super) fn conn_activate(&mut self, files: &Storage, id: i16) {
        self.activated = id;
        if id == ids::OK && !self.conn_ok(files) {
            return;
        }
        self.end_dialog(id as i32);
    }

    /// 0x4bade0: Battle.net goes to its screen (4); another provider must initialize
    /// (0x4d3cc0), then the profile login (5) is next.
    fn conn_ok(&mut self, files: &Storage) -> bool {
        if self.conn.sel == 0x424e_4554 {
            self.conn.next = screen::BATTLE_NET;
            return true;
        }
        if !self.init_provider(files, self.conn.sel) {
            return false;
        }
        self.conn.next = screen::LOGIN;
        true
    }

    /// 0x4d3cc0 (EBX = the provider): Ordinal 117 initializes it, Ordinal 114 its caps:
    /// the turn size 0x57f0d8 = min(caps +8, 0x200) (under 0x80 refused), the turns in flight
    /// 0x57f090 (1 single player, else caps +0x20 clamped to 2 ..= 8); 0x59688c = the provider;
    /// 0x4b0fb0 picks a device (Ordinal 103: UDP has none); the Storm event handlers (Ordinal
    /// 123: 2 0x4c4980, 3 0x4c3c90, 4 0x47f750, 1 0x4aaea0). A failure shows network.tbl 0x53 /
    /// 0x55 / 0x56 (0x4bb0b0) and drops the provider (Ordinal 102).
    fn init_provider(&mut self, files: &Storage, id: u32) -> bool {
        todo_port!("0x4d3cc0", "0x472770, 0x4ad190 (the Storm UI callbacks), 0x4d9b50 before Ordinal 117");
        let msg = if !snet::initialize_provider(id) {
            Some(0x53)
        } else {
            snet::provider_caps().filter(|c| c.max_message >= 0x80).is_none().then_some(0x55)
        };
        if let Some(n) = msg {
            snet::destroy();
            self.error_popup(files, n);
            return false;
        }
        // 0x57f0d8 / 0x57f090 size Storm's turns; the game's turns go through our rollback
        // session instead (net/mod.rs), which has its own delay.
        self.setup.net_provider = id;
        todo_port!("0x4d3cc0", "the Storm event handlers 0x4c4980 / 0x4c3c90 / 0x47f750 / 0x4aaea0 (Ordinal 123)");
        true
    }

    /// After 0x41a080 in 0x4baf30: Ok (9) goes to 0x5999bc's screen, anything else to the
    /// main menu (0x6d11bc = 0).
    pub(super) fn done_conn(&mut self, result: i32) {
        if result == ids::OK as i32 {
            self.screen = self.conn.next;
        } else {
            self.screen = screen::MAIN;
        }
    }
}
