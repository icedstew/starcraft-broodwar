//! The front end (glues.cpp, gluMain.cpp, uiSingle.cpp, ...): the top loop 0x4e0820,
//! the glue loop 0x4de200 and its screens.
//!
//! Native screens are blocking: each screen function loads its `.bin`, runs it modally
//! with 0x41a080 (event proc in, dialog result out) and then acts on the result. Here
//! the window loop is not blocking (the browser build calls one frame at a time), so
//! each screen is split at its 0x41a080 call: `open_*` is the part before it, the
//! dialog's event proc runs per input event, and `done_*` is the part after it. Both
//! halves carry the screen function's address.

use crate::dlg::{Dialog, Effects, Skin, Surf};
use crate::grp::{Blend, Grp};
use crate::storage::Storage;
use crate::todo_port;

/// 0x50e06c: the glue palettes (0x518-byte records: directory, flags +0x110, palette).
pub const PALETTES: [&str; 12] = [
    "glue/PalMm", "glue/PalCs", "glue/PalRt", "glue/PalRz", "glue/PalRp", "glue/PalPd",
    "glue/PalPv", "glue/PalZd", "glue/PalZv", "glue/PalTd", "glue/PalTv", "glue/PalNl",
];

/// 0x4ffad0 (12 bytes per screen: id, palette, directory): the palette of each glue
/// screen 0x6d11bc (0x19, the exit screen, has none).
pub const SCREEN_PALETTE: [u8; 0x19] = [
    0, 0xb, 0xb, 0xb, 0xc, 0xb, 1, 2, 3, 4, 0xb, 0xb, 0xb, 0xb, 7, 8, 9, 0xa, 5, 6, 0xb, 0xb,
    1, 0xb, 0xb,
];

/// 0x596904: what the top loop 0x4e0820 runs next.
pub mod state {
    /// 0x4d4470: the intro cinematics, then the glue.
    pub const INTRO: u8 = 0;
    /// 1 / 5: a game starts (5 also sets 0x6d0f0c).
    pub const START: u8 = 1;
    /// 0x4e0710: the game runs.
    pub const GAME: u8 = 3;
    /// 0x4de200: the glue loop (6 / 7 come back from a game to the score screen).
    pub const GLUE: u8 = 4;
    /// 0x4d8fd0: the credits.
    pub const CREDITS: u8 = 8;
    /// 0x4d90c0.
    pub const CREDITS_EXP: u8 = 9;
    /// 0x4f7ae0 / 0x4dc320: back from a game.
    pub const AFTER_GAME: u8 = 10;
}

/// 0x6d11bc: the glue screens (cases of the 0x4de200 switch).
pub mod screen {
    pub const MAIN: u8 = 0;
    pub const SELCONN: u8 = 2;
    pub const CHAT: u8 = 3;
    pub const BATTLE_NET: u8 = 4;
    pub const LOGIN: u8 = 5;
    pub const CAMPAIGN: u8 = 6;
    pub const READY_T: u8 = 7;
    pub const READY_Z: u8 = 8;
    pub const READY_P: u8 = 9;
    pub const GAMESEL: u8 = 10;
    pub const CREATE: u8 = 0xb;
    pub const CREATE2: u8 = 0xc;
    pub const LOAD: u8 = 0xd;
    pub const SCORE_FIRST: u8 = 0xe;
    pub const SCORE_LAST: u8 = 0x13;
    pub const MODEM: u8 = 0x14;
    pub const DIRECT: u8 = 0x15;
    pub const CAMPAIGN2: u8 = 0x16;
    pub const EXIT: u8 = 0x19;
}

/// What the window loop does after a top-loop step.
#[derive(Debug, PartialEq)]
pub enum Top {
    /// Keep running the glue (a dialog is open).
    Glue,
    /// 0x4d4470: play the intro cinematics.
    Intro,
    /// 0x4e0710: run the game set up by the glue.
    Game,
    /// 0x4d8fd0 / 0x4d90c0.
    Credits,
    /// The default case: 0x4207b0, 0x41ee50(1), exit(0).
    Quit,
}

/// The screen that opened the current dialog (picks its event proc and result code).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Open {
    /// 0x4db930 with event proc 0x4db7a0.
    Main,
    /// 0x4b7180: rez\gluPOk.bin, event proc 0x4b6d60.
    Popup,
    /// 0x4b7a70: rez\gluPOkSplit.bin (a title and a text).
    PopupSplit,
    /// 0x4b59a0: the exe's GLUEOK_BIN, event proc 0x4b5920.
    Error,
    /// 0x4b5cc0: GLUEOK_BIN with a title and a text.
    ErrorSplit,
}

