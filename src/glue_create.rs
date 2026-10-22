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
use crate::setup::{slot, sstr_copy, MapHeader, Setup, Slot};
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
    /// +0x48c: the game description (0x8d bytes) of a saved game / replay (the +0x480
    /// header's first 0xc bytes are not kept).
    info: Vec<u8>,
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
    pub(super) speed: u8,
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
    /// The replay file started from the list (0x6d0f24 holds its data natively); the game
    /// start has to play it.
    pub(super) replay: Option<Vec<u8>>,
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
            replay: None,
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

/// 0x41f1b0 with one "%s": the format `f` with `arg` (up to its nul) put in.
fn fmt_s(f: &[u8], arg: &[u8]) -> Vec<u8> {
    let arg = arg.split(|&c| c == 0).next().unwrap_or(&[]);
    match f.windows(2).position(|w| w == b"%s") {
        Some(p) => [&f[..p], arg, &f[p + 2..]].concat(),
        None => f.to_vec(),
    }
}

/// Ordinal 503 (SStrNCat): `src` (up to its nul) appended to `dst` within `max` bytes.
fn cat(dst: &mut Vec<u8>, src: &[u8], max: usize) {
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let room = (max - 1).saturating_sub(dst.len());
    dst.extend_from_slice(&src[..src.len().min(room)]);
}

