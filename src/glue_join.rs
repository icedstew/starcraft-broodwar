//! The join game screen (rez\gluJoin.bin, screen 10: 0x4b86c0, event proc 0x4b8590): the
//! games Storm lists on the LAN (net/snet.rs), refreshed every second, the selected one's
//! description; Ok joins it (0x4d3b50) and goes to the lobby (screen 3), Create Game to the
//! create screen (11), Cancel back to the profile login (5).
//!
//! A child module of `glue`: it extends `Glue` and uses its private state.

use super::{screen, Glue, Open};
use crate::net::{snet, statstr};
use crate::storage::Storage;
use crate::todo_port;

/// The controls of gluJoin.bin.
mod ids {
    /// The game list (proc 0x4b84a0).
    pub const LIST: i16 = 5;
    /// The description: the game type's subtype heading (6), the name (7), the game type (8),
    /// the subtype (9), the map (10), its size (11) and the speed (12).
    pub const SUB_LABEL: i16 = 6;
    pub const NAME: i16 = 7;
    pub const TYPE: i16 = 8;
    pub const SUB: i16 = 9;
    pub const MAP: i16 = 10;
    pub const SIZE: i16 = 11;
    pub const SPEED: i16 = 12;
    pub const OK: i16 = 13;
    pub const CREATE: i16 = 15;
}

/// A listed game (0x51a1f8's 0x98-byte nodes from 0x4d35e0: +8 the description).
#[derive(Clone)]
struct Listed {
    /// The Storm game id (description +0).
    id: u32,
    /// The game description (0x8d bytes, statstr.rs).
    desc: Vec<u8>,
}

/// The screen's globals.
#[derive(Default)]
pub(super) struct Join {
    /// 0x51a1f8: the games of the last Ordinal 133 call.
    games: Vec<Listed>,
    /// 0x51a234: (game id, its row): the rows of the list.
    rows: Vec<(u32, u8)>,
    /// The 1000 ms timer 0x2d (0x4b8460): ms left.
    timer: Option<u32>,
}

impl Glue {
    /// 0x4b86c0: no replay (0x6d0f14 = 0, its data freed), rez\gluJoin.bin run by 0x41a080
    /// with the event proc 0x4b8590. Ended by Ok (13) the lobby (3) is next, Cancel (14) the
    /// login (5; direct cable: the connection screen 2), Create Game (15) the create screen
    /// (11), else the main menu.
    pub(super) fn open_join(&mut self, files: &Storage) {
        self.setup.mission = false;
        let dlg = self.open_glue_dialog(files, "rez/gluJoin.bin");
        self.dlg = Some((dlg, Open::Join));
        self.join_init();
        self.show_palette();
        // The list's event 0 (proc 0x4b84a0 -> 0x4b8460), then event 10 (0x4b8560).
        self.join_list_init();
        self.slide_in(0x512a34, 4, 0);
    }

    /// 0x4b7e10 (event 0xe code 0): the join list freed (0x401270), 0x596888 = 0, Ok off.
    fn join_init(&mut self) {
        self.join.rows.clear();
        self.disable(ids::OK, true);
    }

    /// 0x4b8460: the list made (0x4b8340), laid out, the 1000 ms timer 0x2d started (its event
    /// 8 makes the list again).
    fn join_list_init(&mut self) {
        self.join_fill();
        self.join.timer = Some(1000);
    }

    /// The list box.
    fn join_list(&mut self) -> Option<&mut crate::dlg::Ctl> {
        self.dlg.as_mut().and_then(|(d, _)| d.get_mut(ids::LIST))
    }

    /// The selected row's game (0x4b7da0: the row's text names a join-list node, 0x4b7cb0;
    /// its game id finds the game).
    fn join_selected(&mut self) -> Option<Listed> {
        let sel = self.join_list()?.list.as_ref().map(|l| l.sel)?;
        if sel == 0xff {
            return None;
        }
        let id = self.join.rows.iter().find(|r| r.1 == sel)?.0;
        self.join.games.iter().find(|g| g.id == id).cloned()
    }

