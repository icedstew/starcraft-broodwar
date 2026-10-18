//! The create game screen (rez\gluCustm.bin single player / gluCreat.bin network, dialog
//! event proc 0x4af5c0, screen 0x4af6d0): the map list (maps\ and its folders), the map's
//! information panel, the game type / subtype combos and one owner / race row per slot.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. Natively the
//! controls' event procs (table 0x51aa08) and the Windows timers (ids 5 and 0xc) drive the
//! screen; here the events reach `Glue::create_event` (event 0xe code 0xb of a list or
//! combo), `Glue::create_activate` (code 2) and `Glue::create_tick` (the timers).

use super::{Glue, Open};
use crate::dlg::{flag, Ctl, Dialog, Icon, Update};
use crate::grp::Grp;
use crate::setup::{slot, MapHeader, Setup};
use crate::storage::Storage;
use crate::todo_port;
use std::collections::HashMap;

/// The controls of gluCustm.bin / gluCreat.bin.
mod ids {
    /// The map list (0x4af020).
    pub const LIST: i16 = 5;
    /// The folder shown (0x4ad980 draws it with the folder icon).
    pub const DIR: i16 = 6;
    /// The map's name, description, players, size, tileset (0x4ae420 codes 0, 1, 4, 5, 8).
    pub const TITLE: i16 = 7;
    pub const DESC: i16 = 8;
    pub const PLAYERS: i16 = 9;
    pub const SIZE: i16 = 10;
    pub const TILESET: i16 = 11;
    /// Ok (0x4af1b0 when pressed).
    pub const OK: i16 = 12;
    /// The "Subtype" heading and the game type's heading ("Game Type").
    pub const SUB_LABEL: i16 = 14;
    pub const TYPE_LABEL: i16 = 38;
    /// The game type combo (0x4af490) and the subtype combo (0x4ae710).
    pub const GAME_TYPE: i16 = 17;
    pub const SUBTYPE: i16 = 18;
    /// The slot owner combos 20 .. 27 (the first is the player: 0x4aede0, the others 0x4aed80)
    /// and the race combos 28 .. 35 (0x4aed40).
    pub const OWNER_FIRST: i16 = 20;
    pub const OWNER_LAST: i16 = 27;
    pub const RACE_FIRST: i16 = 28;
    pub const RACE_LAST: i16 = 35;
    /// "Computer Slots" / "Human Slots" (Use Map Settings).
    pub const COMP_SLOTS: i16 = 36;
    pub const HUMAN_SLOTS: i16 = 37;
}

/// A Templates\*.got game type (a 0x190 byte node of the list 0x51a270, made by 0x4ab150):
/// +8 / +0x28 the two names, +0x48 the 0x20 byte record (+0 game type, +1, +2 subtype
/// (u16), +4 the subtype's number, +8 / +0xc / +0x12 the settings 4aaf50 tests, +0xe, +0x12
/// what a single-player game lists).
pub(super) struct Template {
    name: Vec<u8>,
    desc: Vec<u8>,
    rec: [u8; 0x20],
}

/// A map list node (0x544 bytes, the list 0x51a27c).
#[derive(Clone, Default)]
pub(super) struct MapNode {
    /// +0x268: 4 a map (.scm / .scx), 8 a folder, 1 a saved game, 0x80 a replay, 0x20 sort.
    flags: u8,
    /// +0x269: the file or folder.
    path: String,
    /// +0x36d: its name.
    name: Vec<u8>,
    /// +8: the name shown in the list ("[folder]").
    disp: Vec<u8>,
    /// +0x258: the information was read (set when 0x4a7740 starts).
    loaded: bool,
    /// +0x25c: 1 reading, 2 a map for the expansion without it (else 0).
    state: u32,
    /// +0x49 / +0x69: the map's name and description.
    title: Vec<u8>,
    desc: Vec<u8>,
    /// +0x1a5 / +0x1c8 / +0x1eb / +0x20e / +0x231: the formatted players, computer slots,
    /// human slots, size and tileset.
    players_s: Vec<u8>,
    comps_s: Vec<u8>,
    humans_s: Vec<u8>,
    size_s: Vec<u8>,
    tileset_s: Vec<u8>,
    /// +0x472 / +0x474 / +0x476: size and tileset.
    width: u16,
    height: u16,
    /// +0x479 players, +0x47a computer slots, +0x47b human slots.
    players: u8,
    comps: u8,
    humans: u8,
    /// +0x260: the league priority of the map.
    league: u32,
    /// +0x535: the 8 "user selectable race" flags (0x59bda8).
    user_sel: [u8; 8],
}

/// 0x4a6dd0: what the list says of a row's node.
#[derive(Clone, Copy)]
struct NodeInfo {
    loaded: bool,
    flags: u8,
    /// 0xff while not read.
    comps: u8,
    humans: u8,
    players: u8,
}

/// The create game screen's globals.
pub(super) struct Create {
    /// 0x51a270: the game types.
    templates: Vec<Template>,
    /// 0x51a27c: the map list's nodes.
    nodes: Vec<MapNode>,
    /// 0x59bb70: the folder listed; 0x59bf78 the folder "Up One Level" stops at.
    dir: String,
    root: String,
    /// 0x59b850 / 0x59bc88: the folder and the row text kept for the next time.
    saved_dir: String,
    saved_name: Vec<u8>,
    /// 0x59ba68: the row text to select in a new listing.
    last_name: Vec<u8>,
    /// 0x6d63c8: the first screen of the session chose its folder.
    first_done: bool,
    /// 0x59b844 (0 the create game screen, 1 the replay list).
    which: u8,
    /// 0x59ba64 / 0x59ba5c: the chosen game type (low byte; byte 1) and subtype.
    cur: u32,
    sub: u32,
    /// 0x6d5a78: the players of the selected map.
    players: u32,
    /// 0x59bb6c: the game speed.
    speed: u8,
    /// 0x59b848: the slot rows follow a map selection (not Ok).
    slot_poll: bool,
    /// The 100 ms timers: 0xc loads the selected node's information (0x59bf70 its node),
    /// 5 fills the slot rows.
    info_timer: Option<(u32, usize)>,
    slot_timer: Option<u32>,
    /// 0x57f266 .. 0x57f26e: the races the rows chose (the local player, then up to 8
    /// computers; 7 none) and 0x57f1e3: they were chosen. 0x4ee110 (not ported) reads them.
    race_pick: [u8; 9],
    race_picked: bool,
    /// 0x59bf60 .. 0x59bf6c: the races left to deal out (0x4a94c0), 0x6d11c8 (random numbers
    /// are drawn) and 0x51ca14 (their seed).
    bag_left: u32,
    bag: [u32; 3],
    rand_on: bool,
    seed: u32,
    /// 0x5967f8: the description of the single-player game (0x8d bytes) 0x4d3fc0 takes.
    game_desc: Vec<u8>,
    /// The registry values ("Custom Type", "Game Subtype", "Game Speed") Storm keeps; here
    /// for the session only.
    prefs: HashMap<&'static str, Vec<u8>>,
}

impl Default for Create {
    fn default() -> Create {
        Create {
            templates: Vec::new(),
            nodes: Vec::new(),
            dir: String::new(),
            root: String::new(),
            saved_dir: String::new(),
            saved_name: Vec::new(),
            last_name: Vec::new(),
            first_done: false,
            which: 0,
            cur: 0,
            sub: 0,
            players: 0,
            speed: 0,
            slot_poll: false,
            info_timer: None,
            slot_timer: None,
            race_pick: [7; 9],
            race_picked: false,
            bag_left: 3,
            bag: [0; 3],
            rand_on: false,
            seed: 0,
            game_desc: Vec::new(),
            prefs: HashMap::new(),
        }
    }
}

/// The game speed names / the race names of the combos are looked up natively through
/// rez\network.tbl; 0x51aaa0 pairs a race with its string (1-based), 0x51aaa8 a slot owner
/// with its gluAll.tbl string.
const RACES: [(u8, usize); 4] = [(0, 7), (1, 8), (2, 9), (6, 10)];
const OWNERS: [(u32, usize); 2] = [(0, 0x86), (1, 0x84)];

