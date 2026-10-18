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
use crate::setup::{slot, Setup};
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
    /// 0x4f71f0 (0 zerg) / 0x4f7550 (1 terran) / 0x4f78b0 (2 protoss): rez\gluRdy?.bin, event
    /// proc 0x4f7150 / 0x4f74b0 / 0x4f7810.
    Ready(u8),
    /// 0x4edc30: rez\gluLogin.bin, event proc 0x4edaa0.
    Login,
    /// 0x4af6d0: rez\gluCustm.bin (single player) / gluCreat.bin (network), event proc
    /// 0x4af5c0.
    Create(u8),
    /// 0x4b4af0: rez\gluScore.bin, event proc 0x4b4a30; true when the local player won.
    Score(bool),
    /// 0x4b2930: rez\gluCmpgn.bin, event proc 0x4b28b0.
    Campaign,
    /// 0x4b52a0: rez\gluExpCmpgn.bin, event proc 0x4b5220.
    CampaignExp,
    /// 0x4b6490: rez\gluLoad.bin, event proc 0x4b6410.
    Load,
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
    /// rez\network.tbl (0x6d1220: the default player name, the campaign advice, ...).
    network: Vec<String>,
    /// rez\network.tbl as read.
    network_raw: Vec<u8>,
    /// rez\gluAll.tbl (0x4ddd30).
    glu_all: Vec<String>,
    /// gluAll.tbl as read (Setup's profile name check).
    glu_all_raw: Vec<u8>,
    /// The game-setup globals the glue fills (the profile, the mission 0x57f244).
    pub setup: Setup,
    /// 0x59b760 / 0x59a0d4: per race (0 zerg, 1 terran, 2 protoss) the campaign select's
    /// advice code (0 none, -10 .. -12), for the classic and the Brood War screen.
    campaign_advice: [[i32; 3]; 2],
    /// 0x6dc2ac: the glue sound ids (0xa5 .. 0xad) once 0x4de0b0 set them (0x6d6364).
    sound_list: Option<[u16; 9]>,
    /// 0x6d6368: tick of the last hover sound.
    sound_tick: u32,
    /// GetTickCount stand-in: the sum of the `tick` steps.
    clock_ms: u32,
    /// 0x6d5a60: when set, 0x4b4af0 uses layout 1 (zerg victory) whatever the race.
    g6d5a60: u32,
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
    /// 0x6d11cc: the campaign mission record (a VA into the exe's 8-byte records of
    /// 0x5122a0 / 0x5122ac: +2 mission, +4 scenario number, +6 race), 0 none.
    camp_rec: u32,
    /// 0x51ca1c: a campaign map continues (set by 0x4a8590).
    camp_cont: bool,
    /// 0x5122b8: the scenario (cinematic) number 0x4e0820 case 10 plays.
    scenario: u32,
    /// 0x6cef50: a UI notice rectangle is pending (0x6cef52 / 54 / 56 / 58: its four words,
    /// 0x6cef51: flags).
    notice: (bool, u8, [i16; 4]),
    /// 0x51a0e9: the palette is blanked (set by 0x4dcee0).
    pal_blank: bool,
    /// 0x6d5a6c: UI flags (0x20000000: no mission text screens).
    g6d5a6c: u32,
    /// The install's StarCraft.exe: the campaign tables are read from its data.
    exe: Vec<u8>,
    /// arr\mapdata.tbl (0x59c080): the campaign map names by mission number.
    map_names: Vec<String>,
}