    /// 0x4b8340: the list made again from Ordinal 133 (0x4d37c0, a row per game, 0x4b7f50),
    /// the rows of games gone dropped (0x4b7d10); Ok only with a game listed. The game that
    /// was selected stays selected (0x4b7e60), else the first row is; then 0x4b8090.
    fn join_fill(&mut self) {
        let was = self.join_selected().map(|g| g.id);
        if let Some(c) = self.join_list() {
            c.list_clear();
        }
        self.join.rows.clear();
        self.join_enum();
        for g in self.join.games.clone() {
            self.join_add(&g);
        }
        let count = self.join.rows.len();
        self.disable(ids::OK, count == 0);
        if let Some(id) = was {
            // 0x4b7e60: the row of the game, else row 0 when there are rows.
            let row = self.join.rows.iter().find(|r| r.0 == id).map(|r| r.1);
            match row {
                Some(r) => self.list_event(ids::LIST, r),
                None if count != 0 => self.list_event(ids::LIST, 0),
                None => {}
            }
            self.join_info();
            return;
        }
        // Row 0 with games, else no selection (0xff).
        let row = if count != 0 { 0 } else { 0xff };
        self.list_event(ids::LIST, row);
        self.join_info();
    }

    /// 0x4d37c0 / Ordinal 133 with the callback 0x4d35e0: each game's stat string into a
    /// description (0x472570: a game whose string does not parse is not listed), its id,
    /// name ("%s"), players and most players (Storm's, over the stat string's); sorted by
    /// name (stricmp).
    fn join_enum(&mut self) {
        let mut games: Vec<Listed> = Vec::new();
        for e in snet::enum_games() {
            let mut d = vec![0u8; 0x8d];
            if !statstr::parse(&e.stat, &mut d) {
                continue;
            }
            d[0..4].copy_from_slice(&e.id.to_le_bytes());
            let n = e.name.len().min(0x17);
            d[4..4 + n].copy_from_slice(&e.name[..n]);
            d[0x24] = e.players;
            d[0x25] = e.max;
            let at = games.iter().position(|g| lower(&g.desc[4..0x1c]) >= lower(&d[4..0x1c])).unwrap_or(games.len());
            games.insert(at, Listed { id: e.id, desc: d });
        }
        self.join.games = games;
    }

    /// 0x4b7f50: game `g`'s row: its name, with "\x12players/most" when it says (the most is 8
    /// for a saved game, +0x32).
    fn join_add(&mut self, g: &Listed) {
        let d = &g.desc;
        let name = cstr(&d[4..0x1c]);
        let text = if d[0x24] == 0 && d[0x25] == 0 {
            name.to_vec()
        } else {
            let most = if d[0x32] == 0 { d[0x25] } else { 8 };
            let mut t = name.to_vec();
            t.push(0x12);
            t.extend_from_slice(format!("{}/{}", d[0x24], most).as_bytes());
            t
        };
        let Some(c) = self.join_list() else { return };
        let row = c.list_add(&text, 0);
        if row != 0xff {
            self.join.rows.push((g.id, row));
        }
    }