/// 0x4a6510: the order of the map list: "Up One Level" first, folders before files, names
/// with a ':' third last, else the names without case. Negative: `a` goes before `b`.
fn node_cmp(a: &[u8], b: &[u8]) -> i32 {
    fn ic(a: &[u8], b: &[u8]) -> i32 {
        let (x, y) = (a.iter().map(u8::to_ascii_lowercase), b.iter().map(u8::to_ascii_lowercase));
        match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
    const UP: &[u8] = b"Up One Level";
    if ic(a, UP) == 0 {
        return -1;
    }
    if ic(b, UP) == 0 {
        return 1;
    }
    let (ab, bb) = (a.first() == Some(&b'['), b.first() == Some(&b'['));
    if ab && !bb {
        return -1;
    }
    if !ab && bb {
        return 1;
    }
    let (ac, bc) = (a.get(2) == Some(&b':'), b.get(2) == Some(&b':'));
    if ac && !bc {
        return 1;
    }
    if !ac && bc {
        return -1;
    }
    ic(a, b)
}

/// 0x4a66c0: `node` into the list before the first node it does not follow (a node that
/// follows every one before it and not the one after, the end counting as "after").
fn insert_sorted(nodes: &mut Vec<MapNode>, node: MapNode) {
    let mut prev: Option<usize> = None;
    loop {
        let (before, cand) = match prev {
            None => (1, if nodes.is_empty() { None } else { Some(0) }),
            Some(p) => (node_cmp(&node.disp, &nodes[p].disp), (p + 1 < nodes.len()).then_some(p + 1)),
        };
        let after = cand.map_or(-1, |c| node_cmp(&node.disp, &nodes[c].disp));
        if before > 0 && after <= 0 {
            nodes.insert(prev.map_or(0, |p| p + 1), node);
            return;
        }
        match cand {
            Some(_) => prev = cand,
            None => return,
        }
    }
}

/// SStrCopy (Ordinal 501): at most `max - 1` bytes.
fn sstr(s: &[u8], max: usize) -> Vec<u8> {
    let s = s.split(|&c| c == 0).next().unwrap_or(&[]);
    s[..s.len().min(max - 1)].to_vec()
}

/// 0x41a890's comparison: byte `i` of both, without case.
fn same(a: u8, b: u8) -> bool {
    a.eq_ignore_ascii_case(&b)
}

impl Glue {
    // ---- the controls (0x418640 / 0x418e00 / 0x4186a0 / 0x418700 / 0x41b760 ...) ----

    fn cdlg(&mut self) -> Option<&mut Dialog> {
        match self.dlg.as_mut() {
            Some((d, Open::Create(_))) => Some(d),
            _ => None,
        }
    }

    fn cctl(&mut self, id: i16) -> Option<&mut Ctl> {
        self.cdlg()?.get_mut(id)
    }

    /// A list or combo's row count and selected row (0xff none).
    fn clist(&mut self, id: i16) -> (u8, u8) {
        self.cctl(id).and_then(|c| c.list.as_ref()).map_or((0, 0xff), |l| (l.items.len() as u8, l.sel))
    }

    /// The data of row `row` (0x42 array).
    fn cdata(&mut self, id: i16, row: u8) -> Option<u32> {
        self.cctl(id)?.list.as_ref()?.items.get(row as usize).map(|i| i.2)
    }

    /// The data of the selected row.
    fn csel_data(&mut self, id: i16) -> Option<u32> {
        let (_, sel) = self.clist(id);
        self.cdata(id, sel)
    }

    /// 0x4186a0 (`on`) / 0x418700: show or hide the control.
    fn cshow(&mut self, id: i16, on: bool) {
        if let Some(c) = self.cctl(id) {
            if on {
                c.flags |= flag::VISIBLE;
            } else {
                c.flags &= !flag::VISIBLE;
            }
        }
    }

    /// 0x418e00 (`on`) / 0x418640: enable or disable the control.
    fn cenable(&mut self, id: i16, on: bool) {
        if let Some(c) = self.cctl(id) {
            if on {
                c.flags &= !flag::DISABLED;
            } else {
                c.flags |= flag::DISABLED;
            }
        }
    }

    /// Event 0xe code 0xb to list / combo `id` (its proc selects the row and goes on), then
    /// 0x41a600 scrolls to the row.
    fn cselect(&mut self, id: i16, row: u8) {
        self.list_event(id, row);
        if let Some(c) = self.cctl(id) {
            c.list_scroll_to(row);
        }
    }

    /// 0x41b760 on a list or combo: a row, its data set; the row (0xff when full).
    fn cadd(&mut self, id: i16, text: &[u8], flags: u8, data: u32) -> u8 {
        let Some(c) = self.cctl(id) else { return 0xff };
        let row = c.list_add(text, flags);
        if let Some(i) = c.list.as_mut().and_then(|l| l.items.get_mut(row as usize)) {
            i.2 = data;
        }
        row
    }

    /// 0x4dcf30: the control's text.
    fn ctext(&mut self, id: i16, text: &[u8]) {
        if let Some(c) = self.cctl(id) {
            c.text = text.to_vec();
        }
    }

    /// gluAll.tbl string `n` (1-based, 0x4ddd30).
    fn glu(&self, n: usize) -> String {
        self.glu_all.get(n.wrapping_sub(1)).cloned().unwrap_or_default()
    }

    /// The row of the selected map list entry's node.
    fn cnode(&mut self) -> Option<usize> {
        self.csel_data(ids::LIST).map(|d| (d & 0xff_ffff) as usize)
    }

    /// 0x4a6dd0 on node `idx`.
    fn node_info(&self, idx: usize) -> Option<NodeInfo> {
        let n = self.create.nodes.get(idx)?;
        let (comps, humans, players) = if n.loaded { (n.comps, n.humans, n.players) } else { (0xff, 0xff, 0xff) };
        Some(NodeInfo { loaded: n.loaded, flags: n.flags & 0x8f, comps, humans, players })
    }

    // ---- the game types (0x51a270) ----

    /// 0x4ab860 with 0x4ab150: the game types of Templates\templates.lst, a node per quoted
    /// file name (0x4aae20) whose .got 0x4ab750 accepts.
    fn load_templates(&mut self, files: &Storage) {
        if !self.create.templates.is_empty() {
            return;
        }
        let Ok(list) = files.read("Templates\\templates.lst") else { crate::bw::fatal_error("Unable to read game templates.") };
        for name in list.split(|&c| c == b'"').skip(1).step_by(2) {
            let name = String::from_utf8_lossy(&name[..name.len().min(0x103)]).into_owned();
            let name = name.rsplit_once('.').map_or(name.as_str(), |(n, _)| n);
            let (mut a, mut b) = ([0u8; 0x20], [0u8; 0x20]);
            if let Some(rec) = Setup::load_got(files, name, &mut a, &mut b) {
                self.create.templates.push(Template { name: sstr(&a, 0x20), desc: sstr(&b, 0x20), rec });
            }
        }
    }

    /// 0x4aac90: the record of the game type (`gt`, `b1`, `sub` at +0 / +1 / +2).
    fn find_template(&self, gt: u8, b1: u8, sub: u16) -> Option<&Template> {
        self.create.templates.iter().find(|t| t.rec[0] == gt && t.rec[1] == b1 && u16::from_le_bytes([t.rec[2], t.rec[3]]) == sub)
    }

    /// 0x4aaf50: the game type (`gt`, `b1`, `sub`) exists and plays by the map's settings (+8,
    /// +0xc and +0x12 are 0: Use Map Settings).
    fn template_plain(&self, gt: u8, b1: u8, sub: u16) -> bool {
        self.find_template(gt, b1, sub).is_some_and(|t| t.rec[8] == 0 && t.rec[0xc] == 0 && t.rec[0x12] == 0)
    }

    /// 0x4aafa0: the game type exists and has +8 = 1 (the races are the players' to pick).
    fn template_melee(&self, gt: u8, b1: u8, sub: u16) -> bool {
        self.find_template(gt, b1, sub).is_some_and(|t| t.rec[8] == 1)
    }

    /// 0x4aaf50 for the game type the lists show (0x59ba64 / 0x59ba5c).
    fn cur_plain(&self) -> bool {
        let c = &self.create;
        self.template_plain(c.cur as u8, (c.cur >> 8) as u8, c.sub as u16)
    }

    /// 0x4ab5b0: the game types a list shows: single player those without +0x12 whose +0xe
    /// is not 0 or 1 (a network game's also have no +0x12 unless on battle.net).
    fn template_listed(&self, rec: &[u8; 0x20]) -> bool {
        // 0x4ab5b0 (the argument is 0): battle.net lists what a ladder game may play.
        let (mp, bnet) = (self.setup.multiplayer, self.setup.net_provider == 0x424e4554);
        if !(mp && bnet) && rec[0x12] != 0 {
            return false;
        }
        if rec[0] == 0x80 && rec[1] != 0 {
            return false;
        }
        mp || (rec[0xe] != 0 && rec[0xe] != 1)
    }

    /// 0x4ab6c0: a subtype's text: its number, or "%d vs %d" for Top vs Bottom.
    fn subtype_text(&self, rec: &[u8; 0x20], players: u32) -> Vec<u8> {
        let n = u32::from_le_bytes(rec[4..8].try_into().unwrap());
        match rec[0] {
            9 => self.glu(n as usize + 0xa4).into_bytes(),
            0xf => self.glu(0xac).replacen("%d", &n.to_string(), 1).replacen("%d", &players.wrapping_sub(n).to_string(), 1).into_bytes(),
            _ => n.to_string().into_bytes(),
        }
    }

    // ---- opening (0x4af6d0 .. 0x4af510, 0x4adac0) ----

    /// The events a new dialog gets: 10 (0x4af510: the controls the procs use, the game
    /// speed), 0 (0x4adac0, the palette 0x4de150, then every control's own init), and when the
    /// controls have slid in (0x4dd5e0: not ported, so at once) 0x405 to the combos and the
    /// dialog.
    pub(super) fn create_open(&mut self, files: &Storage, which: u8) {
        self.load_templates(files);
        self.create.which = which;
        // 0x4af6d0: 0x59ba60 is iCreate.grp (the row and label icons); 0x6d5a74 = 1.
        let icons = files.read("glue/create/iCreate.grp").ok().map(Grp::parse);
        // Event 10 of the controls' procs: the list (0x4af020) takes flags 0x30000, the combos
        // (0x4af490, 0x4ae710, 0x4aede0, 0x4aed80, 0x4aed40) 0x20000; the subtype combo
        // (0x4ae710) starts hidden with subtype 1.
        if let Some(d) = self.cdlg() {
            for c in d.controls.iter_mut() {
                match c.id {
                    ids::LIST => c.flags |= 0x30000,
                    ids::GAME_TYPE | ids::SUBTYPE | ids::OWNER_FIRST..=ids::RACE_LAST => c.flags |= 0x20000,
                    _ => {}
                }
            }
        }
        self.cshow(ids::SUBTYPE, false);
        self.create.sub = 1;
        // 0x4af510 (event 10): 0x59b840 / 0x59bc74 / 0x59bc80 are controls 19 / 18 / 17.
        if !self.setup.multiplayer {
            self.create.speed = crate::menu::Settings::default().game_speed as u8;
        }
        // 0x41bc40 event 10 starts a combo's rows like a list box's 0x41b140.
        let p = self.palette_id();
        self.look(files, p);
        let skin = &self.looks[p].as_ref().unwrap().skin;
        if let Some((d, _)) = self.dlg.as_mut() {
            for c in d.controls.iter_mut().filter(|c| c.ty == 13 && c.list.is_none()) {
                skin.list_init(c);
            }
        }
        // 0x4adac0: the buttons slide in (0x4dd5e0 over list 0x51a9f0: not ported) and
        // the folder label gets its draw 0x4ad980.
        todo_port!("0x4dd5e0", "slide-in of the controls (list 0x51a9f0) and its timer 0x4dd220");
        todo_port!("0x4af5c0", "mouse wheel (events 0x11 / 0x12) to the map list's scroll bar");
        if self.setup.multiplayer {
            todo_port!("0x4ae860", "the network screen's game speed slider (id 16, 0x4ae250 / 0x4ae3c0) and game name");
        }
        if let Some(icons) = icons {
            let frames: Vec<Icon> = (0..icons.frame_count())
                .filter_map(|i| {
                    let f = *icons.frame(i)?;
                    let (px, w, h) = icons.pixels(i)?;
                    Some(Icon { x: f.x, y: f.y, w, h, px })
                })
                .collect();
            if let Some(c) = self.cctl(ids::DIR) {
                c.update = Update::DirLabel;
                c.icons = frames.clone();
            }
            if let Some(c) = self.cctl(ids::LIST) {
                c.update = Update::MapList;
                c.icons = frames;
            }
        }
        self.show_palette();
        // The default proc 0x418eb0 sends event 0 to every control, in order.
        self.create_list_init();
        self.create_fill_owners();
        self.create_fill_races();
        // 0x405: the combos (17: 0x4af400), then the dialog (0x4af100).
        self.create_fill_types();
        self.create_fill_list();
    }

    /// 0x4ae920 (the list's event 0): the folder to list (the first time maps\, with
    /// BroodWar\ for the expansion; later the one left; a replay list maps\replays), the
    /// folder's name over the list. 0x4dcb00 makes a path absolute (the install's directory
    /// first); the paths here are absolute from the start, with '/' as the separator.
    fn create_list_init(&mut self) {
        let maps = crate::install_dir().join("maps");
        let slash = |p: std::path::PathBuf| p.to_string_lossy().replace('\\', "/");
        let c = &mut self.create;
        if c.which != 0 {
            c.dir = slash(maps.join("replays"));
            c.root = c.dir.clone();
        } else {
            c.root = slash(maps.clone());
            if !c.first_done {
                c.first_done = true;
                let dir = if self.expansion { maps.join("BroodWar") } else { maps };
                c.saved_dir = slash(dir);
                c.dir = c.saved_dir.clone();
            } else {
                c.dir = c.saved_dir.clone();
                c.last_name = c.saved_name.clone();
            }
        }
        // 0x4dca20 makes the folder relative to the install (0x59b958); 0x41eab0 creates
        // it when it is missing.
        let _ = std::fs::create_dir_all(&c.dir);
        self.create_dir_label();
        // 0x4ae920 also gives the info labels their setters (0x4ae420) and the list its
        // row draw 0x4ad6b0.
    }

    /// 0x4ada80: the folder's name (after the last '\') in the label over the list.
    fn create_dir_label(&mut self) {
        let name = self.create.dir.rsplit_once('/').map_or(self.create.dir.as_str(), |(_, n)| n).as_bytes().to_vec();
        self.ctext(ids::DIR, &name);
    }

    /// 0x4aede0 (id 20, event 0) / 0x4ae660 (ids 21 .. 27, event 0): the first slot's owner
    /// is the player's name; the others list Closed and Computer, Computer selected.
    fn create_fill_owners(&mut self) {
        let name = sstr(&self.setup.local_name, 0x19);
        self.cadd(ids::OWNER_FIRST, &name, 0, 0);
        if let Some(c) = self.cctl(ids::OWNER_FIRST) {
            // The combo shows its first row (0x41bc40).
            c.list_select(0);
        }
        for i in ids::OWNER_FIRST + 1..=ids::OWNER_LAST {
            for (ty, s) in OWNERS {
                let text = self.glu(s).into_bytes();
                self.cadd(i, &text, 0, ty);
            }
            let comp = self.cctl(i).and_then(|c| c.list.as_ref()).and_then(|l| l.items.iter().position(|r| r.2 == 1)).unwrap_or(0xff) as u8;
            self.cselect(i, comp);
        }
    }

    /// 0x4adf20 (ids 28 .. 35, event 0): Zerg, Terran, Protoss and Random; Random selected.
    fn create_fill_races(&mut self) {
        for i in ids::RACE_FIRST..=ids::RACE_LAST {
            for (race, s) in RACES {
                let text = self.network.get(s - 1).cloned().unwrap_or_default().into_bytes();
                self.cadd(i, &text, 0, race as u32);
            }
            let (count, _) = self.clist(i);
            if count > 3 {
                self.cselect(i, 3);
            }
        }
    }

    // ---- the map list (0x4a73c0, 0x4a7050, 0x4af100) ----

    /// 0x4a6b60: a folder node ("[name]"); ".." is the parent ("[Up One Level]") unless the
    /// listing is the root.
    fn add_dir_node(&mut self, name: &str) {
        let c = &mut self.create;
        let mut n = MapNode { flags: 8, ..Default::default() };
        if name == ".." {
            if c.root.eq_ignore_ascii_case(&c.dir) {
                return;
            }
            n.path = c.dir.rsplit_once('/').map_or(String::new(), |(p, _)| p.to_string());
            n.disp = format!("[{}]", "Up One Level").into_bytes();
            // 0x4017b0 puts it first.
            c.nodes.insert(0, n);
            return;
        }
        n.path = format!("{}/{name}", c.dir.trim_end_matches('/'));
        n.disp = sstr(format!("[{name}]").as_bytes(), 0x41);
        insert_sorted(&mut c.nodes, n);
    }

    /// 0x4a6a70: a file node; `flags` & 0x20 sorts it in, else it goes last.
    fn add_file_node(&mut self, flags: u8, name: &str) {
        let c = &mut self.create;
        let n = MapNode {
            flags,
            path: format!("{}/{name}", c.dir.trim_end_matches('/')),
            name: sstr(name.as_bytes(), 0x104),
            disp: sstr(name.as_bytes(), 0x41),
            ..Default::default()
        };
        if flags & 0x20 != 0 {
            insert_sorted(&mut c.nodes, n);
        } else {
            c.nodes.push(n);
        }
    }

    /// 0x4a7050 (`param` & 0x70 are kept in the nodes' flags, & 8 lists the folders): the
    /// files of the folder: .scm (.scx with Brood War) maps, in a network game the saved
    /// games, and replays.
    fn scan_dir(&mut self, param: u8) {
        let Ok(rd) = std::fs::read_dir(&self.create.dir) else { return };
        let mut entries: Vec<(String, bool)> = rd
            .flatten()
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?.is_dir())))
            // A hidden file (0x4a7050 skips attribute 2) is a dotfile here.
            .filter(|(n, _)| !n.starts_with('.'))
            .collect();
        // FindFirstFile's order on the native file system.
        entries.sort_by_key(|(n, _)| n.to_ascii_lowercase());
        let keep = param & 0x70;
        let save_ext = if self.expansion { "mlx" } else { "mlt" };
        if param & 8 != 0 {
            self.add_dir_node("..");
        }
        for (name, dir) in entries {
            if dir {
                if param & 8 != 0 {
                    self.add_dir_node(&name);
                }
                continue;
            }
            let Some((_, ext)) = name.rsplit_once('.') else { continue };
            let flags = if (self.expansion && ext.eq_ignore_ascii_case("scx")) || ext.eq_ignore_ascii_case("scm") {
                keep | 4
            } else if self.setup.multiplayer && ext.eq_ignore_ascii_case(save_ext) {
                keep | 1
            } else if ext.eq_ignore_ascii_case("rep") {
                keep | 0x80
            } else {
                continue;
            };
            self.add_file_node(flags, &name);
        }
    }

    /// 0x4a73c0 (`param`, the folder 0x59bb70, `name` the row to select): list the folder and
    /// add a row per node (the callback 0x4adee0: flags 3, the node as data); the row of
    /// the node called `name`, else of the first map, else row 0 (0xff when empty).
    fn list_dir(&mut self, param: u8, name: Option<&[u8]>) -> u8 {
        // 0x6d5aa8 = "Up One Level" (gluAll 0x1c) is `node_cmp`'s constant.
        self.create.info_timer = None;
        self.create.nodes.clear();
        if param & 0x10 == 0 {
            self.scan_dir(param);
        } else {
            todo_port!("0x4a6ee0", "the recent maps list of the registry");
        }
        let rows: Vec<(Vec<u8>, u32)> = self.create.nodes.iter().enumerate().map(|(i, n)| (n.disp.clone(), (n.flags as u32) << 24 | i as u32)).collect();
        let mut row_of = Vec::new();
        for (text, data) in &rows {
            row_of.push(self.cadd(ids::LIST, text, 3, *data));
        }
        if let Some(name) = name {
            for (n, &row) in self.create.nodes.iter().zip(&row_of) {
                if name.len() >= n.disp.len() && name[..n.disp.len()].eq_ignore_ascii_case(&n.disp) {
                    return row;
                }
            }
        }
        for (n, &row) in self.create.nodes.iter().zip(&row_of) {
            let ext = n.path.rsplit_once('.').map(|(_, e)| e);
            if ext.is_some_and(|e| e.eq_ignore_ascii_case("scx") || e.eq_ignore_ascii_case("scm")) {
                return row;
            }
        }
        if self.create.nodes.is_empty() { 0xff } else { 0 }
    }

    /// 0x4af100 (event 0x405 of the dialog, and Ok on a folder): the list is made again and
    /// the row 0x4a73c0 found selected (event 0xb) and shown.
    fn create_fill_list(&mut self) {
        if let Some(c) = self.cctl(ids::LIST) {
            if let Some(l) = c.list.as_mut() {
                l.batch = true;
            }
            c.list_clear();
            if let Some(l) = c.list.as_mut() {
                l.batch = true;
            }
        }
        let last = (!self.create.last_name.is_empty()).then(|| self.create.last_name.clone());
        let row = self.list_dir(0x28, last.as_deref());
        if let Some(c) = self.cctl(ids::LIST) {
            if let Some(l) = c.list.as_mut() {
                l.batch = false;
            }
            c.list_layout();
        }
        if row != 0xff {
            if let Some(n) = self.cdata(ids::LIST, row) {
                self.info_queue((n & 0xff_ffff) as usize);
            }
        }
        let (count, _) = self.clist(ids::LIST);
        if row < count || row == 0xff {
            self.cselect(ids::LIST, row);
        }
    }

    // ---- the map's information (0x4a7fc0, 0x4a7f50, 0x4a7740, 0x4a79d0, 0x4a6580) ----

    /// 0x4a7fc0 (EDI = node): the node's information shows now when it is a folder or was
    /// read; else the labels are cleared and the 100 ms timer 0xc reads it.
    fn info_queue(&mut self, idx: usize) {
        self.create.info_timer = None;
        let Some(n) = self.create.nodes.get(idx) else { return };
        if n.flags & 0x85 == 0 || n.loaded {
            self.info_apply(idx);
        } else {
            self.info_clear();
            self.create.info_timer = Some((100, idx));
        }
    }

    /// 0x4a7f50 (the timer 0xc): read the node (a map 0x4a7740, a saved game 0x4a7540, a
    /// replay 0x4a7c30), then show it.
    fn info_timer_fired(&mut self, files: &Storage, idx: usize) {
        let Some(flags) = self.create.nodes.get(idx).map(|n| n.flags) else { return };
        if flags & 4 != 0 {
            self.map_info_load(files, idx);
        } else if flags & 1 != 0 {
            todo_port!("0x4a7540", "the saved game's information (0x4cfa10)");
        } else if flags & 0x80 != 0 {
            todo_port!("0x4a7c30", "the replay's information (0x4df570, 0x4de9d0)");
        }
        self.info_apply(idx);
    }

    /// 0x4a7740: read the map `idx`'s header (0x4bf5d0: the lobby slots, forces and title
    /// of the setup change too) and set its texts, size, players and slots.
    fn map_info_load(&mut self, files: &Storage, idx: usize) {
        let net = self.network_raw.clone();
        let Some(n) = self.create.nodes.get(idx) else { return };
        if n.loaded {
            return;
        }
        let (path, name_len, path_len) = (n.path.clone(), n.name.len(), n.path.len());
        let invalid = sstr(self.glu(0x3d).as_bytes(), 0x20);
        {
            let n = &mut self.create.nodes[idx];
            n.loaded = true;
            n.state = 1;
            n.title = invalid;
        }
        if name_len >= 0x20 || path_len + 1 + name_len > 0x104 {
            let d = sstr(self.glu(0x3e).as_bytes(), 0x100);
            self.create.nodes[idx].desc = d;
            return;
        }
        let mut hdr: MapHeader = [0; 0x20];
        if !self.setup.load_map(files, &path, &mut hdr, false, &net) {
            let ver = u16::from_le_bytes([hdr[0x18], hdr[0x19]]);
            let (state, msg) = if ver > 0x3b { (2, 0x3c) } else { (1, 0x3f) };
            let d = sstr(self.glu(msg).as_bytes(), 0x100);
            let n = &mut self.create.nodes[idx];
            n.state = state;
            n.desc = d;
            return;
        }
        let players = self.setup.lobby_occupied();
        let comps = self.setup.lobby[..8].iter().rev().filter(|s| s[slot::TYPE] == 1 || s[slot::TYPE] == 5).count() as u8;
        let humans = self.setup.slots[..8].iter().rev().filter(|s| s[slot::TYPE] == 6).count() as u8;
        let (w, h, tile) = (self.setup.map_width, self.setup.map_height, self.setup.tileset);
        let str_of = |s: &Setup, id: u16| if id == 0 { Vec::new() } else { s.map_string(id).unwrap_or_default().to_vec() };
        let mut title = sstr(&str_of(&self.setup, u16::from_le_bytes([hdr[0], hdr[1]])), 0x20);
        if title.is_empty() {
            title = sstr(&self.create.nodes[idx].name, 0x20);
        }
        let desc = sstr(&str_of(&self.setup, u16::from_le_bytes([hdr[2], hdr[3]])), 0x100);
        let num = |s: &Glue, k: usize| s.glu(k);
        let players_s = num(self, 0x1e).replacen("%u", &players.to_string(), 1).into_bytes();
        let size_s = num(self, 0x20).replacen("%u", &w.to_string(), 1).replacen("%u", &h.to_string(), 1).into_bytes();
        let comps_s = num(self, 0x23).replacen("%u", &comps.to_string(), 1).into_bytes();
        let humans_s = num(self, 0x25).replacen("%u", &humans.to_string(), 1).into_bytes();
        let tileset_s = sstr(self.glu(tile as usize + 0x27).as_bytes(), 0x23);
        let user_sel = self.setup.user_selectable;
        let n = &mut self.create.nodes[idx];
        n.title = title;
        n.desc = desc;
        n.players_s = players_s;
        n.size_s = size_s;
        n.comps_s = comps_s;
        n.humans_s = humans_s;
        n.tileset_s = tileset_s;
        n.width = w;
        n.height = h;
        n.players = players;
        n.comps = comps;
        n.humans = humans;
        n.user_sel = user_sel;
        n.league = u32::from_le_bytes(hdr[0x1c..0x20].try_into().unwrap());
        n.state = 0;
    }

    /// 0x4ae420: the information label `code` (0 name, 1 description, 4 players, 5 size, 6
    /// computer slots, 7 human slots, 8 tileset) shows `text`; 4 .. 7 are "Label:\x12value"
    /// (gluAll 0x1f / 0x21 / 0x24 / 0x26 / 0x22) and 4 hides with Use Map Settings, 6 and 7
    /// show only with it.
    fn info_label(&mut self, code: u8, text: &[u8]) {
        let (cid, fmt) = match code {
            0 => (ids::TITLE, 0),
            1 => (ids::DESC, 0),
            4 => (ids::PLAYERS, 0x1f),
            5 => (ids::SIZE, 0x21),
            6 => (ids::COMP_SLOTS, 0x24),
            7 => (ids::HUMAN_SLOTS, 0x26),
            8 => (ids::TILESET, 0x22),
            _ => return,
        };
        let mut t = text.to_vec();
        if fmt != 0 {
            let f = self.glu(fmt);
            if f.len() + text.len() >= 0x80 {
                return;
            }
            t = f.trim_end_matches("%c%s").as_bytes().to_vec();
            t.push(0x12);
            t.extend_from_slice(text);
        }
        self.ctext(cid, &t);
        match code {
            4 => {
                let plain = self.cur_plain();
                self.cshow(cid, !plain);
            }
            6 | 7 => {
                let plain = self.cur_plain();
                self.cshow(cid, plain);
            }
            _ => {}
        }
    }

    /// 0x4a6580: every information label empty, the user selectable races (0x59bda8) none.
    fn info_clear(&mut self) {
        for code in [0, 1, 4, 6, 7, 5, 8] {
            self.info_label(code, b"");
        }
        self.setup.user_selectable = [0; 8];
    }

    /// 0x4a79d0: show node `idx`'s texts (the description without control characters), take
    /// its user selectable races, then (the screen is open) the players the subtype list
    /// and the controls follow.
    fn info_apply(&mut self, idx: usize) {
        let Some(n) = self.create.nodes.get(idx).cloned() else { return };
        // The "game type" text of saved games and replays (+0x169, 0x4ab0e0) has no label.
        let desc: Vec<u8> = n.desc.iter().copied().filter(|&c| c >= 0x20 || matches!(c, 10 | 13 | 9)).collect();
        self.info_label(0, &n.title);
        self.info_label(1, &desc);
        self.info_label(4, &n.players_s);
        self.info_label(6, &n.comps_s);
        self.info_label(7, &n.humans_s);
        self.info_label(5, &n.size_s);
        self.info_label(8, &n.tileset_s);
        self.setup.user_selectable = n.user_sel;
        // 0x6d11a1 (the same flags for the game) is not modelled.
        self.create.players = if n.flags & 0x80 != 0 { 0 } else { n.players as u32 };
        if self.create.cur as u8 == 0xf {
            self.create_fill_subtypes();
        }
        self.create_type_controls();
    }

    // ---- game type and subtype (0x4af400, 0x4aeed0, 0x4ae790, 0x4ad640, 0x4adb10) ----

    /// 0x41a890: the row of list `id` whose text starts with `query` (selected, scrolled
    /// to); else (a longest match is never kept) 0xff, which is selected too.
    fn find_text(&mut self, id: i16, query: &[u8]) -> u8 {
        let rows: Vec<Vec<u8>> = self.cctl(id).and_then(|c| c.list.as_ref()).map(|l| l.items.iter().map(|i| i.0.clone()).collect()).unwrap_or_default();
        for (i, text) in rows.iter().enumerate() {
            let q = |k: usize| query.get(k).copied().unwrap_or(0);
            let t = |k: usize| text.get(k).copied().unwrap_or(0);
            if t(0) == 0 || !same(q(0), t(0)) {
                continue;
            }
            let mut k = 0;
            loop {
                k += 1;
                if q(k) == 0 {
                    self.cselect(id, i as u8);
                    return i as u8;
                }
                if !same(q(k), t(k)) {
                    break;
                }
            }
        }
        self.cselect(id, 0xff);
        0xff
    }

    /// 0x4ad5b0: the game type of last time (the registry's "Custom Type") selected if
    /// there is one, else the first row. True when it was found.
    fn restore_game_type(&mut self) -> bool {
        let (count, _) = self.clist(ids::GAME_TYPE);
        if count == 0 {
            return false;
        }
        if let Some(v) = self.create.prefs.get("Custom Type").cloned() {
            if self.find_text(ids::GAME_TYPE, &v) != 0xff {
                return true;
            }
        }
        let (count, _) = self.clist(ids::GAME_TYPE);
        if count != 0 {
            self.cselect(ids::GAME_TYPE, 0);
        }
        false
    }

    /// 0x4af400 (event 0x405 of the game type combo): a row per game type of the list that
    /// has a subtype 1 (0x4aad00, 0x4adea0 with 0x4ab5b0: its name, its game type as data);
    /// the one of last time or, in single player, the last whose +8 is 1 (Free For All) is
    /// selected.
    fn create_fill_types(&mut self) {
        let rows: Vec<(Vec<u8>, u32)> = self
            .create
            .templates
            .iter()
            .filter(|t| u16::from_le_bytes([t.rec[2], t.rec[3]]) == 1 && self.template_listed(&t.rec))
            .map(|t| (t.name.clone(), t.rec[0] as u32))
            .collect();
        for (name, gt) in rows {
            self.cadd(ids::GAME_TYPE, &name, 0, gt);
        }
        let restored = self.restore_game_type();
        let (count, _) = self.clist(ids::GAME_TYPE);
        if count == 0 {
            self.cenable(ids::GAME_TYPE, false);
            return;
        }
        if !self.setup.multiplayer && !restored {
            for i in (0..count).rev() {
                let gt = self.cdata(ids::GAME_TYPE, i).unwrap_or(0);
                if self.template_melee(gt as u8, (gt >> 8) as u8, 1) {
                    self.cselect(ids::GAME_TYPE, i);
                    break;
                }
            }
        }
        self.game_type_changed();
    }

    /// 0x4aeed0 (a game type was selected): remember it (0x59ba64), the subtype heading
    /// ("Resources:", ...), the subtypes, the game's template (0x596865) and, in single
    /// player, the slot rows; the selected map's information is shown again.
    fn game_type_changed(&mut self) {
        let Some(gt) = self.csel_data(ids::GAME_TYPE) else { return };
        self.create.cur = gt;
        let mut head = self.find_by_type(gt as u8, (gt >> 8) as u8).map(|t| t.desc.clone()).unwrap_or_default();
        if !head.is_empty() {
            head.extend_from_slice(b":");
        }
        let head = sstr(&head, 0x20);
        self.ctext(ids::SUB_LABEL, &head);
        self.create_fill_subtypes();
        let rec = self.find_template(gt as u8, (gt >> 8) as u8, self.create.sub as u16).map(|t| t.rec);
        if let Some(rec) = rec {
            self.setup.template = rec;
        }
        if !self.setup.multiplayer {
            self.create.slot_poll = true;
            self.create_slot_fill();
        }
        if let Some(n) = self.cnode() {
            self.info_queue(n);
        }
    }

    /// 0x4ab0b0: the first game type with (`gt`, byte 1).
    fn find_by_type(&self, gt: u8, b1: u8) -> Option<&Template> {
        self.create.templates.iter().find(|t| t.rec[0] == gt && t.rec[1] == b1)
    }

    /// 0x4ae790 (the subtype combo): a row per subtype of the game type (0x4aacc0 with
    /// 0x4ae1f0: its number as text, the subtype as data), the one of last time or the first
    /// (Top vs Bottom: the middle) selected; hidden with none.
    fn create_fill_subtypes(&mut self) {
        let (gt, b1) = (self.create.cur as u8, (self.create.cur >> 8) as u8);
        let players = self.create.players;
        if let Some(c) = self.cctl(ids::SUBTYPE) {
            c.list_clear();
        }
        let rows: Vec<(Vec<u8>, u32)> = self
            .create
            .templates
            .iter()
            .filter(|t| t.rec[0] == gt && t.rec[1] == b1)
            .filter(|t| {
                let n = u32::from_le_bytes(t.rec[4..8].try_into().unwrap());
                n != 0 && (t.rec[0] != 0xf || (players > 1 && n < players))
            })
            .map(|t| (sstr(&self.subtype_text(&t.rec, players), 0x10), u16::from_le_bytes([t.rec[2], t.rec[3]]) as u32))
            .collect();
        for (text, sub) in rows {
            self.cadd(ids::SUBTYPE, &text, 0x10, sub);
        }
        self.restore_subtype();
        let (count, _) = self.clist(ids::SUBTYPE);
        if count == 0 {
            self.cshow(ids::SUBTYPE, false);
            self.create.sub = 1;
            return;
        }
        if gt == 0xf {
            self.cselect(ids::SUBTYPE, count / 2);
        }
        self.cshow(ids::SUBTYPE, true);
        if let Some(d) = self.csel_data(ids::SUBTYPE) {
            self.create.sub = d;
        }
    }

    /// 0x4ad640: the subtype of last time ("Game Subtype") selected, else the first row.
    fn restore_subtype(&mut self) {
        let (count, _) = self.clist(ids::SUBTYPE);
        if count == 0 {
            return;
        }
        if let Some(v) = self.create.prefs.get("Game Subtype").cloned() {
            if self.find_text(ids::SUBTYPE, &v) != 0xff {
                return;
            }
        }
        let (count, _) = self.clist(ids::SUBTYPE);
        if count != 0 {
            self.cselect(ids::SUBTYPE, 0);
        }
    }

    /// 0x4adb10: the game type controls follow the selected node: a folder or map shows the
    /// game type heading and combo and enables the subtype combo.
    fn create_type_controls(&mut self) {
        let Some(idx) = self.cnode() else { return };
        let Some(info) = self.node_info(idx) else { return };
        if !info.loaded || info.flags & 0x80 == 0 {
            if self.cctl(ids::GAME_TYPE).is_some() {
                self.cshow(ids::GAME_TYPE, true);
                self.cenable(ids::GAME_TYPE, true);
            }
            self.cshow(ids::TYPE_LABEL, true);
            self.cenable(ids::SUBTYPE, true);
        } else {
            todo_port!("0x4adb10", "the game type controls for a replay's information");
        }
    }

    // ---- the slot rows (0x4adfe0, 0x4add90) ----

    /// 0x4adfe0 (the timer 5 and after a selection; single player only): one owner / race row
    /// per player of the map (Melee, Free For All) or per computer slot (Use Map Settings),
    /// the others hidden; a map still being read waits for the timer.
    fn create_slot_fill(&mut self) {
        if self.setup.multiplayer {
            return;
        }
        self.create.slot_timer = None;
        let (mut a, mut b) = (0u8, 0u8);
        if let Some(info) = self.cnode().and_then(|n| self.node_info(n)) {
            if info.flags & 0x80 == 0 {
                a = info.players;
                b = info.comps;
            }
        }
        if self.create.slot_poll && a == 0xff {
            self.create.slot_timer = Some(100);
            return;
        }
        if self.cctl(ids::OWNER_FIRST).is_none() || self.cctl(ids::RACE_FIRST).is_none() {
            return;
        }
        let mut next = ids::OWNER_FIRST;
        let mut melee = false;
        let (_, sel17) = self.clist(ids::GAME_TYPE);
        if a != 0 && sel17 != 0xff {
            let gt = self.cdata(ids::GAME_TYPE, sel17).unwrap_or(0);
            melee = self.template_melee(gt as u8, (gt >> 8) as u8, 1);
            self.cshow(ids::OWNER_FIRST, true);
            let user = if self.create.slot_poll {
                self.setup.open_slot_user_selectable() != 0
            } else {
                let i = self.setup.local_slot[0];
                // 0x4a8b90: slots past 7 have none.
                i < 8 && self.setup.user_selectable[i as usize] != 0
            };
            self.cshow(ids::RACE_FIRST, melee || user);
            // The other players, or with Use Map Settings only the computer slots.
            let mut left = if melee { a.wrapping_sub(1) } else { b };
            next = ids::OWNER_FIRST + 1;
            while left != 0 {
                if next >= ids::RACE_FIRST || self.cctl(next).is_none() {
                    break;
                }
                self.cshow(next, true);
                self.slot_state(melee, next);
                left = left.wrapping_sub(1);
                next += 1;
            }
        }
        while next < ids::RACE_FIRST && self.cctl(next).is_some() {
            self.cshow(next, false);
            self.slot_state(melee, next);
            next += 1;
        }
    }

    /// 0x4add90 (`all`: Melee / Free For All, else Use Map Settings; owner combo `oid`): with
    /// `all` the owner can be chosen; without, it is Computer and fixed. The race combo
    /// (id + 8) shows with its owner unless it is Closed (row data 0).
    fn slot_state(&mut self, all: bool, oid: i16) {
        let Some(c) = self.cctl(oid) else { return };
        let mut vis = all && c.flags & flag::VISIBLE != 0;
        let (count, sel) = self.clist(oid);
        if all {
            self.cenable(oid, true);
        } else {
            if sel != 0xff && self.cdata(oid, sel) != Some(1) && count != 0 {
                let rows: Vec<u32> = self.cctl(oid).and_then(|c| c.list.as_ref()).map(|l| l.items.iter().map(|i| i.2).collect()).unwrap_or_default();
                if let Some(r) = rows.iter().position(|&d| d == 1) {
                    self.cselect(oid, r as u8);
                }
            }
            self.cenable(oid, false);
        }
        let (count, sel) = self.clist(oid);
        if count != 0 && sel != 0xff {
            vis = vis && self.cdata(oid, sel).is_some_and(|d| d != 0);
        }
        self.cshow(oid + 8, vis);
    }

    // ---- events and timers ----

    /// Event 0xe code 0xb to list / combo `id` with `row` selected: 0x4af020 shows the node's
    /// information and fills the slot rows, 0x4af490 the game type (and remembers it, 0x4ad390
    /// on its code 1), 0x4ae710 the subtype (0x59ba5c), 0x4aed80 the owner row.
    pub(super) fn create_event(&mut self, cid: i16, _row: u8) {
        match cid {
            ids::LIST => {
                if let Some(n) = self.cnode() {
                    self.info_queue(n);
                }
                if !self.setup.multiplayer {
                    self.create.slot_poll = true;
                    self.create_slot_fill();
                }
            }
            ids::GAME_TYPE => {
                let key = if self.setup.multiplayer { "Game Type" } else { "Custom Type" };
                let text = self.cctl(cid).and_then(|c| c.list.as_ref()).and_then(|l| l.items.get(l.sel as usize)).map(|i| i.0.clone());
                if let Some(t) = text {
                    self.create.prefs.insert(key, t);
                }
                self.game_type_changed();
            }
            ids::SUBTYPE => {
                // 0x4ad400 (its code 1) remembers the text ("Game Subtype").
                let text = self.cctl(cid).and_then(|c| c.list.as_ref()).and_then(|l| l.items.get(l.sel as usize)).map(|i| i.0.clone());
                if let Some(t) = text {
                    self.create.prefs.insert("Game Subtype", t);
                }
                if let Some(d) = self.csel_data(cid) {
                    self.create.sub = d;
                }
            }
            ids::OWNER_FIRST..=ids::OWNER_LAST if cid != ids::OWNER_FIRST => self.slot_state(true, cid),
            _ => {}
        }
    }

    /// `ms` pass: the timers 0xc (0x4a7f50) and 5 (0x4adfe0).
    pub(super) fn create_tick(&mut self, files: &Storage, ms: u32) {
        if !matches!(self.dlg, Some((_, Open::Create(_)))) {
            return;
        }
        if let Some((left, idx)) = self.create.info_timer {
            if left > ms {
                self.create.info_timer = Some((left - ms, idx));
            } else {
                self.create.info_timer = None;
                self.info_timer_fired(files, idx);
            }
        }
        if let Some(left) = self.create.slot_timer {
            if left > ms {
                self.create.slot_timer = Some(left - ms);
            } else {
                self.create.slot_timer = None;
                self.create_slot_fill();
            }
        }
    }

    // ---- Ok (0x4af1b0, 0x4a8050, 0x4aee40, 0x4ad850) ----

    /// 0x4af5c0 event 0xe code 2 (control `cid` activated): Ok runs 0x4af1b0 and stays when it
    /// says so; everything else (Cancel) ends the dialog with its id (0x4dd040).
    pub(super) fn create_activate(&mut self, files: &Storage, cid: i16) {
        self.activated = cid;
        if cid == ids::OK && !self.create_ok(files) {
            return;
        }
        self.create.slot_timer = None;
        self.end_dialog(cid as i32);
    }

    /// 0x4af1b0: Ok with a map row selected: 0x4a8050 checks and starts it; a folder is entered
    /// (the list is made again), a refusal is a message (gluAll 0x33 .. 0x3c, 0x34 for the
    /// rest); done, the folder and row are kept for next time and (single player) 0x4aee40
    /// has the last word. False stays on the screen.
    fn create_ok(&mut self, files: &Storage) -> bool {
        let (count, sel) = self.clist(ids::LIST);
        if count == 0 || sel == 0xff {
            return false;
        }
        let Some(node) = self.cnode() else { return false };
        let packed = self.create.cur & 0xff | (self.create.cur >> 8 & 0xff) << 8 | (self.create.sub & 0xffff) << 16;
        let rc = self.start_check(files, node, packed);
        if rc != 0 {
            // 0x4af1b0's switch (0x80000001 + n): the message of each refusal.
            let msg = match rc {
                -0x7fff_ffff => Some(0x3c),
                -0x7fff_fffe => Some(0x3b),
                -0x7fff_fffb => Some(0x37),
                -0x7fff_fff9 => Some(0x33),
                -0x7fff_fff8 => Some(0x36),
                -0x7fff_fff5 => {
                    // A folder: 0x4ada80 shows its name, the row to select is forgotten,
                    // 0x4af100 lists it.
                    self.create_dir_label();
                    self.create.last_name.clear();
                    self.create_fill_list();
                    return false;
                }
                -0x7fff_fff4 => Some(0x3a),
                -0x7fff_fff3 => Some(0x39),
                // Ordinal 463 (SErrGetLastError) is not 0x4c6 (no network provider).
                _ => Some(0x34),
            };
            let text = self.glu(msg.unwrap_or(0x34));
            self.open_popup(files, &text);
            return false;
        }
        if self.create.which == 0 {
            // 0x59b850 / 0x59bc88: the folder (relative to the install) and the row's text.
            let (_, sel) = self.clist(ids::LIST);
            let text = self.cctl(ids::LIST).and_then(|c| c.list.as_ref()).and_then(|l| l.items.get(sel as usize)).map(|i| i.0.clone()).unwrap_or_default();
            self.create.saved_dir = self.create.dir.clone();
            self.create.saved_name = text;
        }
        if self.setup.multiplayer {
            return true;
        }
        self.create_finish(files)
    }

    /// 0x4a8050 on node `idx` (`packed`: game type, byte 1, subtype): 0 started, else the
    /// refusal. A folder becomes the current folder (-0x7ffffff5).
    fn start_check(&mut self, files: &Storage, idx: usize, packed: u32) -> i32 {
        self.create.info_timer = None;
        let Some(n) = self.create.nodes.get(idx) else { return -0x7fff_fffa };
        let (flags, loaded, state, path) = (n.flags, n.loaded, n.state, n.path.clone());
        match flags & 0x8f {
            8 => {
                self.create.dir = path;
                -0x7fff_fff5
            }
            4 => {
                let state = if loaded {
                    state
                } else {
                    self.map_info_load(files, idx);
                    self.create.nodes[idx].state
                };
                match state {
                    1 => return -0x7fff_fff9,
                    2 => return -0x7fff_ffff,
                    _ => {}
                }
                if loaded {
                    let net = self.network_raw.clone();
                    let mut hdr: MapHeader = [0; 0x20];
                    if !self.setup.load_map(files, &path, &mut hdr, false, &net) {
                        return if u16::from_le_bytes([hdr[0x18], hdr[0x19]]) > 0x3b { -0x7fff_ffff } else { -0x7fff_fff9 };
                    }
                }
                self.start_map(idx, packed)
            }
            1 => {
                todo_port!("0x4a7540", "start a saved game (0x4a68d0, 0x4d3910)");
                -0x7fff_fffa
            }
            0x80 => {
                todo_port!("0x4a7c30", "start a replay from the list");
                -0x7fff_fffa
            }
            _ => -0x7fff_fffa,
        }
    }

    /// 0x4a8050 case 4 after the map is read: the players / slots the game type needs, the
    /// game description (0x8d bytes), 0x4a68d0 (the game type and its settings), the local
    /// slot (0x4dbea0) and the game (0x4d3fc0).
    fn start_map(&mut self, idx: usize, packed: u32) -> i32 {
        let n = self.create.nodes[idx].clone();
        let (gt, b1, sub) = (packed as u8, (packed >> 8) as u8, (packed >> 16) as u16);
        if !self.setup.multiplayer {
            if !self.template_plain(gt, b1, sub) {
                if n.players < 2 {
                    return -0x7fff_fff3;
                }
            } else {
                if n.humans == 0 {
                    return -0x7fff_fff4;
                }
                if n.comps == 0 {
                    return -0x7fff_fff3;
                }
            }
        }
        let mut d = vec![0u8; 0x8d];
        let name = sstr(&self.setup.local_name, 0x18);
        d[4..4 + name.len()].copy_from_slice(&name);
        let title = sstr(&n.title, 0x20);
        d[0x4d..0x4d + title.len()].copy_from_slice(&title);
        // The size, 1, the players, the league, the game type and the tileset (the speed is
        // 0x4a68d0's).
        d[0x20..0x22].copy_from_slice(&n.width.to_le_bytes());
        d[0x22..0x24].copy_from_slice(&n.height.to_le_bytes());
        d[0x24] = 1;
        d[0x25] = n.players;
        d[0x27] = n.league as u8;
        d[0x28] = gt;
        d[0x29] = b1;
        d[0x2a..0x2c].copy_from_slice(&sub.to_le_bytes());
        d[0x30..0x32].copy_from_slice(&self.setup.tileset.to_le_bytes());
        let rc = self.check_template(&mut d);
        if rc != 0 {
            return rc;
        }
        // 0x4dbea0: 0x51ca1c = 1, 0x57f1e3 = 1, then 0x4dbe50.
        self.create.race_picked = true;
        if !self.setup.local_slot_setup() {
            return -0x7fff_fff9;
        }
        self.create.game_desc = d;
        todo_port!("0x4d3fc0", "create the single-player game from the description 0x5967f8 (SNetCreateGame, the slot / storm tables, 0x4a8d40, 0x4d3240)");
        todo_port!("0x4a7210", "put the map first in the recent maps list of the registry");
        0
    }

    /// 0x4a68d0 (`d` the description): the game type's settings must suit the game; the game
    /// speed or 6 is set in it; 0x80000008 or the refusal.
    fn check_template(&mut self, d: &mut [u8]) -> i32 {
        let (gt, b1, sub) = (d[0x28], d[0x29], u16::from_le_bytes([d[0x2a], d[0x2b]]));
        let Some(rec) = self.find_template(gt, b1, sub).map(|t| t.rec) else { return -0x7fff_fff8 };
        let cur = self.setup.template[0];
        if (0x11..=0x16).contains(&cur) && d[0x27] != 6 {
            return -0x7fff_ffee;
        }
        if rec[0x12] == 1 {
            d[0x26] = 6;
            if d[0x27] != 2 {
                return -0x7fff_fffd;
            }
        } else if rec[0] == 0x80 && rec[1] != 0 {
            d[0x26] = 6;
            if !self.expansion {
                return -0x7fff_ffef;
            }
            if d[0x27] != 5 {
                return -0x7fff_fff2;
            }
        } else {
            d[0x26] = self.create.speed;
        }
        if u16::from_le_bytes([d[0x30], d[0x31]]) == 2 && !self.template_plain(gt, b1, sub) {
            return -0x7fff_fffe;
        }
        // 0x4aaff0: the game type must need no more players than the map has.
        if rec[0x10] != 0 && rec[0x10] > d[0x25] {
            return -0x7fff_fff8;
        }
        // 0x6cdfd4 = the speed (the settings are not modelled here); the template follows.
        d[0x6d..0x8d].copy_from_slice(&rec);
        0
    }

    /// 0x4aee40 (after 0x4a8050 started the game): a map chosen (not a folder) must have a
    /// computer opponent (0x4ad460, else gluAll 0x39); 0x4ad850 then sets the races.
    fn create_finish(&mut self, files: &Storage) -> bool {
        self.create.slot_timer = None;
        let Some(idx) = self.cnode() else { return true };
        if self.node_info(idx).is_none_or(|i| i.flags == 8) {
            return true;
        }
        self.create.slot_poll = false;
        self.create_slot_fill();
        if !self.has_opponent() {
            // 0x6d0f14 (a replay) is 0.
            let text = self.glu(0x39);
            self.open_popup(files, &text);
            return false;
        }
        self.create_races();
        true
    }

    /// 0x4ad460: the game has a computer opponent: with Use Map Settings the map has
    /// computer slots (0x47a), else one of the shown owner rows is Computer.
    fn has_opponent(&mut self) -> bool {
        if self.cur_plain() {
            let Some(info) = self.cnode().and_then(|n| self.node_info(n)) else { return false };
            return info.flags == 4 && info.comps != 0 && info.comps != 0xff;
        }
        for oid in ids::OWNER_FIRST + 1..=ids::OWNER_LAST {
            let Some(c) = self.cctl(oid) else { return false };
            let visible = c.flags & flag::VISIBLE != 0;
            let (count, sel) = self.clist(oid);
            if visible && count != 0 && sel != 0xff && self.cdata(oid, sel) == Some(1) {
                return true;
            }
        }
        false
    }

    /// 0x4ad850: the races of the rows. Free For All (a game type with +8 = 0): the player's
    /// race (0x4a9d50). Melee / Free For All (+8 = 1): every shown row's race for the game
    /// start (0x57f266 ..).
    fn create_races(&mut self) {
        let melee = {
            let c = &self.create;
            self.template_melee(c.cur as u8, (c.cur >> 8) as u8, c.sub as u16)
        };
        if !melee {
            if self.cctl(ids::RACE_FIRST).is_none_or(|c| c.flags & flag::VISIBLE == 0) {
                return;
            }
            let race = self.csel_data(ids::RACE_FIRST);
            let i = self.setup.local_slot[0] as usize % 12;
            self.setup.slots[i][slot::RACE] = race.map_or(6, |r| r as u8);
            self.create.rand_on = true;
            self.assign_races();
            self.create.rand_on = false;
            return;
        }
        self.create.race_pick = [7; 9];
        self.create.race_picked = true;
        if let Some(r) = self.csel_data(ids::RACE_FIRST) {
            self.create.race_pick[0] = r as u8;
        }
        let mut k = 1;
        for rid in ids::RACE_FIRST + 1..=ids::RACE_LAST {
            let Some(c) = self.cctl(rid) else { break };
            if c.flags & flag::VISIBLE != 0 {
                if let Some(r) = self.csel_data(rid) {
                    self.create.race_pick[k] = r as u8;
                    k += 1;
                }
            }
        }
    }

    /// 0x4dc550 (EDX = `lo`, `hi`): a number in `lo..=hi`; with 0x6d11c8 off `lo`.
    fn create_rand(&mut self, lo: i32, hi: i32) -> i32 {
        if !self.create.rand_on {
            return lo;
        }
        let r = self.create_lcg();
        (((hi - lo + 1) * r as i32) >> 15) + lo
    }

    /// The random number generator 0x51ca14 (the next 15 bits).
    fn create_lcg(&mut self) -> u32 {
        self.create.seed = self.create.seed.wrapping_mul(0x015a_4e35).wrapping_add(1);
        self.create.seed >> 16 & 0x7fff
    }

    /// 0x4a94c0 (`total` the random slots, `drawn` the ones dealt): a race from the bag of
    /// three (each comes up once before one repeats).
    fn deal_race(&mut self, total: u32, drawn: u32) -> u32 {
        let left = self.create.bag_left;
        if left != 1 {
            let mut r = self.create_rand(0, left as i32 - 1) as usize;
            if left == 2 {
                if r == 0 && self.create.bag[0] != 0 {
                    r = 1;
                } else if r == 1 && self.create.bag[2] == 0 {
                    r = 2;
                }
                if drawn == 1 && total == 2 && self.create_rand(0, 7) == 4 {
                    r = self.create_rand(0, 2) as usize;
                }
            }
            self.create.bag[r] = 1;
            self.create.bag_left = left - 1;
            return r as u32;
        }
        let r = if self.create.bag[0] == 0 { 0 } else { (self.create.bag[1] != 0) as u32 + 1 };
        self.create.bag = [0; 3];
        self.create.bag_left = 3;
        let again = if drawn == 2 { total == 3 } else { drawn == 5 && total == 6 };
        if again {
            return self.create_rand(0, 2) as u32;
        }
        r
    }

    /// 0x4a9a30: the slots (0x57eee0) with the random race (6) get one: a human or computer
    /// slot's from the bag of 0x4a94c0, the others' at random; a team game's teammates
    /// share it; the dealt races are then shuffled among the slots (0x6d11c8 on).
    fn assign_races(&mut self) {
        self.create.bag = [0; 3];
        self.create.bag_left = 3;
        let random = |s: &[u8; 0x24]| s[slot::RACE] == 6 && (s[slot::TYPE] == 2 || s[slot::TYPE] == 5);
        let total = self.setup.slots[..8].iter().filter(|s| random(s)).count() as u32;
        let mut dealt: Vec<u8> = Vec::new();
        for i in (0..8).rev() {
            if self.setup.slots[i][slot::RACE] != 6 {
                continue;
            }
            let ty = self.setup.slots[i][slot::TYPE];
            if ty == 2 || ty == 5 {
                let drawn = dealt.len() as u32;
                dealt.push(self.deal_race(total, drawn) as u8);
            } else {
                self.setup.slots[i][slot::RACE] = self.create_rand(0, 2) as u8;
            }
            if self.setup.multiplayer && self.setup.template[0x10] != 0 && (ty == 1 || ty == 5) {
                let (force, race) = (self.setup.slots[i][slot::FORCE], self.setup.slots[i][slot::RACE]);
                for s in self.setup.slots[..8].iter_mut().filter(|s| s[slot::FORCE] == force) {
                    s[slot::RACE] = race;
                }
            }
        }
        // The shuffle (16 swaps with the generator, none when 0x6d11c8 is off).
        if total != 0 {
            for _ in 0..16 {
                let a = if self.create.rand_on { self.create_lcg() } else { 0 };
                let a = ((total * a) >> 15) as usize;
                let b = if self.create.rand_on { self.create_lcg() } else { 0 };
                let b = ((total * b) >> 15) as usize;
                dealt.swap(a, b);
            }
        }
        let mut it = dealt.into_iter();
        for i in (0..8).rev() {
            if random(&self.setup.slots[i]) {
                if let Some(r) = it.next() {
                    self.setup.slots[i][slot::RACE] = r;
                }
            }
        }
    }
}