impl Glue {
    /// 0x4e0820 (before its loop): state 4 (glue), settings, cursor at (320, 240).
    pub fn new(files: &Storage) -> Glue {
        todo_port!("0x4e0820", "0x4daf30 / 0x4d2ff0 / 0x4e05b0 / 0x4db060 / 0x4f6100 init, CPUThrottle");
        let network_raw = files.read("rez/network.tbl").unwrap_or_default();
        let network = if network_raw.is_empty() { Vec::new() } else { crate::dat::parse_tbl(&network_raw) };
        let glu_all_raw = files.read("rez/gluAll.tbl").unwrap_or_default();
        let glu_all = if glu_all_raw.is_empty() { Vec::new() } else { crate::dat::parse_tbl(&glu_all_raw) };
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
            network,
            network_raw,
            glu_all,
            glu_all_raw,
            setup: Setup::default(),
            campaign_advice: [[0; 3]; 2],
            sound_list: None,
            sound_tick: 0,
            clock_ms: 0,
            g6d5a60: 0,
            mouse: (320, 240),
            shown: -1,
            looks: (0..PALETTES.len()).map(|_| None).collect(),
            cursor_frame: 0,
            cursor_ms: 0,
            error_depth: 0,
            camp_rec: 0,
            camp_cont: false,
            scenario: 0,
            notice: (false, 0, [0; 4]),
            pal_blank: false,
            g6d5a6c: 0,
            exe: std::fs::read(crate::install_dir().join("StarCraft.exe")).unwrap_or_default(),
            map_names: files.read("arr/mapdata.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default(),
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
                todo_port!("0x4f7ae0", "play the scenario cinematic 0x5122b8");
                self.scenario = 0x1c;
                if self.state == state::AFTER_GAME {
                    self.campaign_next(files, true);
                }
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
    fn glue_enter(&mut self, files: &Storage) {
        self.glue_sound_init();
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
            // network.tbl word 0x48 (string index 0x47), else the default at 0x501b7d.
            self.player_name = self.network.get(0x47).cloned().unwrap_or_default();
            self.player_name.truncate(0x18);
        }
        self.expansion = self.direct_map;
        // 0x57f1e3 = 0.
        // 0x4db200 (campaign .bin present; it returns 1, so AX = 1) and 0x4dbfc0 (start the
        // mission) pick ready room 8 / 7 / 9 by 0x51c608 and set 0x6d11c0; 0x596904 = 1 with
        // 0x6d5a6c & 0x20000000.
        if (!self.direct_map || self.expansion_installed) && self.campaign_bin_present(files) && self.start_mission(files, 1) {
            todo_port!("0x4de200", "ready room by 0x51c608, 0x6d11c0 = 1, 0x596904 = 1 (0x6d5a6c & 0x20000000)");
            return;
        }
        // Their failure path.
        self.screen = screen::MAIN;
        self.expansion = false;
    }

    /// One pass of 0x4de200's `while (0x596904 == 4) switch (0x6d11bc)` (the glue loop),
    /// entering it first when it was not running.
    fn glue_step(&mut self, files: &Storage) {
        if self.state != state::GLUE {
            self.glue_enter(files);
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
            screen::LOGIN => self.open_login(files),
            screen::CAMPAIGN => self.open_campaign(files, false),
            screen::READY_Z => self.open_ready(files, 0),
            screen::READY_T => self.open_ready(files, 1),
            screen::READY_P => self.open_ready(files, 2),
            screen::CREATE => self.open_create(files, 0),
            screen::CREATE2 => self.open_create(files, 1),
            screen::LOAD => self.open_load(files),
            screen::SCORE_FIRST..=screen::SCORE_LAST => self.open_score(files),
            screen::CAMPAIGN2 => self.open_campaign(files, true),
            screen::EXIT => self.glue_exit(),
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
        // The event proc 0x4db7a0: case 10 (0x4dd9e0), case 0 (0x4db280 + 0x4de150 + title
        // music), the dialog's init.
        let mut dlg = self.open_glue_dialog(files, "rez/gluMain.bin");
        self.main_version_label(&mut dlg);
        self.show_palette();
        todo_port!("0x4a6310", "title music (unless 0x6d5bbc)");
        self.dlg = Some((dlg, Open::Main));
    }

    /// The start every glue screen shares: the .bin (0x4ddbe0), +0x18 |= 4, 0x4194e0 (the
    /// images: eTail, pIntro, pCredit), then 0x41a080's event 10 to the controls (the
    /// light-up buttons open their videos) and to the event proc (0x4dd9e0).
    fn open_glue_dialog(&mut self, files: &Storage, bin: &str) -> Dialog {
        let Ok(d) = files.read(bin) else { crate::bw::fatal_error(bin) };
        let mut dlg = Dialog::parse(&d);
        dlg.this.flags |= 4;
        dlg.bind(files);
        dlg.init_lightups(files);
        self.glue_dialog_init(files, &mut dlg, true);
        dlg
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

    /// 0x4b26e0 (classic) / 0x4b5050 (Brood War): the advice for each campaign button from
    /// the player's profile: -10 when the race's first campaign was not started, -11 / -12
    /// when the other campaigns are not done (the messages network.tbl 0x8d ..).
    fn campaign_precheck(&mut self, exp: bool) {
        self.campaign_advice[exp as usize] = [0; 3];
        self.setup.multiplayer = self.multiplayer;
        let (p, ok) = self.setup.load_profile(self.player_name.as_bytes());
        if !ok {
            let default = self.network.get(0x47).map(String::as_str).unwrap_or_default();
            // 0x6d5a10 is 0 here (0x4de200's main menu case clears it).
            if !self.player_name.eq_ignore_ascii_case(default) {
                todo_port!("0x4bb300", "error popup (string 0x58)");
            }
            return;
        }
        let w = |o: usize| u32::from_le_bytes(p.0[o..o + 4].try_into().unwrap());
        // (the three best-mission fields, the limits, the buttons' indices).
        let (c, e, d, lim1, lim2, ia, ib) = if exp {
            (w(0x38), w(0x3c), w(0x34), 0x27, 0x30, 1, 0)
        } else {
            (w(0x28), w(0x2c), w(0x30), 0xb, 0x15, 0, 2)
        };
        let a = &mut self.campaign_advice[exp as usize];
        if c == 0 && e <= lim1 {
            a[ia] = -10;
        }
        if d == 0 {
            if e <= lim1 {
                a[ib] = -11;
                return;
            }
            if c <= lim2 {
                a[ib] = -12;
            }
        }
    }

    /// 0x4b2930 (`exp` false, rez\gluCmpgn.bin, event proc 0x4b28b0) / 0x4b52a0 (rez\
    /// gluExpCmpgn.bin, 0x4b5220), before 0x41a080: the campaign select.
    fn open_campaign(&mut self, files: &Storage, exp: bool) {
        // 0x6d11c0 = 0, 0x57f0b4 = 0.
        self.multiplayer = false;
        self.campaign_precheck(exp);
        let bin = if exp { "rez/gluExpCmpgn.bin" } else { "rez/gluCmpgn.bin" };
        let dlg = self.open_glue_dialog(files, bin);
        todo_port!("0x4dd5e0", "slide-in of the buttons (lists 0x512b10 / 0x512a8c) and its timer 0x4dd220");
        self.show_palette();
        todo_port!("0x4a6310", "title music (unless 0x6d5bbc)");
        self.dlg = Some((dlg, if exp { Open::CampaignExp } else { Open::Campaign }));
    }

    /// 0x4b2930 / 0x4b52a0 (after 0x41a080): act on the result.
    fn done_campaign(&mut self, result: i32) {
        self.screen = match result {
            5 => screen::LOAD,
            6 => screen::READY_P,
            7 => screen::READY_T,
            8 => screen::READY_Z,
            9 => screen::LOGIN,
            10 => screen::CREATE,
            11 => screen::CREATE2,
            _ => screen::MAIN,
        };
        self.glue_close();
    }

    /// 0x4b2810 (classic) / 0x4b5180 (Brood War): the campaign select's event proc for an
    /// activated control: ids 6 / 7 / 8 (protoss, terran, zerg) start their campaign
    /// (0x4b27a0 / 0x4b5110: the advice confirmation, then 0x4dc0b0) and end the dialog on
    /// success.
    fn campaign_activate(&mut self, files: &Storage, id: i16, exp: bool) -> bool {
        self.activated = id;
        let idx = match id {
            6 => 2,
            7 => 1,
            8 => 0,
            _ => return self.end_dialog(id as i32),
        };
        if !self.campaign_check(files, idx, exp) {
            return true;
        }
        self.end_dialog(id as i32)
    }

    /// 0x4b27a0 / 0x4b5110 (EAX = idx: 0 zerg, 1 terran, 2 protoss): with advice for the
    /// button, ask first (0x4b5b20, the text network.tbl 0x8d / 0x8c (classic) or
    /// 0x90 / 0x8f (Brood War), 1-based); then start the campaign.
    fn campaign_check(&mut self, files: &Storage, idx: usize, exp: bool) -> bool {
        if self.campaign_advice[exp as usize][idx] != 0 {
            let k = if exp { 0x8e + (idx != 1) as usize } else { 0x8c + (idx == 2) as usize };
            let _text = self.network.get(k).cloned().unwrap_or_default();
            todo_port!("0x4b5b20", "OK / Cancel confirmation (GLUEOKCANCEL_BIN) with the advice; taken as OK");
        }
        self.campaign_start(files, idx)
    }

    /// 0x4dc0b0 (EDI = idx): the player's profile (created when missing for the default
    /// name), the mission list for the race's best mission, then the chosen mission:
    /// with a scenario number the next game state, else 0x4dbfc0.
    fn campaign_start(&mut self, files: &Storage, idx: usize) -> bool {
        // 0x57f1e3 = 0.
        self.camp_cont = false;
        self.setup.multiplayer = self.multiplayer;
        let name = self.player_name.clone();
        let (mut p, ok) = self.setup.load_profile(name.as_bytes());
        if !ok {
            let default = self.network.get(0x47).map(String::as_str).unwrap_or_default();
            let created = if name.eq_ignore_ascii_case(default) { self.setup.new_profile(name.as_bytes(), &self.glu_all_raw) } else { None };
            match created {
                Some(n) => p = n,
                // 0x6d5a10 is 0 here.
                None => todo_port!("0x4bb300", "error popup (string 0x58)"),
            }
        }
        // 0x4b7820 returns the chosen record (a VA into the exe's tables), 0 for cancel.
        let rec: u32 = {
            todo_port!("0x4b7820", "mission list dialog (gluHist.tbl / .bin, list 0x5122a0 / 0x5122ac [idx]) -> the chosen record");
            0
        };
        self.camp_rec = rec;
        if rec == 0 {
            return false;
        }
        let (mission, scenario) = (self.exe_u16(rec + 2), self.exe_u16(rec + 4));
        if p.best(self.expansion, idx as u8) < mission as u32 {
            p.set_best(self.expansion, idx as u8, mission as u32);
            self.setup.write_profile(&p);
        }
        if scenario != 0 {
            self.scenario = scenario as u32;
            self.setup.map_dir_flag = mission;
            self.setup.next_map.clear();
            self.state = state::AFTER_GAME;
            return true;
        }
        self.start_mission(files, mission)
    }

    /// The exe's initialized data: .rdata / .data sit at file offset VA - 0x400000.
    fn exe_u8(&self, va: u32) -> u8 {
        va.checked_sub(0x40_0000).and_then(|o| self.exe.get(o as usize)).copied().unwrap_or(0)
    }

    fn exe_u16(&self, va: u32) -> u16 {
        u16::from_le_bytes([self.exe_u8(va), self.exe_u8(va + 1)])
    }

    fn exe_u32(&self, va: u32) -> u32 {
        u32::from(self.exe_u16(va)) | u32::from(self.exe_u16(va + 2)) << 16
    }

    /// 0x4dbc20 (ESI = table 0x5122a0 / 0x5122ac, DI = mission): the first record without a
    /// scenario number (+4 = 0) for the mission in the table's three race lists (zerg,
    /// terran, protoss; records of 8 bytes ending at +2 = 0); 0 when none.
    fn camp_find(&self, table: u32, mission: u16) -> u32 {
        for k in 0..3 {
            let mut r = self.exe_u32(table + 4 * k);
            while self.exe_u16(r + 2) != 0 {
                if self.exe_u16(r + 4) == 0 && self.exe_u16(r + 2) == mission {
                    return r;
                }
                r += 8;
            }
        }
        0
    }

    /// 0x4dbd60: 0x6d11cc = the record of the current mission 0x57f244 unless it already is
    /// that one: the classic lists first, then the Brood War ones.
    fn camp_select(&mut self) {
        let mission = self.setup.map_dir_flag;
        if self.camp_rec != 0 && self.exe_u16(self.camp_rec + 2) == mission {
            return;
        }
        self.camp_rec = self.camp_find(0x5122a0, mission);
        if self.camp_rec == 0 {
            self.camp_rec = self.camp_find(0x5122ac, mission);
        }
    }

    /// 0x4dbd20 (ESI = pointer to the index counter, EBX = file name, EDI = the length of
    /// its base name): advance the counter to the next entry (< 0x41) of the map name table
    /// 0x59c080 whose name after its last backslash (SStrChrR, ordinal 572; the table has
    /// "campaign\\terran\\terran01") begins with the base name
    /// (SStrCmpI, ordinal 509, over `len` characters). False when the table ends.
    fn camp_next_name(&self, idx: &mut usize, name: &[u8], len: usize) -> bool {
        while *idx < 0x41 {
            if let Some(s) = self.map_names.get(*idx) {
                if let Some(bs) = s.bytes().rposition(|c| c == b'\\') {
                    let tail = s.as_bytes()[bs + 1..].iter().copied().chain(std::iter::repeat(0));
                    let want = name.iter().copied().chain(std::iter::repeat(0));
                    if tail.zip(want).take(len).all(|(a, b)| a.eq_ignore_ascii_case(&b)) {
                        return true;
                    }
                }
            }
            *idx += 1;
        }
        false
    }

    /// 0x4dbda0 (EAX = a file name, 0x57f246; ordinal 572 finds the last '.'): the record of the campaign map called that.
    /// The extension (".SCM", else Brood War's lists) picks the table; the map is found in
    /// 0x59c080 by its base name and its index is the mission to look up.
    fn camp_lookup(&self, name: &[u8]) -> u32 {
        let Some(dot) = name.iter().rposition(|&c| c == b'.') else { return self.camp_rec };
        let table = if name[dot..].eq_ignore_ascii_case(b".SCM") { 0x5122a0 } else { 0x5122ac };
        let mut idx = 0;
        while self.camp_next_name(&mut idx, name, dot) {
            let r = self.camp_find(table, idx as u16);
            if r != 0 {
                return r;
            }
            idx += 1;
        }
        self.camp_rec
    }

    /// 0x4dc1c0: continue with the next scenario named by 0x57f246 (set by the "next
    /// scenario" trigger action): load it from the directory of the current map through
    /// 0x4a8590 and open the ready room of the local slot's race. Without a next map, or
    /// when that fails (popup 0x6a), back to the campaign select (Brood War: +0x10).
    /// True when a ready room was picked.
    fn campaign_continue(&mut self, files: &Storage) -> bool {
        let back = if self.expansion { 0x16 } else { 6 };
        if self.setup.next_map.first().copied().unwrap_or(0) == 0 {
            self.screen = back;
            return true;
        }
        let (mut a, mut b) = ([0u8; 0x20], [0u8; 0x20]);
        if Setup::load_got(files, "Use Map Settings(1)", &mut a, &mut b).is_some() {
            let mp = self.setup.map_path;
            let mp = &mp[..mp.iter().position(|&c| c == 0).unwrap_or(mp.len())];
            if let Some(sep) = mp.iter().rposition(|&c| c == b'\\') {
                let mut path = mp[..=sep].to_vec();
                path.extend_from_slice(&self.setup.next_map);
                // 0x4a8590 (EAX = path, ECX = the template's first 4 bytes, ESI = 0x57ee9c,
                // stack: 0x6cdfd4) sets 0x51ca1c, picks the slot (0x4dbe50) and starts the
                // game (0x4d3fc0); 0 is success.
                let r: i32 = {
                    todo_port!("0x4a8590", "start the next campaign map (load header, game description, 0x4a68d0, 0x4dbe50, 0x4d3fc0)");
                    -1
                };
                if r == 0 {
                    let race = self.setup.slots[self.setup.local_slot[0] as usize % 12][slot::RACE];
                    match race {
                        0 => self.screen = screen::READY_Z,
                        1 => self.screen = screen::READY_T,
                        2 => self.screen = screen::READY_P,
                        _ => {}
                    }
                    if race < 3 {
                        return true;
                    }
                }
            }
        }
        // 0x6d5a10 is 0 here.
        todo_port!("0x4bb300", "error popup (string 0x6a)");
        self.screen = back;
        false
    }

    /// 0x4dc320 (`flag` = the stack argument): what comes after a game or a scenario. A
    /// continued map goes through 0x4dc1c0; `flag` 0 starts a game (state 5); otherwise the
    /// mission text (0x4d91b0), the next record of the campaign (0x57f246 names a jump,
    /// else the following record) with the profile's progress (0x4dbee0), then the end of
    /// the list (credits state 9 / the campaign select), a scenario (state 10) or the next
    /// mission's ready room. False when it could not start the mission.
    fn campaign_next(&mut self, files: &Storage, flag: bool) -> bool {
        self.state = state::GLUE;
        if self.camp_cont {
            self.campaign_continue(files);
            return true;
        }
        if !flag {
            self.state = 5;
            return true;
        }
        self.mission_text(false);
        self.camp_select();
        if self.camp_rec == 0 || self.exe_u16(self.camp_rec + 2) == 0 {
            return false;
        }
        if self.setup.next_map.first().copied().unwrap_or(0) != 0 {
            let name = self.setup.next_map.clone();
            self.camp_rec = self.camp_lookup(&name);
            self.setup.next_map.clear();
        } else {
            self.camp_rec += 8;
        }
        let rec = self.camp_rec;
        let (mission, race) = (self.exe_u16(rec + 2), self.exe_u8(rec + 6));
        self.setup.record_mission(mission, race, &self.network_raw, &self.glu_all_raw);
        if mission == 0 {
            // The end of the list.
            if !self.expansion {
                if race != 2 {
                    self.screen = screen::CAMPAIGN;
                    self.camp_rec = 0;
                    return true;
                }
            } else if race != 0 {
                self.screen = screen::CAMPAIGN2;
                self.camp_rec = 0;
                return true;
            }
            self.state = state::CREDITS_EXP;
            self.camp_rec = 0;
            return true;
        }
        let scenario = self.exe_u16(rec + 4);
        if scenario != 0 {
            self.setup.map_dir_flag = mission;
            self.scenario = scenario as u32;
            self.state = state::AFTER_GAME;
            return true;
        }
        if !self.start_mission(files, mission) {
            return false;
        }
        self.screen = match self.exe_u8(self.camp_rec + 6) {
            0 => screen::READY_Z,
            1 => screen::READY_T,
            _ => screen::READY_P,
        };
        true
    }

    /// 0x4d91b0 (`exp` false: the one record of 0x500160, "FinZ09bx" for mission 0x3f) /
    /// 0x4d9200 (`exp`: the 0x40 records of 0x4fff60, "Est..." per mission): when a
    /// single-player game ran a mission and 0x6d5a6c lacks 0x20000000, show the mission's
    /// text screen (rez\<name>.txt on rez\credits.bin, 0x4d8df0).
    fn mission_text(&mut self, exp: bool) {
        let mission = self.setup.map_dir_flag;
        if self.setup.multiplayer || self.g6d5a6c & 0x2000_0000 != 0 || mission == 0 {
            return;
        }
        let (table, n) = if exp { (0x4fff60, 0x40) } else { (0x500160, 1) };
        for i in 0..n {
            if self.exe_u16(table + 8 * i + 4) == mission {
                let ptr = self.exe_u32(table + 8 * i);
                let name: Vec<u8> = (0..).map(|k| self.exe_u8(ptr + k)).take_while(|&c| c != 0).collect();
                let _ = name;
                todo_port!("0x4d8df0", "mission text / credits screen: rez\\<name>.txt on rez\\credits.bin, event proc 0x4d8d60 (scroll 0x4d8bd0, 0x4d8c60)");
                return;
            }
        }
    }

    /// 0x4db200: whether the campaign dialog (rez\glucmpgn.bin, or gluexpcmpgn.bin for
    /// Brood War) can be opened from the archives.
    fn campaign_bin_present(&self, files: &Storage) -> bool {
        files.read(if self.expansion { "rez/gluexpcmpgn.bin" } else { "rez/glucmpgn.bin" }).is_ok()
    }

    /// 0x4dbfc0 (AX = mission): start the campaign mission: 0x57f244 = AX, its map (name
    /// 0x59c080[AX]) loaded by 0x4bf5d0, the game description from the player name and the
    /// map title, the template (0x4ab750), the local slot (0x4dbe50), then 0x4d3fc0. False
    /// when it cannot.
    fn start_mission(&mut self, files: &Storage, mission: u16) -> bool {
        self.setup.map_dir_flag = mission;
        let path = self.map_names.get(mission as usize).cloned().unwrap_or_default();
        let mut hdr = [0u8; 0x20];
        if !self.setup.load_map(files, &path, &mut hdr, true, &self.glu_all_raw) {
            return false;
        }
        let (mut a, mut b) = ([0u8; 0x20], [0u8; 0x20]);
        if Setup::load_got(files, "Use Map Settings(1)", &mut a, &mut b).is_none() {
            return false;
        }
        if !self.setup.local_slot_setup() {
            return false;
        }
        // 0x596888 = 0.
        todo_port!("0x4d3fc0", "create the single-player game from the description (name, map title, template, settings 0x6cdfd4)");
        false
    }

    /// 0x4f71f0 (`race` 0 zerg, rez\glurdyz.bin, event proc 0x4f7150) / 0x4f7550 (1 terran,
    /// glurdyt.bin, 0x4f74b0) / 0x4f78b0 (2 protoss, glurdyp.bin, 0x4f7810), before 0x41a080:
    /// the ready room (mission briefing). The music of the race (0x4a6310: 0xa + race)
    /// after the previous one fades out (0x4a5f50) and the mission text (0x4d9200).
    fn open_ready(&mut self, files: &Storage, race: u8) {
        todo_port!("0x4a5f50", "fade out the music (audio.rs)");
        todo_port!("0x4a6310", "ready room music 0xa + race");
        self.mission_text(true);
        if self.state != state::GLUE {
            return;
        }
        self.shown = -1;
        let bin = ["rez/glurdyz.bin", "rez/glurdyt.bin", "rez/glurdyp.bin"][race as usize];
        let dlg = self.open_glue_dialog(files, bin);
        // The event proc's case 10: 0x46d3c0 (briefing init), the slide-in 0x4dd5e0 (list
        // 0x512a70 / 0x512a54 / 0x512a14) and 0x4dd9e0; case 0: 0x4de150.
        todo_port!("0x46d3c0", "briefing init; 0x4f70f0 / 0x4f7450 / 0x4f77b0 frame images on event 0x405");
        todo_port!("0x4dd5e0", "slide-in of the buttons and its timer 0x4dd220");
        self.show_palette();
        self.dlg = Some((dlg, Open::Ready(race)));
    }

    /// 0x4f71f0 / 0x4f7550 / 0x4f78b0 (after 0x41a080): 0xe (back) goes to the campaign
    /// select (Brood War +0x10) or, in a network game, to the screen of the provider
    /// (0x4dc5b0); 0x13 (next) is 0x4dc320(1); 0x65 starts the game (state 1); 100 stays;
    /// anything else goes to the main menu. All but 0x13 / 0x65 then fade the music
    /// (0x4a5f50) and play 0x16 (0x4a6310).
    fn done_ready(&mut self, files: &Storage, result: i32, _race: u8) {
        match result {
            0x13 => {
                self.campaign_next(files, true);
                self.glue_close();
                return;
            }
            0x65 => {
                self.state = state::START;
                self.glue_close();
                return;
            }
            0xe if self.setup.multiplayer => self.net_screen(),
            0xe => self.screen = if self.expansion { screen::CAMPAIGN2 } else { screen::CAMPAIGN },
            100 => {}
            _ => self.screen = screen::MAIN,
        }
        todo_port!("0x4a5f50", "fade out the music (audio.rs)");
        todo_port!("0x4a6310", "music 0x16");
        self.glue_close();
    }

    /// 0x4dc5b0: the screen of the network provider 0x59688c: 'NXPN' ... 10 (game list),
    /// 'BNET' 4 (battle.net), modem 0x14, 'LBCS' direct 0x15, anything else the main menu.
    fn net_screen(&mut self) {
        self.screen = match self.setup.net_provider {
            0x4d444d58 | 0x4d4f444d => screen::MODEM,
            0x4950584e | 0x41544c4b | 0x49505858 | 0x5544504e => screen::GAMESEL,
            0x5343424c => screen::DIRECT,
            0x424e4554 => screen::BATTLE_NET,
            _ => screen::MAIN,
        };
    }

    /// 0x4edc30 (rez\gluLogin.bin, event proc 0x4edaa0), before 0x41a080: the profile
    /// login.
    fn open_login(&mut self, files: &Storage) {
        let dlg = self.open_glue_dialog(files, "rez/gluLogin.bin");
        self.show_palette();
        self.dlg = Some((dlg, Open::Login));
    }

    /// 0x4edc30 (after 0x41a080): 4 (ok) goes on: single player to the campaign select
    /// (Brood War +0x10), network to the provider's screen (modem 0x14, direct 0x15, else
    /// the game list 10); 5 (cancel) 0x4c4680 then the connection select (2) in a network
    /// game, else the main menu; anything else the main menu.
    fn done_login(&mut self, result: i32) {
        match result {
            4 if !self.setup.multiplayer => self.screen = if self.expansion { screen::CAMPAIGN2 } else { screen::CAMPAIGN },
            4 => {
                self.screen = match self.setup.net_provider {
                    0x4d444d58 | 0x4d4f444d => screen::MODEM,
                    0x5343424c => screen::DIRECT,
                    _ => screen::GAMESEL,
                }
            }
            5 => {
                todo_port!("0x4c4680", "login cancel cleanup");
                self.screen = if self.setup.multiplayer { screen::SELCONN } else { screen::MAIN };
            }
            _ => self.screen = screen::MAIN,
        }
        self.glue_close();
    }

    /// 0x4af6d0 (EAX = `which`, 0x59b844: 0 from screen 0xb, 1 from 0xc), before 0x41a080:
    /// the create game screen, rez\gluCustm.bin (single player) or gluCreat.bin (network)
    /// with event proc 0x4af5c0.
    fn open_create(&mut self, files: &Storage, which: u8) {
        let bin = if self.setup.multiplayer { "rez/gluCreat.bin" } else { "rez/gluCustm.bin" };
        let dlg = self.open_glue_dialog(files, bin);
        // 0x59ba60 = 0x4d2b30 (the map list buffer), 0x6d5a74 = 1.
        todo_port!("0x4d2b30", "map list buffer of the create game screen");
        self.show_palette();
        self.dlg = Some((dlg, Open::Create(which)));
    }

    /// 0x4af6d0 (after 0x41a080): 0xc (play) picks the ready room of the local slot's race
    /// (single player with plain settings) or starts the game (state 1), in a network game
    /// goes to the chat (3); anything but 0xd / -3 (cancel) goes to the main menu; cancel
    /// drops the mission map and goes back (campaign select, or the provider's screen).
    fn done_create(&mut self, result: i32, _which: u8) {
        match result {
            0xc => {
                if self.setup.multiplayer {
                    self.screen = screen::CHAT;
                } else if self.setup.template[8] == 0 && self.setup.template[0xc] == 0 && self.setup.template[0x12] == 0 && !self.setup.mission {
                    match self.setup.slots[self.setup.local_slot[0] as usize % 12][slot::RACE] {
                        0 => self.screen = screen::READY_Z,
                        1 => self.screen = screen::READY_T,
                        2 => self.screen = screen::READY_P,
                        _ => {}
                    }
                } else {
                    self.state = state::START;
                }
            }
            -3 | 0xd => {
                self.setup.mission = false;
                self.setup.mission_chk = Vec::new();
                self.screen = if !self.setup.multiplayer {
                    if self.expansion { screen::CAMPAIGN2 } else { screen::CAMPAIGN }
                } else {
                    match self.setup.net_provider {
                        0x424e4554 => screen::BATTLE_NET,
                        0x4d4f444d | 0x4d444d58 => screen::MODEM,
                        0x5343424c => screen::DIRECT,
                        _ => screen::GAMESEL,
                    }
                };
            }
            _ => self.screen = screen::MAIN,
        }
        self.glue_close();
        // 0x59ba60 is freed, 0x6d5a74 = 0.
    }

    /// 0x4b4af0 (rez\gluScore.bin, event proc 0x4b4a30), before 0x41a080: the score screen
    /// after a game. 0x4c4220 gives the outcome; the screen (0x512ab0: 0xe .. 0x13), the
    /// image directory (0x512a98: glue\score{Z,T,P}{d,v}\) and the music (0x512ac8) are
    /// picked by the local slot's race and victory (index 1 with 0x6d5a60 set).
    fn open_score(&mut self, files: &Storage) {
        let (_, local) = self.setup.player_outcomes(self.clock_ms);
        let won = local == 1;
        let i = if self.g6d5a60 == 0 {
            let race = self.setup.slots[self.setup.local_slot[0] as usize % 12][slot::RACE] as u32;
            race * 2 + won as u32
        } else {
            1
        };
        self.screen = self.exe_u32(0x512ab0 + 4 * i) as u8;
        todo_port!("0x4a6310", "score music (0x512ac8[i])");
        // iScore.grp and tminimap.pcx from the directory 0x512a98[i] (0x4d2b30 buffer,
        // SBmpAllocLoadImage), then 0x4bca80.
        todo_port!("0x4bca80", "score screen images (iScore.grp, tMinimap.pcx)");
        let dlg = self.open_glue_dialog(files, "rez/gluScore.bin");
        self.show_palette();
        self.dlg = Some((dlg, Open::Score(won)));
    }

    /// 0x4b4af0 (after 0x41a080): 7 (continue) goes to the provider's screen in a network
    /// game, else on with the campaign (0x4dc320 with the victory flag); anything else
    /// (or a campaign that cannot go on) records the progress (0x4dbee0) of a single-player
    /// game that is not a continued map and goes to the main menu. Then the music fades.
    fn done_score(&mut self, files: &Storage, result: i32, won: bool) {
        let mut done = false;
        if result == 7 {
            if self.setup.multiplayer {
                self.net_screen();
                done = true;
            } else {
                done = self.campaign_next(files, won);
            }
        } else if !self.setup.multiplayer && !self.camp_cont {
            self.camp_select();
            if self.camp_rec != 0 && self.exe_u16(self.camp_rec + 2) != 0 {
                let (mission, race) = (self.exe_u16(self.camp_rec + 2), self.exe_u8(self.camp_rec + 6));
                self.setup.record_mission(mission, race, &self.network_raw, &self.glu_all_raw);
            }
        }
        if !done {
            self.screen = screen::MAIN;
        }
        self.glue_close();
        todo_port!("0x4a5f50", "fade out the music (audio.rs)");
    }

    /// 0x4b6490 (rez\gluLoad.bin, event proc 0x4b6410), before 0x41a080: the load game
    /// screen.
    fn open_load(&mut self, files: &Storage) {
        let dlg = self.open_glue_dialog(files, "rez/gluLoad.bin");
        todo_port!("0x4dd5e0", "slide-in of the buttons (list 0x512a48) and its timer 0x4dd220");
        self.show_palette();
        self.dlg = Some((dlg, Open::Load));
    }

    /// 0x4b6490 (after 0x41a080): 4 loads the game chosen (0x4cf950), 5 goes back to the
    /// campaign select (Brood War: +0x10), anything else to the main menu.
    fn done_load(&mut self, result: i32) {
        match result {
            4 => todo_port!("0x4cf950", "load the chosen saved game"),
            5 => self.screen = if self.expansion { screen::CAMPAIGN2 } else { screen::CAMPAIGN },
            _ => self.screen = screen::MAIN,
        }
        self.glue_close();
    }

    /// 0x4b6410: the load screen's event proc for an activated control: 7 (the game list)
    /// opens 0x4b62a0, any other ends the dialog.
    fn load_activate(&mut self, id: i16) -> bool {
        self.activated = id;
        if id == 7 {
            todo_port!("0x4b62a0", "saved game list dialog (0x4b6010)");
            return true;
        }
        self.end_dialog(id as i32)
    }

    /// 0x4be0b0: flush the pending notice: flags bit 0 is set and 0x41e0d0 (the UI engine,
    /// bucketed "replaced") gets (0x6cef58 + 0x6cef54 - 1, 0x6cef56 + 0x6cef52 - 1).
    fn flush_notice(&mut self) {
        if self.notice.0 {
            self.notice.1 |= 1;
        }
        self.notice.0 = false;
    }

    /// 0x4dcee0: one-time init of the dialog palette: blank the fade table (0x6ceb40), copy
    /// the palette 0x6ce320 to 0x6ce720, 0x41e8b0 and 0x41ca00 (UI engine, "replaced"), then
    /// flush the notice (0x4be0b0).
    fn glue_palette_init(&mut self) {
        if !self.pal_blank {
            self.pal_blank = true;
        }
        self.flush_notice();
    }

    /// 0x4dcc90: the exit screen: back to the main menu; an open popup or error dialog
    /// (0x6d5a3c / 0x6d5a58) and the screen's dialog (0x51c418) are destroyed (0x419ea0)
    /// with 0x6d051c = 3 (the popup's last button 0x6d5a54, else 3) or -1.
    fn glue_exit(&mut self) {
        self.screen = screen::MAIN;
        self.activated = -1;
        if self.popup.take().is_some() {
            self.activated = 3;
            self.error_depth = 0;
        }
        self.dlg = None;
        self.result = None;
    }

    /// 0x4de0b0: the glue sounds: the list 0x6dc2ac = 0xa5 .. 0xad (once), the sound
    /// hook 0x6d5eac = 0x4dd1d0.
    fn glue_sound_init(&mut self) {
        self.sound_list.get_or_insert([0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad]);
        todo_port!("0x4bccf0", "play / preload the sound list");
    }

    /// 0x4dd1d0: the dialogs' sound hook: event 0 (hover) is sound 0xa6 at most every
    /// 200 ms (`now` is GetTickCount), event 1 (click) 0xa5. Returns the sound to play.
    fn glue_sound_hook(&mut self, event: i32, now: u32) -> Option<u16> {
        match event {
            0 if now.wrapping_sub(self.sound_tick) > 199 => {
                self.sound_tick = now;
                Some(0xa6)
            }
            1 => Some(0xa5),
            _ => None,
        }
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
                    // popup with network.tbl string 0x66 (0-based), else the empty default.
                    let msg = self.network.get(0x66).cloned().unwrap_or_default();
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
            Open::Campaign => {
                self.campaign_activate(files, id, false);
            }
            Open::CampaignExp => {
                self.campaign_activate(files, id, true);
            }
            Open::Load => {
                self.load_activate(id);
            }
            Open::Score(_) => {
                self.activated = id;
                todo_port!("0x4b4a30", "score screen event proc (tabs, 0x4b4600 / 0x4b4a00 init, continue)");
            }
            Open::Create(_) => {
                self.activated = id;
                todo_port!("0x4af5c0", "create game event proc (map list, game type, options)");
            }
            Open::Login => {
                self.activated = id;
                todo_port!("0x4edaa0", "login event proc (profile list, create / delete, ok / cancel)");
            }
            Open::Ready(_) => {
                // The event proc's activation (event 0xe code 2) is 0x46d160.
                self.activated = id;
                todo_port!("0x46d160", "ready room button events (briefing / start / cancel)");
            }
            _ => {}
        }
        if let Some(r) = self.result.take() {
            let (_, open) = self.dlg.take().unwrap();
            match open {
                Open::Main => self.done_main(r),
                Open::Campaign | Open::CampaignExp => self.done_campaign(r),
                Open::Load => self.done_load(r),
                Open::Score(won) => self.done_score(files, r, won),
                Open::Create(which) => self.done_create(r, which),
                Open::Login => self.done_login(r),
                Open::Ready(race) => self.done_ready(files, r, race),
                _ => {}
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
        self.clock_ms = self.clock_ms.wrapping_add(ms);
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