    /// 0x4b8090: the selected game's description in the panel: its name (7), map (10), size
    /// ("%ux%u", 11), game type name (8, the type record's +8), subtype heading (6, +0x28 with
    /// ':') and subtype (9, 0x4ab6c0; "saved game" gluAll 0xb2 for a saved game) and speed (12,
    /// gluAll 0x55 + speed); all empty with none selected.
    pub(super) fn join_info(&mut self) {
        let Some(g) = self.join_selected() else {
            let Some((dlg, _)) = self.dlg.as_mut() else { return };
            for id in [ids::NAME, ids::MAP, ids::TYPE, ids::SUB_LABEL, ids::SUB, ids::SIZE, ids::SPEED] {
                if let Some(c) = dlg.get_mut(id) {
                    c.text.clear();
                }
            }
            // 0x5999f0 (the description shown) is cleared.
            return;
        };
        let d = &g.desc;
        let size = format!("{}x{}", u16::from_le_bytes([d[0x20], d[0x21]]), u16::from_le_bytes([d[0x22], d[0x23]])).into_bytes();
        let (gt, b1, sub) = (d[0x28], d[0x29], u16::from_le_bytes([d[0x2a], d[0x2b]]));
        let (type_name, sub_label, sub_text) = self.join_type_texts(gt, b1, sub, d[0x25]);
        let sub_text = if d[0x32] != 0 { self.glu(0xb2).into_bytes() } else { sub_text };
        let speed = (0..7u8).find(|&s| s == d[0x26]).map(|s| self.glu(0x55 + s as usize).into_bytes());
        let (name, map) = (cstr(&d[4..0x1c]).to_vec(), cstr(&d[0x4d..0x6d]).to_vec());
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        let mut set = |id: i16, t: Vec<u8>| {
            if let Some(c) = dlg.get_mut(id) {
                c.text = t;
            }
        };
        set(ids::NAME, name);
        set(ids::MAP, map);
        set(ids::TYPE, type_name);
        set(ids::SUB_LABEL, sub_label);
        set(ids::SUB, sub_text);
        set(ids::SIZE, size);
        if let Some(s) = speed {
            set(ids::SPEED, s);
        }
        // 0x5999f0 = the description shown (0x4d3b50 joins it; here the selected game).
    }

    /// 0x4b8090's game type texts: the type's name (the first game type node with this type
    /// and byte 1), its subtype heading (+0x28, ':' appended) and, when the exact template is
    /// listed (0x4aac90), the subtype's text (0x4ab6c0 with the most players).
    pub(super) fn join_type_texts(&self, gt: u8, b1: u8, sub: u16, most: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let Some(t) = self.find_by_type(gt, b1) else { return (Vec::new(), Vec::new(), Vec::new()) };
        let name = cstr(&t.name).to_vec();
        let mut label = cstr(&t.desc).to_vec();
        if label.is_empty() {
            return (name, Vec::new(), Vec::new());
        }
        label.push(b':');
        let sub_text = self.find_template(gt, b1, sub).map(|t| self.subtype_text(&t.rec, most as u32)).unwrap_or_default();
        (name, label, sub_text)
    }

    /// 0x4b8590 event 0xe code 2: Ok joins the selected game (0x4d3b50; a failure keeps the
    /// screen); Create Game in the spawn version only shows a message (network.tbl 0x67 /
    /// gluAll); then 0x4dd040 ends the dialog with the control's id.
    pub(super) fn join_activate(&mut self, files: &Storage, id: i16) {
        self.activated = id;
        if id == ids::OK {
            if !self.join_game(files) {
                return;
            }
        } else if id == ids::CREATE && self.spawn {
            todo_port!("0x4b8590", "the spawn version's Create Game message");
            return;
        }
        self.join.timer = None;
        self.end_dialog(id as i32);
    }