/// 0x50e06c + 0x10a: the light percentage translucent image controls use (0x6d5e24).
/// Every palette has 50 (0x32) but the last (PalNl) with 75.
fn image_percent(palette: usize) -> i32 {
    if palette == 11 { 75 } else { 50 }
}

/// 0x50e06c + 0x108: the tEffect.pcx pixel the palette's shade color (0x51a18c) is.
const SHADE_INDEX: usize = 1;

pub struct Glue {
    /// 0x596904.
    pub state: u8,
    /// 0x6d11bc.
    pub screen: u8,
    /// 0x51bff8: 1 until the glue loop's first pass picked its start screen.
    first: bool,
    /// 0x57f0b4: a multiplayer game.
    pub multiplayer: bool,
    /// 0x58f440: Brood War (expansion) chosen.
    pub expansion: bool,
    /// 0x6d11e4: Brood War installed (BroodWar.mpq found).
    pub expansion_installed: bool,
    /// 0x5124d0: the spawn (demo) version. Always 0 for a full install.
    pub spawn: bool,
    /// 0x51c4bc: started with a map to play (command line); 0 here.
    pub direct_map: bool,
    /// 0x57ee9c: the player name (0x19 bytes).
    pub player_name: String,
    /// 0x596865: the game type template (8 dwords) the main menu copies from 0x4ab840.
    pub template: [u32; 8],
    /// 0x6d051c: the control the last event activated.
    pub activated: i16,
    /// The dialog 0x41a080 runs, and which screen opened it.
    pub dlg: Option<(Dialog, Open)>,
    /// A popup 0x41a080 runs over it (0x6d5a3c / 0x6d5a58, nested in the screen's event
    /// proc natively).
    pub popup: Option<(Dialog, Open)>,
    /// 0x41a080's result once the event proc ended the dialog (0x4dd040).
    result: Option<i32>,
    /// rez\stat_txt.tbl (0x6d1220).
    stat_txt: Vec<String>,
    /// rez\gluAll.tbl (0x4ddd30).
    glu_all: Vec<String>,
    /// 0x6cddc4 / 0x6cddc8: the mouse.
    pub mouse: (i32, i32),
    /// 0x50e064: the palette currently shown (-1 none).
    shown: i32,
    /// Per-palette graphics (0x4dd850, 0x4dd910, 0x4ddf00, 0x4ddf90), loaded on demand.
    looks: Vec<Option<Look>>,
    /// 0x597390: the cursor animation frame, advanced by the 50 ms timer 0x4dcea0.
    cursor_frame: usize,
    cursor_ms: u32,
    /// 0x6d63b0: an error dialog is being shown as a popup.
    error_depth: u8,
}