/// 0x4cfa10: the game description (0x8d bytes) and the name (the file name without its
/// extension, under 0x1c characters) of the saved game `path`; Err(1) when it cannot be read
/// (0x4ce470 text up to 0x1a, 0x4cf0f0 header), Err(2) when it is no save of this game.
fn save_head_info(path: &str) -> Result<(Vec<u8>, Vec<u8>), u8> {
    let f = std::fs::read(crate::saveload::host(path)).map_err(|_| 1u8)?;
    let mut i = crate::bw::save::In::new(&f);
    loop {
        match i.bytes(1) {
            None => return Err(1),
            Some(b) if b[0] == 0x1a => break,
            _ => {}
        }
    }
    let h = crate::saveload::read_header(&mut i).ok_or(1u8)?;
    if h.id != 0x69 {
        return Err(2);
    }
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let mut name = sstr(file.as_bytes(), 0x20);
    if let Some(dot) = name.iter().rposition(|&c| c == b'.') {
        name.truncate(dot);
    }
    if name.len() >= 0x1c {
        return Err(1);
    }
    Ok((h.info.to_vec(), name))
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

    /// 0x4af5c0 events 0x11 / 0x12 (the mouse wheel up / down): event 0xe code 0xf / 0x10 to the
    /// map list's scroll bar (0x59bc7c + 0x36, 0x415af0), the list moves a row.
    pub(super) fn create_wheel(&mut self, down: bool) {
        if self.popup.is_some() {
            return;
        }
        if let Some(c) = self.cctl(ids::LIST) {
            c.list_scroll_wheel(down);
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

    /// 0x4aae20: the next quoted name of the list, 0 (stop) when unterminated (inline here).
    /// 0x4ab860 with 0x4ab150: the game types of Templates\templates.lst, a node per quoted
    /// file name (0x4aae20) whose .got 0x4ab750 accepts.
    pub(super) fn load_templates(&mut self, files: &Storage) {
        if !self.create.templates.is_empty() {
            return;
        }
        let Ok(list) = files.read("Templates\\templates.lst") else { crate::bw::fatal_error("Unable to read game templates.") };
        let mut rest = &list[..];
        // 0x4aae20: the next quoted name (under 0x104 bytes); it stops at an unterminated or
        // too long one.
        while let Some(open) = rest.iter().position(|&c| c == b'"') {
            let after = &rest[open + 1..];
            let Some(len) = after.iter().position(|&c| c == b'"').filter(|&l| l < 0x104) else { break };
            let name = String::from_utf8_lossy(&after[..len]).into_owned();
            rest = &after[len + 1..];
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
    /// controls have slid in (0x4dd5e0, `create_405`) 0x405 to the combos and the
    /// dialog.
    /// 0x4adac0: the create screen's slide-in (list 0x51a9f0) and the folder label proc (inline here).
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
        // 0x4adac0: the buttons slide in (0x4dd5e0 over list 0x51a9f0, AX = 5) and
        // the folder label gets its draw 0x4ad980.
        self.slide_in(0x51a9f0, 5, 0);
        if self.setup.multiplayer {
            todo_port!("0x4ae860", "the network screen's game speed slider (id 16, 0x4ae250 / 0x4ae3c0) and game name");
        }
        if let Some(c) = self.cctl(ids::DIR) {
            c.update = Update::DirLabel;
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
    }

    /// Event 0x405 once the controls have slid in: the combos (17: 0x4af400), then the dialog
    /// (0x4af100).
    pub(super) fn create_405(&mut self) {
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
            self.save_info_load(idx);
        } else if flags & 0x80 != 0 {
            self.replay_info_load(files, idx);
        }
        self.info_apply(idx);
    }

    /// 0x4a8cd0: the count of the slots of type 6 (inline here).
    /// 0x4a8d10: the count of the lobby slots of type 1 or 5 (inline here).
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

    /// 0x4a7540: read the saved game node `idx` (once): 0x4cfa10 reads its header, the texts
    /// are the game's title, "\n\n" and the save's name; failures put a message into the
    /// description (state 1 stays).
    fn save_info_load(&mut self, idx: usize) {
        let Some(n) = self.create.nodes.get(idx) else { return };
        if n.loaded {
            return;
        }
        let (path, name_len, path_len, name) = (n.path.clone(), n.name.len(), n.path.len(), n.name.clone());
        let title = sstr(self.glu(0x2f).as_bytes(), 0x20);
        {
            let n = &mut self.create.nodes[idx];
            n.loaded = true;
            n.state = 1;
            n.title = title;
        }
        let msg = if name_len >= 0x20 || path_len + 1 + name_len >= 0x105 {
            0x41
        } else {
            match save_head_info(&path) {
                Ok((info, save_name)) => {
                    let (w, h) = (u16::from_le_bytes([info[0x20], info[0x21]]), u16::from_le_bytes([info[0x22], info[0x23]]));
                    let tile = u16::from_le_bytes([info[0x30], info[0x31]]);
                    let mut desc = fmt_s(self.glu(0x30).as_bytes(), &info[0x4d..0x6d]);
                    cat(&mut desc, b"\n\n", 0x100);
                    cat(&mut desc, &save_name, 0x100);
                    let players_s = self.glu(0x1e).replacen("%u", &info[0x24].to_string(), 1).into_bytes();
                    let size_s = self.glu(0x20).replacen("%u", &w.to_string(), 1).replacen("%u", &h.to_string(), 1).into_bytes();
                    let tileset_s = sstr(self.glu(tile as usize + 0x27).as_bytes(), 0x23);
                    let n = &mut self.create.nodes[idx];
                    n.disp = sstr(&name, 0x41);
                    n.players = info[0x25];
                    n.width = w;
                    n.height = h;
                    n.desc = sstr(&desc, 0x100);
                    n.players_s = players_s;
                    n.size_s = size_s;
                    n.tileset_s = tileset_s;
                    n.info = info;
                    n.state = 0;
                    return;
                }
                Err(e) => 0x41 + e as usize,
            }
        };
        self.create.nodes[idx].desc = sstr(self.glu(msg).as_bytes(), 0x100);
    }

    /// 0x4a7c30 (with 0x4df570 and 0x4de9d0): read the replay node `idx` (once) with
    /// 0x6d0f14 = 1: the header's game description and slots, the map header from the
    /// replay's embedded scenario (0x4bf5d0 with the mission mode), the texts.
    fn replay_info_load(&mut self, files: &Storage, idx: usize) {
        let net = self.network_raw.clone();
        let Some(n) = self.create.nodes.get(idx) else { return };
        if n.loaded {
            return;
        }
        let (path, name_len, path_len) = (n.path.clone(), n.name.len(), n.path.len());
        let title = sstr(self.glu(0xb2).as_bytes(), 0x20);
        {
            let n = &mut self.create.nodes[idx];
            n.loaded = true;
            n.state = 1;
            n.title = title;
        }
        if name_len >= 0x20 || path_len + 1 + name_len >= 0x105 {
            self.create.nodes[idx].desc = sstr(self.glu(0x3e).as_bytes(), 0x100);
            return;
        }
        // 0x4df570: the file must be a 1.16 replay ("reRS") with a map; a Brood War replay
        // needs the expansion. The command buffers (0x4ce280, 0x4ce220) are the replay
        // player's.
        let mut bw = false;
        let rep = std::fs::read(crate::saveload::host(&path)).ok().and_then(|f| crate::replay::Replay::parse(&f).ok()).filter(|r| {
            let raw = &r.header.raw;
            if r.modern || raw.len() < 0x279 || raw[0x46] == 0 {
                return false;
            }
            bw = raw[0] != 0;
            !bw || self.expansion
        });
        let Some(r) = rep else {
            let msg = if bw { 0x3c } else { 0x3f };
            let d = sstr(self.glu(msg).as_bytes(), 0x100);
            let n = &mut self.create.nodes[idx];
            n.state = 2;
            n.desc = d;
            return;
        };
        let raw = &r.header.raw;
        // 0x4de9d0: the game description (0x6d0f44) and the 12 slots (0x6d0fd1).
        let info = raw[0x14..0x14 + 0x8d].to_vec();
        let slots = &raw[0xa1..0xa1 + 12 * 0x24];
        let comps = slots.chunks(0x24).filter(|s| s[0] == 1).count() as u8;
        let humans = slots.chunks(0x24).filter(|s| s[0] == 2).count() as u8;
        let (w, h) = (u16::from_le_bytes([info[0x20], info[0x21]]), u16::from_le_bytes([info[0x22], info[0x23]]));
        let tile = u16::from_le_bytes([info[0x30], info[0x31]]);
        let campaign = u16::from_le_bytes([raw[5], raw[6]]);
        let mut hdr: MapHeader = [0; 0x20];
        let ok = if campaign == 0 {
            // 0x6d0f14 is the mission flag: the map is the replay's scenario.
            let saved = (self.setup.mission, std::mem::replace(&mut self.setup.mission_chk, r.chk.clone()));
            self.setup.mission = true;
            let ok = self.setup.load_map(files, &path, &mut hdr, false, &net);
            (self.setup.mission, self.setup.mission_chk) = saved;
            ok
        } else {
            todo_port!("0x4bf5d0", "the map header of a campaign mission replay (0x59c080)");
            false
        };
        if !ok {
            let old = u16::from_le_bytes([hdr[0x18], hdr[0x19]]) < 0x3c;
            let d = sstr(self.glu(if old { 0x3f } else { 0x3c }).as_bytes(), 0x100);
            let n = &mut self.create.nodes[idx];
            if !old {
                n.state = 2;
            }
            n.desc = d;
            return;
        }
        let mut desc = fmt_s(self.glu(0x30).as_bytes(), &info[0x4d..0x6d]);
        cat(&mut desc, b"\n\n", 0x100);
        let players_s = self.glu(0x1e).replacen("%u", "1", 1).into_bytes();
        let size_s = self.glu(0x20).replacen("%u", &w.to_string(), 1).replacen("%u", &h.to_string(), 1).into_bytes();
        let comps_s = self.glu(0x23).replacen("%u", &comps.to_string(), 1).into_bytes();
        let humans_s = self.glu(0x25).replacen("%u", &humans.to_string(), 1).into_bytes();
        let tileset_s = sstr(self.glu(tile as usize + 0x27).as_bytes(), 0x23);
        let n = &mut self.create.nodes[idx];
        n.disp = sstr(&n.name, 0x41);
        n.humans = humans;
        n.width = w;
        n.height = h;
        n.comps = comps;
        n.players = 8;
        n.desc = sstr(&desc, 0x100);
        n.players_s = players_s;
        n.size_s = size_s;
        n.comps_s = comps_s;
        n.humans_s = humans_s;
        n.tileset_s = tileset_s;
        n.info = info;
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

    /// 0x4aad00: the game types with subtype 1, in list order (inline here).
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

    /// 0x4ab0b0 / 0x4ab0e0: the first game type with (`gt`, byte 1): its record (+0x28) /
    /// its name (+8).
    fn find_by_type(&self, gt: u8, b1: u8) -> Option<&Template> {
        self.create.templates.iter().find(|t| t.rec[0] == gt && t.rec[1] == b1)
    }

    /// 0x4aacc0: the game types with (`gt`, byte 1), in list order (inline here).
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
    /// 0x4a8b90: a slot's user selectable flag, 0 past slot 7 (inline here).
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
                if !loaded {
                    self.save_info_load(idx);
                }
                match self.create.nodes[idx].state {
                    1 => return -0x7fff_fff9,
                    2 => return -0x7fff_ffff,
                    _ => {}
                }
                let mut d = self.create.nodes[idx].info.clone();
                d.resize(0x8d, 0);
                let name = sstr(&self.setup.local_name, 0x18);
                d[4..0x1c].fill(0);
                d[4..4 + name.len()].copy_from_slice(&name);
                todo_port!("0x4d3910", "create the network game of the saved game's description");
                -0x7fff_fffa
            }
            0x80 => {
                // 0x6d0f14 = 1 (the replay's map comes from memory) stays set.
                self.create.nodes[idx].loaded = false;
                self.replay_info_load(files, idx);
                match self.create.nodes[idx].state {
                    1 => return -0x7fff_fff9,
                    2 => return -0x7fff_ffff,
                    _ => {}
                }
                let mut d = self.create.nodes[idx].info.clone();
                d.resize(0x8d, 0);
                let name = sstr(&self.setup.local_name, 0x18);
                d[4..0x1c].fill(0);
                d[4..4 + name.len()].copy_from_slice(&name);
                if self.setup.multiplayer {
                    todo_port!("0x4d3910", "create the network game of the replay's description");
                    return -0x7fff_fffa;
                }
                if !self.setup.local_slot_setup() {
                    return -0x7fff_fff9;
                }
                if !self.create_game(&d) {
                    return -0x7fff_fffa;
                }
                self.create.replay = std::fs::read(crate::saveload::host(&path)).ok();
                0
            }
            _ => -0x7fff_fffa,
        }
    }

    /// 0x4a8050 case 4 after the map is read: the players / slots the game type needs, the
    /// game description (0x8d bytes), 0x4a68d0 (the game type and its settings), the local
    /// slot (0x4dbea0) and the game (0x4d3fc0).
    /// 0x4dbea0: 0x51ca1c = 1, 0x57f1e3 = 1, then 0x4dbe50 (inline here).
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
        // 0x4dbea0 (inline here): 0x51ca1c = 1, 0x57f1e3 = 1, then 0x4dbe50.
        self.create.race_picked = true;
        if !self.setup.local_slot_setup() {
            return -0x7fff_fff9;
        }
        self.create.game_desc = d.clone();
        if !self.create_game(&d) {
            return -0x7fff_fff9;
        }
        todo_port!("0x4a7210", "put the map first in the recent maps list of the registry");
        0
    }

    /// 0x4a68d0 (`d` the description): the game type's settings must suit the game; the game
    /// speed or 6 is set in it; 0x80000008 or the refusal.
    pub(super) fn check_template(&mut self, d: &mut [u8]) -> i32 {
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

    /// 0x4ad560: a row is selected and it is not a folder (flags 8; inline here).
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

    /// 0x4a9d50: the race of the local slot set and dealt with 0x6d11c8 on (inline here).
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

    /// 0x4d3fc0 (EAX = the description `d`, 0x8d bytes), single-player path: start the game
    /// 0x59688c = provider, the (here: one) player's storm id, the description (0x5967f8) and
    /// the storm id -> slot tables and per-game tables. The Storm calls (the provider setup
    /// 0x4d3cc0, SNetCreateGame Ordinal 101) are not ported: networking is our own; the
    /// local player's storm id is 0.
    pub(super) fn create_game(&mut self, d: &[u8]) -> bool {
        // 0x4d3cc0 (provider setup: Ordinal 117 / 114 / 123 callbacks 0x4c4980, 0x4c3c90,
        // 0x47f750, 0x4aaea0): a single-player game has provider 0 (0x59688c).
        todo_port!("0x4d3cc0", "Storm provider setup and message callbacks for the game");
        self.setup.net_provider = 0;
        // Ordinal 101 (SNetCreateGame) returns the local storm id in 0x51268c.
        let (me, id) = (self.setup.local_slot[0], 0u32);
        self.setup.local_storm_id = id;
        self.setup.slot_by_storm[id as usize] = me;
        self.setup.slot_by_storm2[id as usize] = self.setup.local_slot[1];
        if let Some(s) = self.setup.slots.get_mut(me as usize) {
            s[slot::STORM..slot::STORM + 4].copy_from_slice(&id.to_le_bytes());
        }
        self.create.game_desc = d.to_vec();
        // 0x5967f8 + 0x6d (0x596865) is the game type record.
        self.setup.template.copy_from_slice(&d[0x6d..0x8d]);
        // 0x596888 = 1 (the description is in place)
        self.setup.index_human_slots();
        self.setup.reset_game_tables();
        true
    }

    /// 0x4dbbe0: the computer slots (5) of slots 7..0 become open (6).
    fn race_computers_open(&mut self) {
        for s in self.setup.slots[..8].iter_mut().rev() {
            if s[slot::TYPE] == 5 {
                s[slot::TYPE] = 6;
            }
        }
    }

    /// 0x4dbb70 (`race`): the first open slot (6) from slot 7 down becomes a computer (5) of
    /// that race. False when there is none.
    fn race_computer_add(&mut self, race: u8) -> bool {
        for s in self.setup.slots[..8].iter_mut().rev() {
            if s[slot::TYPE] == 6 {
                s[slot::TYPE] = 5;
                s[slot::RACE] = race;
                return true;
            }
        }
        false
    }

    /// 0x4ee110: the races picked in the create screen (0x57f266 ..) go to the slots of a
    /// melee-type game: the computers become open, the local slot gets the first, and
    /// open slots become computers of the others.
    fn race_picks_apply(&mut self) {
        let t = &self.setup.template;
        if self.setup.multiplayer || !self.create.race_picked || (t[8] == 0 && t[0xc] == 0 && t[0x12] == 0) {
            return;
        }
        self.race_computers_open();
        let me = self.setup.local_slot[0] as usize;
        self.setup.slots[me][slot::RACE] = self.create.race_pick[0];
        for k in 1..9 {
            let r = self.create.race_pick[k];
            if r != 7 {
                self.race_computer_add(r);
            }
        }
    }

    /// 0x4a9d80 (the part a single-player game takes; `rand_on` is 0x6d11c8): the slot types
    /// (computer 5 -> 1, open 6 / 8 -> 0), then for the melee game types (template 0x59686d /
    /// 0x596871 / 0x596877) the occupied slots are put on random free start slots, unless
    /// 0x596872. Returns what 0x49b360 takes.
    fn place_players(&mut self) -> bool {
        for s in self.setup.slots[..12].iter_mut().rev() {
            match s[slot::TYPE] {
                5 => s[slot::TYPE] = 1,
                6 | 8 => s[slot::TYPE] = 0,
                _ => {}
            }
        }
        let t = self.setup.template;
        if self.setup.multiplayer && t[0x10] != 0 {
            self.team_places();
            return false;
        }
        if t[8] == 0 && t[0xc] == 0 && t[0x12] == 0 {
            return self.place_forces();
        }
        if t[0xd] != 0 {
            self.setup.index_human_slots();
            return false;
        }
        self.shuffle_players([true; 8]);
        // 0x49b360 gets [ebp-0x18], still 0 on this path.
        false
    }

    /// 0x4a9250 (0x4ef100 with a saved game, 0x6d1218 set, instead of 0x4a9d80; a network
    /// game only): the slot types (5 -> 1, 6 / 8 -> 0), the occupied slots (2, 1, 3, 4, 7)
    /// moved to the slots 0x66ff34 gives, the storm ids of the non-human ones cleared and the
    /// indexes renumbered; with teams (0x596875) each force's leader is swapped to the slot
    /// 0x57f1cc names (when that is not human) and the storm tables follow the leaders.
    pub(super) fn rebuild_slots(&mut self) {
        let n = &mut self.setup;
        if !n.multiplayer {
            return;
        }
        for s in n.slots.iter_mut().rev() {
            match s[slot::TYPE] {
                5 => s[slot::TYPE] = 1,
                6 | 8 => s[slot::TYPE] = 0,
                _ => {}
            }
        }
        let old: [Slot; 8] = std::array::from_fn(|i| n.slots[i]);
        n.slots = [[0; 0x24]; 12];
        for i in (0..8).rev() {
            if matches!(old[i][slot::TYPE], 2 | 1 | 3 | 4 | 7) {
                n.slots[n.slot_order[i] as usize] = old[i];
            }
        }
        for k in (0..12).rev() {
            if n.slots[k][slot::TYPE] != 2 {
                n.slots[k][slot::STORM..slot::STORM + 4].fill(0xff);
            }
            n.slots[k][..4].copy_from_slice(&(k as u32).to_le_bytes());
        }
        n.index_human_slots();
        if n.template[0x10] == 0 {
            return;
        }
        for f in 1..=4u8 {
            // 0x484df0 over the slots 7..0: a computer / open slot (1, 5) of the force.
            let has = |n: &Setup, tys: &[u8]| (0..8).any(|i| n.slots[i][slot::FORCE] == f && tys.contains(&n.slots[i][slot::TYPE]));
            if has(n, &[1, 5]) || !has(n, &[2]) {
                // with one the force is left (0x484df0 != 0); with no human slot it is skipped.
                continue;
            }
            let l = n.team_leaders[f as usize] as usize;
            if l < 12 && n.slots[l][slot::TYPE] != 2 {
                let u = n.force_leader(f) as usize;
                if u < 12 {
                    n.slots.swap(l, u);
                    n.slots[u][..4].copy_from_slice(&(u as u32).to_le_bytes());
                    n.slots[l][..4].copy_from_slice(&(l as u32).to_le_bytes());
                }
            }
        }
        n.index_human_slots();
        let me = n.local_slot[0] as usize;
        if n.slots.get(me).is_some_and(|s| s[slot::TYPE] == 2) {
            n.local_slot[0] = n.team_leaders.get(n.slots[me][slot::FORCE] as usize).copied().unwrap_or(0) as u32;
        }
        let by = n.slot_by_storm2;
        n.slot_by_storm = [8; 8];
        for j in 0..8 {
            let s = &n.slots[by[j] as usize];
            if s[slot::TYPE] == 2 {
                n.slot_by_storm[j] = n.team_leaders.get(s[slot::FORCE] as usize).copied().unwrap_or(0) as u32;
            }
        }
    }

    /// 0x485160: team game setup. The lobby copy takes the types 0x66ff3c; the leader of each
    /// force (0x484f70) and the human / computer / reserved slots are lifted out of the slot
    /// table, which is restored from the lobby copy; the leaders go to random start slots
    /// of types 6 / 5 / 3 / 4 / 7 (the first with 0x596872), the others to the first free one;
    /// then the storm id tables follow the leaders (0x57f1cb).
    fn team_places(&mut self) {
        let n = &mut self.setup;
        for k in 0..12 {
            n.lobby[k][slot::TYPE] = n.net_types[k];
        }
        let leads: Vec<u8> = (0..4u8).map(|f| n.force_leader(f + 1)).collect();
        let mut lifted: Vec<Slot> = Vec::new();
        for &l in leads.iter().filter(|&&l| l != 0xff) {
            lifted.push(n.slots[l as usize]);
            n.slots[l as usize] = n.lobby[l as usize];
        }
        let mut rest: Vec<Slot> = Vec::new();
        for s in n.slots[..8].iter_mut() {
            match s[slot::TYPE] {
                1 => s[slot::TYPE] = 0,
                2 | 3 | 4 | 7 => rest.push(*s),
                _ => {}
            }
        }
        n.team_leaders[1..].fill(0xff);
        n.slots = n.lobby;
        for s in lifted {
            let list: Vec<u8> = (0..8u8).filter(|&i| matches!(self.setup.slots[i as usize][slot::TYPE], 3 | 4 | 5 | 6 | 7)).collect();
            let p = if self.setup.template[0xd] == 0 && self.create.rand_on {
                // 0x51c690 / 0x51ca18 (counters nothing reads) are bumped too.
                let r = self.create_lcg();
                list.get((r as usize * list.len()) >> 15).copied().unwrap_or(0)
            } else {
                list.first().copied().unwrap_or(0)
            };
            let n = &mut self.setup;
            n.slots[p as usize] = s;
            n.slots[p as usize][..4].copy_from_slice(&(p as u32).to_le_bytes());
            let f = s[slot::FORCE] as usize;
            if let Some(l) = n.team_leaders.get_mut(f) {
                *l = p;
            }
        }
        let n = &mut self.setup;
        for s in rest {
            let p = (0..8).find(|&i| !matches!(n.slots[i][slot::TYPE], 1 | 2 | 3 | 4 | 7)).unwrap_or(0);
            n.slots[p] = s;
            n.slots[p][..4].copy_from_slice(&(p as u32).to_le_bytes());
        }
        n.slot_by_storm = [8; 8];
        let mut local = n.local_slot[0];
        for i in (0..8).rev() {
            let st = u32::from_le_bytes(n.slots[i][slot::STORM..slot::STORM + 4].try_into().unwrap());
            if n.slots[i][slot::TYPE] == 2 {
                if let Some(e) = n.slot_by_storm.get_mut(st as usize) {
                    *e = i as u32;
                }
            }
            if st == n.local_storm_id {
                local = i as u32;
            }
        }
        n.slot_by_storm2 = n.slot_by_storm;
        n.local_slot = [local; 2];
        for s in n.slots[..8].iter_mut() {
            if !matches!(s[slot::TYPE], 1 | 2 | 3 | 4 | 7) {
                s[slot::TYPE] = 0;
            }
        }
        let me = n.slots[local as usize];
        if me[slot::TYPE] == 2 {
            n.local_slot[0] = n.team_leaders.get(me[slot::FORCE] as usize).copied().unwrap_or(0) as u32;
        }
        n.slot_by_storm = [8; 8];
        for j in (0..8).rev() {
            let s = n.slot_by_storm2[j];
            let c = n.slots[s as usize];
            if c[slot::TYPE] == 2 {
                n.slot_by_storm[j] = n.team_leaders.get(c[slot::FORCE] as usize).copied().unwrap_or(0) as u32;
            }
            if c[slot::TYPE] == 9 {
                n.slot_by_storm[j] = s;
            }
        }
    }

    /// 0x4a9c90: Use Map Settings: per force (1..4) with its flag set, the slots of the force
    /// that are human / computer / open (2, 1, 6, 5) go to the list of the "user selectable
    /// race" slots or to the other; both lists are shuffled (0x4a95d0), the first first.
    /// True when any slot was in such a force.
    fn place_forces(&mut self) -> bool {
        let mut any = false;
        for force in 1..=4u8 {
            if self.setup.force_flags[force as usize - 1] & 1 == 0 {
                continue;
            }
            let (mut sel, mut other) = ([false; 8], [false; 8]);
            for j in (0..8).rev() {
                if self.setup.slots[j][slot::FORCE] == force && matches!(self.setup.lobby[j][slot::TYPE], 2 | 1 | 6 | 5) {
                    any = true;
                    if self.setup.user_selectable[j] != 0 { sel[j] = true } else { other[j] = true }
                }
            }
            self.shuffle_players(sel);
            self.shuffle_players(other);
        }
        any
    }

    /// 0x4a95d0 (`list` = eax, a non-null list; 0x4a9d80 has the same code inline for the
    /// all-1 list): the occupied slots (2, 1, 3, 4, 7) in `list` are taken out, the list's
    /// slots restored from the lobby copy, and each taken slot (last first) put on a random
    /// free start slot (6, 5, 3, 4, 7) of the list; slots left not occupied become open.
    fn shuffle_players(&mut self, list: [bool; 8]) {
        let occupied = |ty: u8| matches!(ty, 2 | 1 | 3 | 4 | 7);
        let taken: Vec<_> = (0..8).filter(|&i| list[i] && occupied(self.setup.slots[i][slot::TYPE])).map(|i| self.setup.slots[i]).collect();
        for i in (0..8).filter(|&i| list[i]) {
            self.setup.slots[i] = self.setup.lobby[i];
        }
        for entry in taken.iter().rev() {
            let cand: Vec<usize> = (0..8).filter(|&i| list[i] && matches!(self.setup.slots[i][slot::TYPE], 6 | 5 | 3 | 4 | 7)).collect();
            let r = self.create_lcg_if_on();
            let pick = cand.get(((r * cand.len() as u32) >> 15) as usize).copied().unwrap_or(0);
            self.setup.slots[pick] = *entry;
            self.setup.slots[pick][slot::INDEX..slot::INDEX + 4].copy_from_slice(&(pick as u32).to_le_bytes());
        }
        for s in self.setup.slots[..8].iter_mut() {
            if !occupied(s[slot::TYPE]) {
                s[slot::TYPE] = 0;
            }
        }
        self.setup.index_human_slots();
    }

    /// The next random number of 0x51ca14 with 0x6d11c8 on, else 0.
    fn create_lcg_if_on(&mut self) -> u32 {
        if self.create.rand_on { self.create_lcg() } else { 0 }
    }

    /// 0x49b360 (`placed`): unless 0x596872 (and not `placed`), the 8 player colors
    /// 0x57f21c are dealt from 0..8 at random.
    fn deal_colors(&mut self, placed: bool) -> [u8; 8] {
        let mut colors = [0, 1, 2, 3, 4, 5, 6, 7];
        if self.setup.template[0xd] != 0 && !placed {
            return colors;
        }
        let mut pool: Vec<u8> = (0..8).collect();
        for n in (1..=8usize).rev() {
            let r = self.create_lcg_if_on();
            let k = ((n as u32 * r) >> 15 & 0xff) as usize;
            colors[n - 1] = pool.remove(k.min(pool.len() - 1));
        }
        colors
    }

    /// 0x4ed2b0: the computer / neutral slots (1, 3, 4, 7) are named from network.tbl: by
    /// race and the slot's color (0x57f21c).
    fn name_computers(&mut self, colors: &[u8; 8]) {
        let base: u8 = if self.setup.expansion { 0x74 } else { 0 };
        for i in 0..8 {
            let s = &mut self.setup.slots[i];
            if !matches!(s[slot::TYPE], 1 | 3 | 4 | 7) {
                continue;
            }
            let c = match s[slot::RACE] {
                0 => base.wrapping_add(0x25),
                2 => base.wrapping_add(0x2d),
                _ => base.wrapping_add(0x1d),
            };
            let n = colors[i].wrapping_add(c);
            let name = if n == 0 { crate::setup::tbl_str(&self.network_raw, 10) } else { crate::setup::tbl_str(&self.network_raw, n as usize - 1) };
            Setup::sstr_into(&mut s[slot::NAME..], name);
        }
    }

    /// 0x4ee3d0 / 0x4ee520 / 0x4ee5b0 failure (the same code in each): once (0x6d5a10), the
    /// turn exchange ends (0x4c3d20(3)) and the error `id` (network.tbl, 1-based) is shown
    /// (0x4bb300); in the game state (3) the game's result is 4 (the glue loop again); then
    /// 0x4dcc50 goes to the exit screen 0x19. Always false.
    fn start_fail(&mut self, files: &Storage, id: usize) -> bool {
        if self.setup.quiet {
            return false;
        }
        // 0x4c3d20(3), as `net_4c3d20` (glue_ready.rs).
        self.setup.g654a80 = [0; 8];
        self.setup.g6556e4 = 0;
        self.setup.g6554b0 = 0;
        self.setup.g6556e0 = 0;
        self.setup.g57eebc = 0;
        self.setup.g654aa0 = 0;
        // (`net_4c3d20` also clears the ready room's command buffer, private to glue_ready.rs.)
        if self.setup.multiplayer {
            todo_port!("0x4c3d20", "Sleep(1000) and Ordinal_119 (leave the network game)");
        }
        self.setup.quiet = true;
        self.error_popup(files, id);
        if self.state == super::state::GAME {
            // 0x6d11ec = 0 and 0x6d0f31 = 0x57f23c are not kept; 0x51ce90 = 4: the glue loop.
            // The game window never opened, so 0x4e4a10's message dialog has no loop to run in.
            todo_port!("0x4e4a10", "the in-game error dialog before the game window opens");
            crate::menu::take_fatal_pending();
            self.state = super::state::GLUE;
        }
        if self.screen == 4 {
            todo_port!("0x4dcc50", "EnumWindows(0x4dc6d0) closes the process's other windows");
        }
        self.screen = super::screen::EXIT;
        false
    }

    /// 0x4ee5b0 (0x6d11c0, the ready room's mission pick, cleared): the player name default
    /// (network.tbl word 0x48), the map read again (0x4bf5d0: mode 0 from 0x57fd3c, of which
    /// only the first human stays a human; mode 1 for the campaign mission 0x57f244 =
    /// `map_dir_flag`, map name 0x59c080[idx]), the game description and template
    /// (0x4ab750), the local slot (0x4dbe50) and the game (0x4d3fc0). False (with the error
    /// shown) when one fails.
    fn start_mission_pick(&mut self, files: &Storage) -> bool {
        if !self.start_6d11c0 {
            return true;
        }
        self.start_6d11c0 = false;
        if self.setup.local_name[0] == 0 {
            let n = self.network.get(0x47).cloned().unwrap_or_default();
            sstr_copy(&mut self.setup.local_name, n.as_bytes(), 0x19);
        }
        let mission = self.setup.map_dir_flag;
        let mut hdr: MapHeader = [0; 0x20];
        if mission == 0 {
            let path = String::from_utf8_lossy(&sstr(&self.setup.map_path, 0x104)).into_owned();
            if !self.setup.load_map(files, &path, &mut hdr, false, &self.network_raw) {
                return self.start_fail(files, 0x61);
            }
            let mut humans = 0;
            for s in self.setup.slots.iter_mut().take(8) {
                if s[slot::TYPE] == 6 {
                    humans += 1;
                    if humans != 1 {
                        s[slot::TYPE] = 5;
                    }
                }
            }
        } else {
            let path = self.map_names.get(mission as usize).cloned().unwrap_or_default();
            if !self.setup.load_map(files, &path, &mut hdr, true, &self.network_raw) {
                return self.start_fail(files, 0x61);
            }
        }
        let Some(d) = self.mission_desc(files) else { return self.start_fail(files, 0x66) };
        if !self.setup.local_slot_setup() {
            return self.start_fail(files, 0x61);
        }
        // 0x596888 = 0.
        self.create_game(&d)
    }

    /// 0x4ee520 (0x6d1218, a saved game): 0x4cf5f0 reads the head (`view::load_save` does,
    /// over the game state), then the local slot 0x596bac (0x4dbe50) and the game (0x4d3fc0).
    fn start_saved(&mut self) -> bool {
        if self.save.is_none() {
            return true;
        }
        todo_port!("0x4cf5f0", "the save head is read by view::load_save; the slot 0x596bac, the slot setup 0x4dbe50 and the game 0x4d3fc0 of a saved game are not kept");
        true
    }

    /// 0x4ee3d0 (0x6d0f0c, set with state 5, cleared): the map of 0x57fd3c read again
    /// (0x4bf5d0 mode 1), the local slot (0x4dbe50; its race byte kept) and the game
    /// 0x5967f8 created again (0x4d3fc0). False (with the error shown) when one fails.
    fn start_map_again(&mut self, files: &Storage) -> bool {
        if !self.start_6d0f0c {
            return true;
        }
        let me = self.setup.local_slot[0] as usize;
        let race = self.setup.slots.get(me).map(|s| s[slot::RACE]);
        self.start_6d0f0c = false;
        let path = String::from_utf8_lossy(&sstr(&self.setup.map_path, 0x104)).into_owned();
        let mut hdr: MapHeader = [0; 0x20];
        if !self.setup.load_map(files, &path, &mut hdr, true, &self.network_raw) || !self.setup.local_slot_setup() {
            return self.start_fail(files, 0x61);
        }
        let me = self.setup.local_slot[0] as usize;
        if let (Some(r), Some(s)) = (race, self.setup.slots.get_mut(me)) {
            s[slot::RACE] = r;
        }
        // 0x596888 = 0; the description is 0x5967f8.
        let d = self.create.game_desc.clone();
        self.create_game(&d)
    }

    /// 0x4e0710 (-> 0x4ef100, 0x4eee00): the game set up by the glue for the game window.
    /// With 0x6d11c8 on: the picked races go to the slots (0x4ee110), the seed 0x51ca10
    /// seeds the numbers (0x51ca14), 0x4a9a30 deals the random races, 0x4a9d80 places the
    /// players, 0x49b360 deals the colors, 0x4ed2b0 names the computers; the map is the
    /// scenario of the map archive 0x57fd3c. The rest of 0x4ef100 / 0x4eee00 is the game
    /// start `view` runs; 0x4d9950 (the game loop), 0x4ee8c0 and the score screen after it
    /// are not ported.
    pub fn begin_game(&mut self, files: &Storage) -> Option<crate::view::GameSetup> {
        // 0x4ef100 (0x57f0b4 off): 0x4ee5b0, 0x4ee520, 0x4ee3d0 (then 0x4ee110, below).
        if !self.multiplayer && !(self.start_mission_pick(files) && self.start_saved() && self.start_map_again(files)) {
            return None;
        }
        // A saved game (the load screen, 0x4b6490 -> 0x4cf950): 0x4ef100 reads the file's state
        // instead of setting the game up.
        if let Some(save) = self.save.take() {
            // 0x4ef100: 0x4a9250 (instead of 0x4a9d80) rebuilds the slots of a network game.
            self.rebuild_slots();
            return Some(crate::view::GameSetup {
                chk: Vec::new(),
                mpq: None,
                map_dir: None,
                types: [0; 12],
                races: [0; 12],
                names: Default::default(),
                forces: [0; 8],
                colors: [0; 8],
                template: [0; 0x20],
                local: 0,
                seed: 0,
                objectives: self.objectives,
                save: Some(save),
                camp_cont: false,
                mission: self.setup.map_dir_flag,
                expansion: self.expansion,
            });
        }
        self.race_picks_apply();
        // 0x4d4130 (0x6d1218 off): 0x470150 keeps the lobby types before the slots are placed.
        if !self.setup.preset_slot_used {
            self.setup.snapshot_types();
        }
        // 0x51ca10 = time (a debug seed from `set_seed` or SCBW_SEED); 0x51ca14 = 0x51ca10.
        let seed = SEED.with(|s| s.take()).or_else(|| std::env::var("SCBW_SEED").ok().and_then(|s| s.parse().ok())).unwrap_or_else(|| {
            web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32)
        });
        self.create.seed = seed;
        self.create.rand_on = true;
        self.assign_races();
        let placed = self.place_players();
        let colors = self.deal_colors(placed);
        self.create.rand_on = false;
        self.name_computers(&colors);
        let (chk, mpq) = self.setup.load_scenario(files, None, &self.network_raw)?;
        let name = |s: &[u8]| String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]).into_owned();
        let s = &self.setup.slots;
        Some(crate::view::GameSetup {
            chk,
            mpq,
            // 0x57f244: a campaign mission's wavs come from its directory.
            map_dir: (self.setup.map_dir_flag != 0).then(|| self.map_names.get(self.setup.map_dir_flag as usize).cloned().unwrap_or_default()),
            types: std::array::from_fn(|i| s[i][slot::TYPE]),
            races: std::array::from_fn(|i| s[i][slot::RACE].min(2)),
            names: std::array::from_fn(|i| name(&s[i][slot::NAME..slot::NAME + 0x19])),
            forces: std::array::from_fn(|i| s[i][slot::FORCE]),
            colors,
            template: self.setup.template,
            local: self.setup.local_slot[0] as u8,
            seed,
            objectives: self.objectives,
            save: None,
            camp_cont: self.camp_cont,
            mission: self.setup.map_dir_flag,
            expansion: self.expansion,
        })
    }
}

thread_local! {
    static SEED: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// The next game's seed instead of the time (a scripted "seed" step: the recorded game's).
pub fn set_seed(seed: u32) {
    SEED.with(|s| s.set(Some(seed)));
}