    /// 0x4d3b50 (EBX = the selected description): the spawn version joins only its own kind
    /// (0x4d9cd0 vs +0x2c; network.tbl 0x69); 0x596820.. = the game type, Ordinal 118 joins
    /// (network.tbl 0x5a full / 0x59 refused). Joined, the description is the game's
    /// (0x5967f8, the template 0x596865 kept), 0x6d0f14 = +0x32 (a replay), 0x57fe40 = the map
    /// name (0x596845), 0x596888 = 0.
    fn join_game(&mut self, files: &Storage) -> bool {
        let Some(g) = self.join_selected() else { return false };
        if self.spawn {
            todo_port!("0x4d3b50", "the spawn version check (0x4d9cd0 vs the description +0x2c)");
        }
        let Some(me) = snet::join_game(g.id, None, cstr(&self.setup.local_name)) else {
            self.error_popup(files, 0x59);
            return false;
        };
        self.setup.local_storm_id = me as u32;
        self.lobby_host = false;
        // 0x4b8090 / 0x4aac90 made the selected game's type record the template (0x596865).
        let (gt, b1, sub) = (g.desc[0x28], g.desc[0x29], u16::from_le_bytes([g.desc[0x2a], g.desc[0x2b]]));
        if let Some(t) = self.find_template(gt, b1, sub).map(|t| t.rec) {
            self.setup.template.copy_from_slice(&t);
        }
        let template = self.setup.template;
        let mut d = g.desc.clone();
        d[0x6d..0x8d].copy_from_slice(&template);
        self.create.enhanced = d[0x2f] & 0x80 != 0;
        self.create.game_desc = d;
        if g.desc[0x32] != 0 {
            todo_port!("0x4d3b50", "joining a saved game or replay (0x6d0f14 = +0x32)");
        }
        true
    }

    /// Ours (1.16.1 finds games on the LAN or Battle.net only): the game joined by its code
    /// (`snet::adopt`) once the host sent its name and stat string: the provider (0x4d3cc0),
    /// the description as 0x4d3b50 makes it from a listed game, `name` as the player's, then
    /// the lobby (screen 3). False until the game's information came.
    pub fn join_by_code(&mut self, files: &Storage, name: &[u8]) -> bool {
        let (Some((_, stat)), Some(me)) = (snet::game_info(), snet::local_id()) else { return false };
        let mut d = vec![0u8; 0x8d];
        if !statstr::parse(&stat, &mut d) {
            return false;
        }
        snet::initialize_provider(snet::UDPN);
        self.setup.net_provider = snet::UDPN;
        self.setup.turns_in_flight = snet::provider_caps().map_or(1, |c| c.turns_in_flight.clamp(2, 8));
        self.load_templates(files);
        let n = name.len().min(0x18);
        self.setup.local_name = [0; 0x20];
        self.setup.local_name[..n].copy_from_slice(&name[..n]);
        self.player_name = String::from_utf8_lossy(&name[..n]).into_owned();
        self.multiplayer = true;
        self.setup.multiplayer = true;
        self.expansion = true;
        self.setup.expansion = true;
        self.setup.local_storm_id = me as u32;
        self.lobby_host = false;
        let (gt, b1, sub) = (d[0x28], d[0x29], u16::from_le_bytes([d[0x2a], d[0x2b]]));
        if let Some(t) = self.find_template(gt, b1, sub).map(|t| t.rec) {
            self.setup.template.copy_from_slice(&t);
        }
        d[0x6d..0x8d].copy_from_slice(&self.setup.template.clone());
        self.create.enhanced = d[0x2f] & 0x80 != 0;
        self.create.game_desc = d;
        self.state = super::state::GLUE;
        self.first = false;
        self.screen = screen::CHAT;
        true
    }

    /// The 1000 ms timer 0x2d (event 8 to the list: 0x4b8340).
    pub(super) fn join_tick(&mut self, ms: u32) {
        if !matches!(self.dlg, Some((_, Open::Join))) {
            return;
        }
        let Some(left) = self.join.timer else { return };
        if left > ms {
            self.join.timer = Some(left - ms);
        } else {
            self.join.timer = Some(1000);
            self.join_fill();
        }
    }

    /// After 0x41a080 in 0x4b86c0: the screen the result leads to.
    pub(super) fn done_join(&mut self, result: i32) {
        self.screen = match result {
            13 => screen::CHAT,
            14 if self.setup.net_provider == 0x5343_424c => screen::SELCONN,
            14 => screen::LOGIN,
            15 => screen::CREATE,
            _ => screen::MAIN,
        };
    }
}

/// The text before the first NUL.
fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// stricmp's key: the lowercased text before its NUL.
fn lower(b: &[u8]) -> Vec<u8> {
    cstr(b).to_ascii_lowercase()
}