impl Glue {
    /// 0x4e0820 (before its loop): state 4 (glue), settings, cursor at (320, 240).
    pub fn new(files: &Storage) -> Glue {
        todo_port!("0x4e0820", "0x4daf30 / 0x4d2ff0 / 0x4e05b0 / 0x4db060 / 0x4f6100 init, CPUThrottle");
        let stat_txt = files.read("rez/stat_txt.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default();
        let glu_all = files.read("rez/gluAll.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default();
        Glue {
            state: state::GLUE,
            screen: screen::MAIN,
            first: true,
            multiplayer: false,
            expansion: false,
            expansion_installed: files.read("rez/gluexpcmpgn.bin").is_ok(),
            spawn: false,
            direct_map: false,
            player_name: String::new(),
            template: [0; 8],
            activated: 0,
            dlg: None,
            popup: None,
            result: None,
            stat_txt,
            glu_all,
            mouse: (320, 240),
            shown: -1,
            looks: (0..PALETTES.len()).map(|_| None).collect(),
            cursor_frame: 0,
            cursor_ms: 0,
            error_depth: 0,
        }
    }

    /// 0x4e0820's loop body: one pass of its switch on 0x596904. `Top::Glue` while the
    /// glue loop has a dialog open.
    pub fn top_step(&mut self, files: &Storage) -> Top {
        if !self.expansion_installed {
            self.expansion = false;
        }
        match self.state {
            state::INTRO => {
                // 0x4d4470 twice (the expansion's intro when gluexpcmpgn.bin exists), then 4.
                self.state = state::GLUE;
                Top::Intro
            }
            state::GAME => Top::Game,
            state::GLUE | 6 | 7 => {
                if self.dlg.is_none() {
                    self.glue_step(files);
                }
                if self.state == state::GLUE || self.dlg.is_some() { Top::Glue } else { self.top_step(files) }
            }
            5 | state::START => {
                self.state = state::GAME;
                Top::Game
            }
            state::CREDITS | state::CREDITS_EXP => {
                // After 0x4d8fd0 / 0x4d90c0: back to the main menu.
                self.state = state::GLUE;
                self.screen = screen::MAIN;
                Top::Credits
            }
            state::AFTER_GAME => {
                todo_port!("0x4f7ae0", "after a game: 0x4f7ae0, 0x5122b8 = 0x1c, 0x4dc320(1)");
                self.state = state::GLUE;
                Top::Glue
            }
            _ => Top::Quit,
        }
    }

    /// The palette (0x4ffad0) of the current screen.
    fn palette_id(&self) -> usize {
        SCREEN_PALETTE.get(self.screen as usize).copied().unwrap_or(0) as usize
    }

    /// The graphics of palette `p`, loading them first. Natively 0x4de200 loads all 12
    /// up front (fonts 0x4daec0, tEffect 0x4dd910, arrow.grp 0x4ddf90, tFont 0x4dd850,
    /// Dlg.grp 0x4ddf00) and frees them at the end (0x4dc870, 0x4dc940: Drop).
    fn look(&mut self, files: &Storage, p: usize) -> &Look {
        if self.looks[p].is_none() {
            self.looks[p] = Some(Look::load(files, p).unwrap_or_else(|e| crate::bw::fatal_error(&format!("{}: {e}", PALETTES[p]))));
        }
        self.looks[p].as_ref().unwrap()
    }

    /// 0x4de200 before its loop: the glue resources, then (first pass) the start screen.
    fn glue_enter(&mut self) {
        todo_port!("0x4de200", "0x4de0b0 sound hooks 0x4dd1d0 (hover 0xa6 / click 0xa5)");
        if self.state == 6 || self.state == 7 {
            self.state = state::GLUE;
            self.screen = 0x11;
            return;
        }
        if !self.first || self.multiplayer {
            return;
        }
        self.first = false;
        if self.player_name.is_empty() {
            // stat_txt string 0x48 (1-based index 0x49), else the default at 0x501b7d.
            self.player_name = self.stat_txt.get(0x48).cloned().unwrap_or_default();
            self.player_name.truncate(0x18);
        }
        self.expansion = self.direct_map;
        // 0x57f1e3 = 0.
        if !self.direct_map || self.expansion_installed {
            // 0x4db200 (campaign .bin present) and 0x4dbfc0 (start the mission 0x57f244)
            // pick ready room 8 / 7 / 9 by 0x51c608 and set 0x6d11c0; 0x596904 = 1 with
            // 0x6d5a6c & 0x20000000.
            todo_port!("0x4de200", "start screen probe 0x4db200 / 0x4dbfc0");
        }
        // Their failure path.
        self.screen = screen::MAIN;
        self.expansion = false;
    }

    /// One pass of 0x4de200's `while (0x596904 == 4) switch (0x6d11bc)` (the glue loop),
    /// entering it first when it was not running.
    fn glue_step(&mut self, files: &Storage) {
        if self.state != state::GLUE {
            self.glue_enter();
            if self.state != state::GLUE {
                return;
            }
        }
        match self.screen {
            screen::MAIN => self.open_main(files),
            screen::SELCONN | screen::CHAT | screen::BATTLE_NET | screen::GAMESEL | screen::DIRECT | screen::MODEM => {
                // Networking is deferred (re/buckets.txt): 0x4baf30 / 0x4b9e70 / 0x4ad330
                // / 0x4b86c0 / 0x4b19c0. Back to the main menu.
                self.multiplayer = false;
                self.screen = screen::MAIN;
            }
            screen::LOGIN => todo_port!("0x4edc30", "gluLogin.bin"),
            screen::CAMPAIGN => todo_port!("0x4b2930", "campaign select"),
            screen::READY_T => todo_port!("0x4f7550", "ready room (terran)"),
            screen::READY_Z => todo_port!("0x4f71f0", "ready room (zerg)"),
            screen::READY_P => todo_port!("0x4f78b0", "ready room (protoss)"),
            screen::CREATE | screen::CREATE2 => todo_port!("0x4af6d0", "create game"),
            screen::LOAD => todo_port!("0x4b6490", "load game"),
            screen::SCORE_FIRST..=screen::SCORE_LAST => todo_port!("0x4b4af0", "score"),
            screen::CAMPAIGN2 => todo_port!("0x4b52a0", "expansion campaign select"),
            screen::EXIT => todo_port!("0x4dcc90", "glue exit"),
            _ => self.screen = screen::MAIN,
        }
    }

    /// 0x4dd9e0 (event 10 of a glue dialog, `bg` None: the screen's own background): the
    /// dialog takes the palette's BackGnd.pcx (and its palette, shown by 0x4de150), the
    /// interface colors are matched to it (0x41e450) and the translucency tables and
    /// text colors built (0x4178b0, 0x6ce000). The Rust equivalents live in `Look`.
    fn glue_dialog_init(&mut self, files: &Storage, dlg: &mut Dialog, bg: bool) {
        // 0x4e10f0: the light-up buttons' 30 ms timer is `Dialog::tick`.
        dlg.this.flags |= 0x2000_0000;
        let p = self.palette_id();
        let look = self.look(files, p);
        if bg {
            dlg.bg = Some(look.back.clone());
        }
        todo_port!("0x416090", "dialog timers (0x4dcea0 cursor animation is Glue::tick)");
    }

    /// 0x4db930 (before 0x41a080): the main menu, rez\gluMain.bin with timer proc
    /// 0x4d27a0 and event proc 0x4db7a0.
    fn open_main(&mut self, files: &Storage) {
        // 0x6d5a10 = 0 (the 0x4de200 case), 0x57f0b4 = 0.
        self.multiplayer = false;
        let Ok(bin) = files.read("rez/gluMain.bin") else {
            crate::bw::fatal_error("rez\\gluMain.bin");
        };
        let mut dlg = Dialog::parse(&bin);
        // +0x18 |= 4.
        dlg.this.flags |= 4;
        // 0x4194e0: the images (eTail, pIntro, pCredit).
        dlg.bind(files);
        // 0x41a080: event 10 to the controls (the light-up buttons open their videos), then
        // to the event proc 0x4db7a0: case 10 (0x4dd9e0), case 0 (0x4db280 + 0x4de150 +
        // title music), the dialog's init.
        dlg.init_lightups(files);
        self.glue_dialog_init(files, &mut dlg, true);
        self.main_version_label(&mut dlg);
        self.show_palette();
        todo_port!("0x4a6310", "title music (unless 0x6d5bbc)");
        self.dlg = Some((dlg, Open::Main));
    }

    /// 0x4db280: the version label (control 10): gluAll.tbl string 3 ("%s") with the
    /// build string 0x5122c8. The spawn version formats string 2 with the license text.
    fn main_version_label(&mut self, dlg: &mut Dialog) {
        let Some(c) = dlg.get_mut(10) else { return };
        if self.spawn {
            todo_port!("0x4db280", "spawn: 'spawned by %s, %s' fitted to the label");
            return;
        }
        let fmt = self.glu_all.get(2).cloned().unwrap_or_default();
        c.text = fmt.replace("%s", "internal version unknown").into_bytes();
    }

    /// 0x4de150: shows the current screen's palette (fades it in unless it is on).
    fn show_palette(&mut self) {
        let p = SCREEN_PALETTE.get(self.screen as usize).copied().unwrap_or(0) as i32;
        if self.shown != p {
            todo_port!("0x41ea30", "palette fade-in");
            self.shown = p;
        }
    }

    /// 0x4db930 (after 0x41a080): act on the main menu's result.
    fn done_main(&mut self, result: i32) {
        match result {
            2 => self.state = 2,
            3 => {
                self.multiplayer = false;
                self.screen = screen::LOGIN;
                self.template = self.default_template();
            }
            4 => {
                self.multiplayer = true;
                self.screen = screen::SELCONN;
                self.template = self.default_template();
            }
            5 | 0xfff0 => {}
            8 => self.state = state::INTRO,
            9 => self.state = state::CREDITS,
            _ => self.screen = screen::MAIN,
        }
        self.glue_close();
    }

    /// 0x4dcfa0: after a glue dialog: its background cache is dropped (the next one
    /// loads its own); when the next screen has another palette the old one fades out.
    fn glue_close(&mut self) {
        if self.shown != SCREEN_PALETTE.get(self.screen as usize).copied().unwrap_or(0) as i32 {
            todo_port!("0x41e9e0", "palette fade-out");
        }
    }

    /// 0x4ab840: the default game type template (0x4ab750 on Templates\*.got).
    fn default_template(&self) -> [u32; 8] {
        todo_port!("0x4ab840", "Templates\\*.got default game type");
        [0; 8]
    }

    /// 0x4db7a0: the main menu's event proc, for an activated control (event 0xe, code 2;
    /// the id in 0x6d051c). True when it handled the event without closing the dialog.
    fn main_activate(&mut self, files: &Storage, id: i16) -> bool {
        self.activated = id;
        match id {
            3 | 4 => {
                if self.spawn {
                    // The spawn version has no single player campaign / multiplayer: the
                    // popup with stat_txt string 0x66 (0-based), else the empty default.
                    let msg = self.stat_txt.get(0x66).cloned().unwrap_or_default();
                    self.open_popup(files, &msg);
                    return true;
                }
                if self.expansion_installed {
                    todo_port!("0x4db6e0", "StarCraft / Brood War choice (rez\\gluexpcmpgn.bin)");
                    self.expansion = true;
                } else {
                    todo_port!("0x4db770", "single player / multiplayer check");
                    self.expansion = false;
                }
                self.end_dialog(id as i32)
            }
            5 => {
                todo_port!("0x4db490", "campaign editor (StarEdit.exe)");
                true
            }
            _ => self.end_dialog(id as i32),
        }
    }

    /// 0x4dd040: end the modal dialog with `result` (0x41a080 returns it): the buttons
    /// of the exit slide (0x51c428 list) are empty on the main menu, so the 0x4dcd00
    /// timer ends the dialog at once; every control loses flag 0x10.
    fn end_dialog(&mut self, result: i32) -> bool {
        todo_port!("0x4bbfd0", "click sound at the sound volume (0x6d59f4)");
        if let Some((dlg, _)) = self.dlg.as_mut() {
            dlg.this.flags &= !0x10;
            for c in &mut dlg.controls {
                c.flags &= !0x10;
            }
        }
        self.result = Some(result);
        true
    }

    /// 0x4b7180: a message popup (rez\gluPOk.bin) over the current dialog: the text in
    /// the label (id 2). An open popup is closed first.
    pub fn open_popup(&mut self, files: &Storage, msg: &str) {
        self.popup = None;
        self.popup_dialog(files, "rez/gluPOk.bin", Open::Popup, msg, "");
    }

    /// 0x4b7a70: the popup with a title (id 2) and a text (id 5). An empty `title` is
    /// 0x4b7180.
    pub fn open_popup_split(&mut self, files: &Storage, title: &str, msg: &str) {
        self.popup = None;
        if title.is_empty() {
            return self.open_popup(files, msg);
        }
        self.popup_dialog(files, "rez/gluPOkSplit.bin", Open::PopupSplit, title, msg);
    }

    /// The part of 0x4b7180 / 0x4b7a70 up to 0x41a080: event 7 (0x4cd9c0: centered), 0
    /// (0x4b6810: the texts; their fit 0x4b6660), 10 (0x4b6c70: 0x4dd9e0 with the
    /// popup's pOPopup.pcx as the background).
    fn popup_dialog(&mut self, files: &Storage, bin: &str, open: Open, first: &str, second: &str) {
        let Ok(d) = files.read(bin) else { crate::bw::fatal_error(bin) };
        let mut dlg = Dialog::parse(&d);
        dlg.this.flags |= 4;
        dlg.bind(files);
        dlg.init_lightups(files);
        let p = self.palette_id();
        let dir = PALETTES[p];
        let Ok(pcx) = files.read(&format!("{dir}/pOPopup.pcx")) else { crate::bw::fatal_error("pOPopup.pcx") };
        let (w, h, px) = crate::view::pcx(&pcx);
        if self.state == state::GLUE {
            let (x, y) = ((640 - dlg.this.rect.2) / 2, (480 - dlg.this.rect.3) / 2);
            dlg.this.rect = (x, y, dlg.this.rect.2 + x, dlg.this.rect.3 + y);
        }
        if let Some(c) = dlg.get_mut(2) {
            c.text = first.as_bytes().to_vec();
            todo_port!("0x4b6660", "shrink the text's font until it fits the label");
        }
        if !second.is_empty() {
            if let Some(c) = dlg.get_mut(5) {
                c.text = second.as_bytes().to_vec();
            }
        }
        self.glue_dialog_init(files, &mut dlg, false);
        dlg.this.flags |= 0x40000;
        dlg.bg = Some((w as i32, h as i32, px));
        self.popup = Some((dlg, open));
    }

    /// 0x4b59a0: an error message. With a dialog already open (and no error shown yet)
    /// it is the glue popup 0x4b7180; else GLUEOK_BIN from the exe.
    pub fn open_error(&mut self, files: &Storage, msg: &str) {
        self.error_dialog(files, "", msg);
    }

    /// 0x4b5cc0: an error with a title and a text; an empty title is 0x4b59a0.
    pub fn open_error_split(&mut self, files: &Storage, title: &str, msg: &str) {
        self.error_dialog(files, title, msg);
    }

    fn error_dialog(&mut self, files: &Storage, title: &str, msg: &str) {
        if self.popup.as_ref().is_some_and(|(_, o)| matches!(o, Open::Error | Open::ErrorSplit)) {
            self.popup = None;
        }
        if self.dlg.is_some() && self.error_depth == 0 {
            self.error_depth = 1;
            return if title.is_empty() { self.open_popup(files, msg) } else { self.open_popup_split(files, title, msg) };
        }
        // 0x4b57b0 (event 10): the title in id 2, the text in id 5; the picture of the
        // dialog comes from the resource 0x1b5d (0x4b5450).
        let Some(bin) = exe_resource(0x1b5c, "GLUEOK_BIN") else {
            todo_port!("0x4b59a0", "GLUEOK_BIN resource of StarCraft.exe");
            return;
        };
        let mut dlg = Dialog::parse(&bin);
        dlg.this.flags |= 4;
        let (a, b) = if title.is_empty() { (msg, "") } else { (title, msg) };
        if let Some(c) = dlg.get_mut(2) {
            c.text = a.as_bytes().to_vec();
        }
        if let Some(c) = dlg.get_mut(5) {
            c.text = b.as_bytes().to_vec();
        }
        todo_port!("0x4b55a0", "the error dialog's picture from the exe's resources");
        self.popup = Some((dlg, if title.is_empty() { Open::Error } else { Open::ErrorSplit }));
    }

    /// A control of the open dialog was activated (the default proc 0x418eb0 sends event
    /// 0xe code 2 to the dialog's event proc). Runs the screen's `done_*` once the event
    /// proc ended the dialog.
    pub fn activate(&mut self, files: &Storage, id: i16) {
        if self.popup.is_some() {
            // 0x4b6d60 / 0x4b5920 -> 0x418eb0: any control ends the popup (event 1 releases
            // its timers: 0x4dcc00).
            self.popup = None;
            self.error_depth = 0;
            return;
        }
        let Some((_, open)) = self.dlg.as_ref() else { return };
        match open {
            Open::Main => {
                self.main_activate(files, id);
            }
            _ => {}
        }
        if let Some(r) = self.result.take() {
            let (_, open) = self.dlg.take().unwrap();
            if open == Open::Main {
                self.done_main(r);
            }
        }
    }

    /// The mouse moved to (x, y): the dialog's active control (0x419640) lights up.
    pub fn mouse_to(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        let top_is_popup = self.popup.is_some();
        for (n, d) in [self.dlg.as_mut(), self.popup.as_mut()].into_iter().enumerate() {
            let Some((dlg, _)) = d else { continue };
            // The lower dialog is not lit while a popup is on top.
            let over = !(n == 0 && top_is_popup);
            dlg.mouse = if over { (x, y) } else { (-1, -1) };
            let hit = if over { dlg.hit(x, y) } else { None };
            dlg.active = hit;
            for (i, c) in dlg.controls.iter_mut().enumerate() {
                if hit == Some(i) { c.flags |= 0x80 } else { c.flags &= !0x80 }
            }
        }
    }

    /// `ms` pass: the dialogs' 30 ms timers (0x4e0f50) and the cursor's 50 ms timer
    /// (0x4dcea0 -> 0x4be120 advances 0x597390).
    pub fn tick(&mut self, files: &Storage, ms: u32) {
        let _ = files;
        for (d, _) in [self.dlg.as_mut(), self.popup.as_mut()].into_iter().flatten() {
            d.tick(ms);
        }
        self.cursor_ms += ms;
        self.cursor_frame += (self.cursor_ms / 50) as usize;
        self.cursor_ms %= 50;
    }

    /// Draws the open dialogs (the main one, then its popup) and the cursor into the
    /// 640x480 8-bit `px`; returns the screen's palette.
    pub fn draw(&mut self, files: &Storage, px: &mut [u8]) -> [[u8; 3]; 256] {
        let p = self.palette_id();
        self.look(files, p);
        let look = self.looks[p].as_ref().unwrap();
        px.fill(0);
        let mut s = Surf { px, w: 640, h: 480 };
        for (dlg, _) in [self.dlg.as_ref(), self.popup.as_ref()].into_iter().flatten() {
            look.skin.draw(dlg, &mut s);
        }
        // 0x4be120: the frame's grp box sits at the mouse minus 0x3f.
        let n = look.arrow.frame_count().max(1);
        look.arrow.draw(self.cursor_frame % n, false, self.mouse.0 - 0x3f, self.mouse.1 - 0x3f, s.px, 640, 480, Blend::Normal);
        look.pal
    }
}

/// The last 768 bytes of a .pcx: its palette.
fn pcx_palette(d: &[u8]) -> [[u8; 3]; 256] {
    let mut pal = [[0u8; 3]; 256];
    if d.len() >= 769 {
        for (i, c) in d[d.len() - 768..].chunks(3).enumerate() {
            pal[i] = [c[0], c[1], c[2]];
        }
    }
    pal
}

/// FindResourceA on the install's StarCraft.exe (0x4b59a0, 0x4b5450): the named
/// resource of numeric type `ty`.
fn exe_resource(ty: usize, name: &str) -> Option<Vec<u8>> {
    let d = std::fs::read(crate::install_dir().join("StarCraft.exe")).ok()?;
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let pe = u32_at(0x3c)?;
    let opt = pe + 24;
    let (rsrc, _) = (u32_at(opt + 0x60 + 16)?, 0);
    // The section holding the resource directory maps its RVA to a file offset.
    let sections = u16_at(pe + 6)?;
    let first = opt + u16_at(pe + 20)?;
    let to_file = |rva: usize| {
        (0..sections).find_map(|i| {
            let s = first + i * 40;
            let (vs, va, raw) = (u32_at(s + 8)?, u32_at(s + 12)?, u32_at(s + 20)?);
            (rva >= va && rva < va + vs).then_some(rva - va + raw)
        })
    };
    let base = to_file(rsrc)?;
    // A directory: (name or id, offset) entries; the high bit marks a name / subdirectory.
    let entries = |dir: usize| -> Option<Vec<(usize, usize)>> {
        let n = u16_at(dir + 12)? + u16_at(dir + 14)?;
        (0..n).map(|i| Some((u32_at(dir + 16 + i * 8)?, u32_at(dir + 20 + i * 8)?))).collect()
    };
    let (_, t) = entries(base)?.into_iter().find(|&(id, _)| id == ty)?;
    let (_, n) = entries(base + (t & 0x7fff_ffff))?.into_iter().find(|&(id, _)| {
        let o = base + (id & 0x7fff_ffff);
        let len = u16_at(o).unwrap_or(0);
        id & 0x8000_0000 != 0
            && (0..len).map(|k| u16_at(o + 2 + k * 2).unwrap_or(0) as u8 as char).collect::<String>().eq_ignore_ascii_case(name)
    })?;
    let (_, l) = entries(base + (n & 0x7fff_ffff))?.into_iter().next()?;
    let e = base + (l & 0x7fff_ffff);
    let (rva, size) = (u32_at(e)?, u32_at(e + 4)?);
    let o = to_file(rva)?;
    d.get(o..o + size).map(|b| b.to_vec())
}

/// The graphics of one glue palette: 0x4dd850 tFont.pcx (the text color rows), 0x4ddf00
/// Dlg.grp, 0x4dd910 tEffect.pcx (the shade color), 0x4ddf90 arrow.grp, the palette and
/// background of 0x4dd9e0 (BackGnd.pcx) and the fonts (0x4daec0: font8, font10, font16,
/// font16x as ui.rs loads them).
struct Look {
    skin: Skin,
    pal: [[u8; 3]; 256],
    /// BackGnd.pcx (0x4dd9e0): the dialog bitmap.
    back: (i32, i32, Vec<u8>),
    arrow: Grp,
}

impl Look {
    fn load(files: &Storage, p: usize) -> std::io::Result<Look> {
        use crate::ui::{read_font, Font};
        let dir = PALETTES[p];
        let tfont = files.read(&format!("{dir}/tFont.pcx"))?;
        let fonts = [
            Font::parse(read_font(files, "font/font8.fnt")?),
            Font::parse(read_font(files, "font/font10.fnt")?),
            Font::parse(read_font(files, "font/font16.fnt")?),
            Font::parse(read_font(files, "font/font16x.fnt")?),
        ];
        // 0x4dd9e0: the dialog loads <dir>\BackGnd.pcx; its palette is the screen's.
        let back = files.read(&format!("{dir}/BackGnd.pcx"))?;
        let pal = pcx_palette(&back);
        let (bw, bh, bpx) = crate::view::pcx(&back);
        // 0x4bdd60 (tileset 0, as at startup): the colors the interface colors skip.
        let mut excluded = [false; 256];
        for &(lo, hi) in crate::bw::minimap::cycling(0) {
            excluded[lo as usize..=hi as usize].iter_mut().for_each(|e| *e = true);
        }
        let colors = crate::bw::minimap::UI_RGB.map(|c| crate::bw::minimap::nearest(&pal, &excluded, c));
        let shade = crate::view::pcx(&files.read(&format!("{dir}/tEffect.pcx"))?).2.get(SHADE_INDEX).copied().unwrap_or(0);
        let skin = Skin {
            grp: Grp::parse_raw(files.read(&format!("{dir}/dlg.grp"))?),
            tile: Grp::parse(files.read("dlgs/tile.grp")?),
            colors: std::cell::Cell::new(colors),
            fonts,
            text_rows: crate::view::pcx(&tfont).2,
            fx: Some(Effects::new(&pal, shade, image_percent(p))),
        };
        let arrow = Grp::parse(files.read(&format!("{dir}/arrow.grp"))?);
        Ok(Look { skin, pal, back: (bw as i32, bh as i32, bpx), arrow })
    }
}

/// The front end's window loop (`scbw glue`). `shot`: write the first frame to a PNG
/// and exit.
pub fn run(files: &Storage, shot: Option<&str>) -> std::io::Result<()> {
    use minifb::{MouseButton, MouseMode, Scale, Window, WindowOptions};
    let (w, h) = (640usize, 480usize);
    let mut glue = Glue::new(files);
    let mut idx = vec![0u8; w * h];
    let mut rgb = vec![0u32; w * h];
    let mut win = if shot.is_none() {
        let opts = WindowOptions { scale: Scale::X2, ..WindowOptions::default() };
        let mut win = Window::new("scbw", w, h, opts).map_err(|e| std::io::Error::other(e.to_string()))?;
        win.set_target_fps(24);
        Some(win)
    } else {
        None
    };
    let mut pressed: Option<i16> = None;
    let mut popped = false;
    let mut last = std::time::Instant::now();
    loop {
        match glue.top_step(files) {
            Top::Glue => {}
            Top::Quit => return Ok(()),
            Top::Intro | Top::Credits => todo_port!("0x4d4470", "cinematics / credits"),
            Top::Game => {
                todo_port!("0x4e0710", "start the game set up by the glue");
                return Ok(());
            }
        }
        // SCBW_GLUE_POPUP="text" (or "title|text"): shows a popup over the screen.
        if let Some(t) = std::env::var_os("SCBW_GLUE_POPUP").filter(|_| glue.popup.is_none() && !popped) {
            popped = true;
            let t = t.to_string_lossy().into_owned();
            match t.split_once('|') {
                Some((a, b)) => glue.open_popup_split(files, a, b),
                None => glue.open_popup(files, &t),
            }
        }
        let now = std::time::Instant::now();
        glue.tick(files, now.duration_since(last).as_millis() as u32);
        last = now;
        let pal = glue.draw(files, &mut idx);
        for (o, &i) in rgb.iter_mut().zip(&idx) {
            let [r, g, b] = pal[i as usize];
            *o = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
        let Some(win) = win.as_mut() else {
            return crate::write_png(shot.unwrap(), w, h, &idx, &pal);
        };
        if !win.is_open() {
            return Ok(());
        }
        if let Some((x, y)) = win.get_mouse_pos(MouseMode::Clamp) {
            glue.mouse_to(x as i32, y as i32);
            // 0x418eb0's button behavior: activate on release over the pressed control.
            let top = glue.popup.as_ref().or(glue.dlg.as_ref());
            let hit = top.and_then(|(dlg, _)| dlg.hit(x as i32, y as i32).map(|i| dlg.controls[i].id));
            if win.get_mouse_down(MouseButton::Left) {
                pressed = pressed.or(hit);
            } else if let Some(id) = pressed.take() {
                if hit == Some(id) {
                    glue.activate(files, id);
                }
            }
        }
        win.update_with_buffer(&rgb, w, h).map_err(|e| std::io::Error::other(e.to_string()))?;
    }
}
