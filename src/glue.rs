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
use crate::setup::{slot, sstr_copy, Profile, Setup};
use crate::storage::Storage;
use crate::palette::{self, Pal};
use crate::todo_port;

#[path = "glue_create.rs"]
mod create;
#[path = "glue_score.rs"]
mod score;
#[path = "glue_credits.rs"]
mod credits;
#[path = "glue_ready.rs"]
mod ready;
#[path = "glue_cine.rs"]
mod cine;
#[path = "glue_slide.rs"]
mod slide;

/// What the glue asks the sound system for (the window loop plays them: `Audio` is not
/// the glue's).
#[derive(Clone, Debug, PartialEq)]
pub enum AudioCmd {
    /// 0x4bc2c0: sound `id` at the digital volume (`Audio::play_pan(id, 99, 0, None)`).
    Sound(u16),
    /// 0x4a6140 (via 0x4a6310): start a music file, looped (type 1 tracks); `fade` is the
    /// 20 ms fade-in timer 0x4a6030 (attenuation -10000 rising 400 per step).
    Music { path: &'static str, fade: bool },
    /// 0x4a6140 for the in-game tracks 1 ..= 9 of 0x512bb8 (type 2: played once).
    Track(u32),
    /// 0x4a5f50.
    FadeOutMusic,
    /// 0x4bbf50: the playing sounds stop.
    StopSounds,
    /// 0x4bc490: a whole wav file (the briefing's) plays once.
    Wav(Vec<u8>),
    /// 0x4015f0: the wav files playing stop.
    StopWavs,
}

/// 0x512bb8 (8 bytes per track: path, type, fade-in): the front-end tracks 0xa .. 0x12
/// and 0x16 (all type 1: looped).
fn music_track(i: u32) -> Option<(&'static str, bool)> {
    Some(match i {
        0xa => ("music/zrdyroom.wav", true),
        0xb => ("music/trdyroom.wav", true),
        0xc => ("music/prdyroom.wav", true),
        0xd => ("music/zdefeat.wav", true),
        0xe => ("music/tdefeat.wav", false),
        0xf => ("music/pdefeat.wav", false),
        0x10 => ("music/zvict.wav", true),
        0x11 => ("music/tvict.wav", true),
        0x12 => ("music/pvict.wav", false),
        0x16 => ("music/title.wav", false),
        _ => return None,
    })
}

/// A palette fade (0x41e8b0 over 0x41e5c0): 10 ms per step.
struct Fade {
    from: Pal,
    /// None: the palette of the screen shown when drawn.
    to: Option<Pal>,
    ms: u32,
    steps: Vec<(i32, i32)>,
}

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
    /// 0x4d4470: play these cinematics (numbers into 0x512690), then go on.
    Cine(Vec<u8>),
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
    /// 0x4d8df0: rez\credits.bin, event proc 0x4d8d60 (glue_credits.rs).
    Credits,
    /// 0x4b2930: rez\gluCmpgn.bin, event proc 0x4b28b0.
    Campaign,
    /// 0x4b52a0: rez\gluExpCmpgn.bin, event proc 0x4b5220.
    CampaignExp,
    /// 0x4b6490: rez\gluLoad.bin, event proc 0x4b6410.
    Load,
    /// 0x4b6e50 from 0x4ed940 (New ID): rez\gluPEdit.bin, event proc 0x4b6cc0.
    NewId,
    /// 0x4b7820: rez\gluHist.bin (the campaign's mission list), event proc 0x4b6e10.
    MissionList,
    /// 0x4b73b0 from 0x4ed840 (Delete): rez\gluPOkCancel.bin, event proc 0x4b6d60.
    DeleteChar,
    /// 0x4b73b0 from 0x4b62a0 (the load screen's Delete): rez\gluPOkCancel.bin, event proc
    /// 0x4b6d60.
    DeleteSave,
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
    /// The game that just ended in victory / defeat (state 6 / 7), for the score screen.
    pub game_end: Option<crate::view::GameEnd>,
    /// 0x58d6c4: the mission objectives string per player (the briefing's action 0x427ec0
    /// and the game's trigger action 0x4c51b0 set it; the game's menu 0x4c9de0 shows it).
    objectives: [u32; 8],
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
    /// The campaign start waiting for the mission list (0x4b7820): the race index and the
    /// profile; `hist_list` is 0x6d5a4c, the record list.
    hist: Option<(usize, Profile)>,
    hist_list: u32,
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
    /// Sounds and music for the window loop to play.
    pub cmds: Vec<AudioCmd>,
    /// 0x6d5bac && 0x6d5bbc: a track is playing.
    music_playing: bool,
    /// 0x51c428 ...: the buttons slide (0x4dd5e0 in, 0x4dd040 out) and its timer 7.
    slide: slide::Slide,
    /// 0x51a158: the options gamma.
    pub gamma: i16,
    /// The fade in progress (0x41ea30), and the palette it ends in.
    fade: Option<Fade>,
    /// The control the mouse is over (event 9 of the glue buttons).
    hover: Option<i16>,
    /// 0x51a220 / 0x51a228: the login screen's profiles, by creation time (0x4ed440).
    profiles: Vec<Profile>,
    /// The login dialog's 50 ms timer 0x57 (no profiles: New ID opens), ms left.
    login_timer: Option<u32>,
    /// 0x599a88: the characters gluPEdit's edit refuses (0x4b6b10).
    edit_refuse: Vec<u8>,
    /// The last left button press (clock, control id): a second one on the same control
    /// within the double-click time (500 ms) is event 6.
    last_press: Option<(u32, i16)>,
    /// The create game screen's globals (glue_create.rs).
    create: create::Create,
    /// The score screen's globals (glue_score.rs).
    score: score::Score,
    /// The credits and mission text screens' globals (glue_credits.rs).
    credits: credits::Credits,
    /// The ready room's globals (glue_ready.rs).
    ready: ready::Ready,
    /// 0x51a238: the load screen's saved games, as the list rows show them (newest first; the
    /// native records are in the order 0x4b6010 made them): the file name without its
    /// extension (record +0x180). The record's other copies (+0x17c .. +0x1d8, +0x2a2: the
    /// header's strings and game info) are read by nothing ported.
    saves: Vec<Vec<u8>>,
    /// 0x599da4: the name of the saved game selected (0x4b5ef0).
    save_name: Vec<u8>,
    /// The saved game 0x4cf950 opened (the file 0x6d1218), for the game start.
    pub save: Option<Vec<u8>>,
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
            game_end: None,
            objectives: [0; 8],
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
            cmds: Vec::new(),
            music_playing: false,
            slide: slide::Slide::default(),
            gamma: 100,
            fade: None,
            hover: None,
            profiles: Vec::new(),
            login_timer: None,
            edit_refuse: Vec::new(),
            last_press: None,
            hist: None,
            hist_list: 0,
            create: create::Create::default(),
            score: score::Score::default(),
            credits: credits::Credits::default(),
            ready: ready::Ready::default(),
            saves: Vec::new(),
            save_name: Vec::new(),
            save: None,
        }
    }

    /// 0x4e0820's loop body: one pass of its switch on 0x596904. `Top::Glue` while the
    /// glue loop has a dialog open.
    pub fn top_step(&mut self, files: &Storage) -> Top {
        if !self.expansion_installed {
            self.expansion = false;
            self.setup.expansion = self.expansion;
        }
        if std::env::var("SCBW_GLUE_TRACE").is_ok() { eprintln!("top state {} screen {} dlg {:?}", self.state, self.screen, self.dlg.as_ref().map(|d| d.1)); }
        match self.state {
            state::INTRO => {
                // 0x4d4470 twice (the expansion's intro when gluexpcmpgn.bin exists), then 4.
                // (The first-start flags 0x6cdfec & 0x200 / 0x800 play them before the loop.)
                self.state = state::GLUE;
                Top::Cine(vec![0, if self.expansion_installed { 0x18 } else { 1 }])
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
                // 0x4d8fd0 / 0x4d90c0 run their dialogs; at the end they set state 4 and the
                // main menu (credits_finish).
                if !self.credits.running {
                    self.credits_start(files, self.state == state::CREDITS_EXP);
                }
                Top::Glue
            }
            state::AFTER_GAME => {
                // 0x4e0820 case 10: 0x4f7ae0 plays the cinematic of 0x5122b8 (ESI), which is
                // then reset to its default 0x1c (unconditionally, as natively).
                let list = self.cine_after_game();
                self.scenario = 0x1c;
                if self.state == state::AFTER_GAME {
                    self.campaign_next(files, true);
                }
                Top::Cine(list)
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
        self.setup.expansion = self.expansion;
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
        self.setup.expansion = self.expansion;
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
        self.music_start(0x16);
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
        self.init_controls(files, &mut dlg);
        self.glue_dialog_init(files, &mut dlg, true);
        dlg
    }

    /// Event 10 to the list boxes (0x41b140) and edits (0x416450) of a new dialog.
    fn init_controls(&mut self, files: &Storage, dlg: &mut Dialog) {
        let p = self.palette_id();
        let skin = &self.look(files, p).skin;
        let bottom = dlg.this.rect.3;
        for c in &mut dlg.controls {
            match c.ty {
                12 | 13 => {
                    skin.list_init(c);
                    // 0x41b410 opens a combo up when its rows pass the dialog's bottom.
                    if let Some(l) = c.list.as_mut() {
                        l.parent_bottom = bottom;
                    }
                }
                8 => skin.edit_init(c),
                _ => {}
            }
        }
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
            self.fade_in(3);
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
        self.dlg = Some((dlg, if exp { Open::CampaignExp } else { Open::Campaign }));
        // The event proc's slide-in (0x4b28ea / 0x4b525a, after 0x4dd9e0): ECX = the list, AX = 2.
        self.slide_in(if exp { 0x512a8c } else { 0x512b10 }, 2, 0);
        self.show_palette();
        self.music_start(0x16);
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
            let created = if name.eq_ignore_ascii_case(default) { self.setup.new_profile(name.as_bytes(), &self.network_raw) } else { None };
            match created {
                Some(n) => p = n,
                // 0x6d5a10 is 0 here.
                None => todo_port!("0x4bb300", "error popup (string 0x58)"),
            }
        }
        // 0x4b7820 (ECX = the race's record list, EAX = the profile's best mission).
        let list = self.exe_u32(0x5122a0 + if self.expansion { 0xc } else { 0 } + idx as u32 * 4);
        let best = p.best(self.expansion, idx as u8);
        if !self.hist_gate(list, best) {
            return self.campaign_chosen(files, idx, p, list);
        }
        self.open_hist(files, list, best);
        self.hist = Some((idx, p));
        false
    }

    /// 0x4dc0b0 after 0x4b7820 returned `rec` (0: cancelled): the profile's best mission,
    /// then a scenario (the next game state) or the mission (0x4dbfc0).
    fn campaign_chosen(&mut self, files: &Storage, idx: usize, mut p: Profile, rec: u32) -> bool {
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
            // 0x5122b8 = scenario, 0x57f244 = mission, 0x57f246 = 0, state 10.
            self.scenario = scenario as u32;
            self.setup.map_dir_flag = mission;
            self.setup.next_map.clear();
            self.state = state::AFTER_GAME;
            return true;
        }
        self.start_mission(files, mission)
    }

    /// 0x4b6530 (EAX = the record list, ESI = `best`): more than one record of the list
    /// (8 bytes each, +0 flag, +2 mission; ends at mission 0) is within the best mission.
    fn hist_gate(&self, list: u32, best: u32) -> bool {
        let mut n = 0;
        let mut r = list;
        while self.exe_u16(r + 2) != 0 {
            if self.exe_u16(r + 2) as u32 <= best && self.exe_u16(r) != 0 {
                n += 1;
            }
            r += 8;
        }
        n > 1
    }

    /// 0x4b7820 after the gate: the mission history dialog (rez\gluHist.bin over
    /// glue\campaign\pHist.pcx, event proc 0x4b6e10, shared case 0 / 10 with 0x4b6d60). Event 0
    /// (0x4b6930) lists the records within `best` with a flag by the text gluHist.tbl[flag - 1]
    /// (the row's data is the record's index) and selects the first.
    fn open_hist(&mut self, files: &Storage, list: u32, best: u32) {
        let Ok(raw) = files.read("rez/gluHist.tbl") else { crate::bw::fatal_error("rez\\gluHist.tbl") };
        let tbl = crate::dat::parse_tbl(&raw);
        self.popup = None;
        self.popup_dialog_pcx(files, "rez/gluHist.bin", "glue/campaign/pHist", Open::MissionList, "", "");
        let mut rows = Vec::new();
        let (mut r, mut i) = (list, 0u32);
        while self.exe_u16(r + 2) != 0 {
            let flag = self.exe_u16(r) as usize;
            if self.exe_u16(r + 2) as u32 <= best && flag != 0 {
                rows.push((tbl.get(flag - 1).cloned().unwrap_or_default(), i));
            }
            r += 8;
            i += 1;
        }
        self.hist_list = list;
        let Some((dlg, _)) = self.popup.as_mut() else { return };
        let Some(c) = dlg.get_mut(6) else { return };
        c.list.as_mut().unwrap().batch = true;
        for (text, i) in &rows {
            let row = c.list_add(text.as_bytes(), 0);
            if let Some(it) = c.list.as_mut().unwrap().items.get_mut(row as usize) {
                it.2 = *i;
            }
        }
        c.list.as_mut().unwrap().batch = false;
        c.list_layout();
        if !rows.is_empty() {
            c.list_select(0);
            c.list_scroll_to(0);
        }
    }

    /// 0x4b6570 (event 0xe code 2): the chosen record when Ok (1) was activated and a row is
    /// selected: the list + the row's data * 8; else 0 (0x6d5a48).
    fn hist_done(&self, dlg: &Dialog, id: i16) -> u32 {
        let sel = dlg.get(6).and_then(|c| c.list.as_ref()).and_then(|l| l.items.get(l.sel as usize));
        match sel {
            Some(it) if id == 1 => self.hist_list + it.2 * 8,
            _ => 0,
        }
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
        if let Some(t) = Setup::load_got(files, "Use Map Settings(1)", &mut a, &mut b) {
            let mp = self.setup.map_path;
            let mp = &mp[..mp.iter().position(|&c| c == 0).unwrap_or(mp.len())];
            if let Some(sep) = mp.iter().rposition(|&c| c == b'\\') {
                let mut path = mp[..=sep].to_vec();
                path.extend_from_slice(&self.setup.next_map);
                // 0x4a8590 (EAX = path, ECX = the template's first 4 bytes, ESI = 0x57ee9c, stack:
                // 0x6cdfd4); 0 is success.
                let packed = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
                let r = self.start_next_map(files, &path, packed);
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

    /// 0x4a8590 (EAX = `path`, ECX = `packed`, the first 4 bytes of the game type record: type,
    /// +1, subtype; ESI = 0x57ee9c, the player name; stack: the speed 0x6cdfd4): load the map's
    /// header, build the game description (name, map title, size, players, league byte,
    /// game type), check it (0x4a68d0), mark the campaign as continuing (0x51ca1c, 0x57f1e3),
    /// take the local slot (0x4dbe50) and create the game (0x4d3fc0). 0 is success, else the
    /// error code.
    fn start_next_map(&mut self, files: &Storage, path: &[u8], packed: u32) -> i32 {
        if path.is_empty() {
            return -0x7fff_fffa;
        }
        let mut hdr = [0u8; 0x20];
        let path_s = String::from_utf8_lossy(path).into_owned();
        if !self.setup.load_map(files, &path_s, &mut hdr, false, &self.network_raw) {
            let len = u16::from_le_bytes([hdr[0x18], hdr[0x19]]);
            return (-0x7fff_fffa_i32).wrapping_sub(if len > 0x3b { 5 } else { 0 });
        }
        let mut d = [0u8; 0x8d];
        sstr_copy(&mut d[4..], &self.setup.local_name, 0x20);
        let name_id = u16::from_le_bytes([hdr[0], hdr[1]]);
        // 0x4bd0c0: the map's string, "" for a map without the table.
        let title = if name_id != 0 { self.setup.map_string(name_id).unwrap_or_default().to_vec() } else { Vec::new() };
        sstr_copy(&mut d[0x4d..], &title, 0x20);
        d[0x24] = 1;
        d[0x25] = self.setup.lobby_occupied();
        d[0x20..0x22].copy_from_slice(&self.setup.map_width.to_le_bytes());
        d[0x22..0x24].copy_from_slice(&self.setup.map_height.to_le_bytes());
        d[0x30..0x32].copy_from_slice(&self.setup.tileset.to_le_bytes());
        d[0x27] = hdr[0x1c];
        d[0x28..0x2c].copy_from_slice(&packed.to_le_bytes());
        self.load_templates(files);
        self.create.speed = crate::menu::Settings::default().game_speed as u8;
        let rc = self.check_template(&mut d);
        if rc != 0 {
            return rc;
        }
        // 0x596888 = 0.
        self.camp_cont = true;
        if !self.setup.local_slot_setup() {
            return -0x7fff_fff9;
        }
        if self.create_game(&d) {
            0
        } else {
            // 0x4a8590: the Storm error decides (Ordinal 463); 0x80000004 + 2 unless 0xb7.
            -0x7fff_fffa
        }
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
        if self.mission_text(files, false, credits::Then::CampaignNext) {
            // The rest (campaign_next_tail) runs when the text screen ends.
            return true;
        }
        self.campaign_next_tail(files)
    }

    /// 0x4dc320 after the mission text (0x4d91b0).
    fn campaign_next_tail(&mut self, files: &Storage) -> bool {
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
        self.setup.record_mission(mission, race, &self.network_raw);
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
    /// text screen (rez\<name>.txt on rez\credits.bin, 0x4d8df0; glue_credits.rs). True when
    /// it was opened: the caller goes on when it ends (`then`), as the native call returns
    /// after the modal dialog.
    fn mission_text(&mut self, files: &Storage, exp: bool, then: credits::Then) -> bool {
        let mission = self.setup.map_dir_flag;
        if self.setup.multiplayer || self.g6d5a6c & 0x2000_0000 != 0 || mission == 0 {
            return false;
        }
        let (table, n) = if exp { (0x4fff60, 0x40) } else { (0x500160, 1) };
        for i in 0..n {
            if self.exe_u16(table + 8 * i + 4) == mission {
                let ptr = self.exe_u32(table + 8 * i);
                let name: Vec<u8> = (0..).map(|k| self.exe_u8(ptr + k)).take_while(|&c| c != 0).collect();
                self.credits_mission(files, &String::from_utf8_lossy(&name), then);
                return true;
            }
        }
        false
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
        if !self.setup.load_map(files, &path, &mut hdr, true, &self.network_raw) {
            return false;
        }
        // The game description (0x8d bytes): +4 the player name (0x18), +0x24 / +0x25 = 1,
        // +0x26 the speed (0x6cdfd4), +0x4d the map title (0x20), +0x6d the template.
        let mut d = [0u8; 0x8d];
        sstr_copy(&mut d[4..], &self.setup.local_name, 0x18);
        sstr_copy(&mut d[0x4d..], &self.setup.map_title, 0x20);
        d[0x26] = crate::menu::Settings::default().game_speed as u8;
        d[0x24] = 1;
        d[0x25] = 1;
        let (mut a, mut b) = ([0u8; 0x20], [0u8; 0x20]);
        let Some(t) = Setup::load_got(files, "Use Map Settings(1)", &mut a, &mut b) else { return false };
        d[0x6d..0x8d].copy_from_slice(&t);
        if !self.setup.local_slot_setup() {
            return false;
        }
        // 0x596888 = 0.
        self.create_game(&d)
    }

    /// 0x4f71f0 (`race` 0 zerg, rez\glurdyz.bin, event proc 0x4f7150) / 0x4f7550 (1 terran,
    /// glurdyt.bin, 0x4f74b0) / 0x4f78b0 (2 protoss, glurdyp.bin, 0x4f7810), before 0x41a080:
    /// the ready room (mission briefing). The music of the race (0x4a6310: 0xa + race)
    /// after the previous one fades out (0x4a5f50) and the mission text (0x4d9200).
    fn open_ready(&mut self, files: &Storage, race: u8) {
        self.music_fade_out();
        self.music_start(0xa + race as u32);
        if !self.mission_text(files, true, credits::Then::Ready(race)) {
            self.open_ready_dialog(files, race);
        }
    }

    /// 0x4f71f0 / 0x4f7550 / 0x4f78b0 after the mission text: the dialog.
    fn open_ready_dialog(&mut self, files: &Storage, race: u8) {
        if self.state != state::GLUE {
            return;
        }
        self.shown = -1;
        let bin = ["rez/glurdyz.bin", "rez/glurdyt.bin", "rez/glurdyp.bin"][race as usize];
        let dlg = self.open_glue_dialog(files, bin);
        // The event proc's case 10 (0x46d3c0, the slide-in 0x4dd5e0, 0x4dd9e0), 0x405 when
        // the buttons are in (`slide_in_step`) and case 0: 0x4de150.
        self.dlg = Some((dlg, Open::Ready(race)));
        self.ready_open(race);
        self.show_palette();
    }

    /// 0x4f71f0 / 0x4f7550 / 0x4f78b0 (after 0x41a080): 0xe (back) goes to the campaign
    /// select (Brood War +0x10) or, in a network game, to the screen of the provider
    /// (0x4dc5b0); 0x13 (next) is 0x4dc320(1); 0x65 starts the game (state 1); 100 stays;
    /// anything else goes to the main menu. All but 0x13 / 0x65 then fade the music
    /// (0x4a5f50) and play 0x16 (0x4a6310).
    fn done_ready(&mut self, files: &Storage, result: i32, _race: u8) {
        // The dialog's event 1 (0x46cfd0).
        self.ready_46cfd0();
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
        self.music_fade_out();
        self.music_start(0x16);
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
    /// login. Event 10: the list (8, proc 0x4ed770 from 0x51a478) gets flag 0x20000, the
    /// panels slide in (0x4ed810); event 0: 0x4ed720, 0x4de150, then the list's 0x4ed640.
    fn open_login(&mut self, files: &Storage) {
        let mut dlg = self.open_glue_dialog(files, "rez/gluLogin.bin");
        if let Some(c) = dlg.get_mut(8) {
            c.flags |= 0x20000;
        }
        self.dlg = Some((dlg, Open::Login));
        // Event 10's 0x4ed810: after 0x4dd9e0, the slide-in 0x4dd5e0 (ECX = 0x512a00, AX = 3).
        self.slide_in(0x512a00, 3, 0);
        self.login_profiles();
        self.show_palette();
        self.login_fill();
    }

    /// 0x4ed720: the profiles of this mode (0x4a8ab0, 0x4ed490: at most 0xfa, the
    /// network flag matching 0x57f0b4, in creation time order); with none the 50 ms
    /// timer 0x57 is set.
    fn login_profiles(&mut self) {
        self.profiles.clear();
        for p in self.setup.list_profiles() {
            if self.profiles.len() < 0xfa && (p.0[4] != 0) == self.setup.multiplayer {
                self.login_insert(p);
            }
        }
        if self.profiles.is_empty() {
            self.login_timer = Some(50);
        }
    }

    /// 0x4ed440: `p` goes before the first profile created later; its index.
    fn login_insert(&mut self, p: Profile) -> usize {
        let t = |p: &Profile| u32::from_le_bytes(p.0[8..12].try_into().unwrap());
        let i = self.profiles.iter().position(|q| t(q) > t(&p)).unwrap_or(self.profiles.len());
        self.profiles.insert(i, p);
        i
    }

    /// The login dialog's list box (id 8).
    fn login_list(&mut self) -> Option<&mut crate::dlg::Ctl> {
        self.dlg.as_mut().and_then(|(d, _)| d.get_mut(8))
    }

    /// 0x418640 (on) / 0x418e00 (off) on control `id` of the screen's dialog.
    fn disable(&mut self, id: i16, on: bool) {
        if let Some(c) = self.dlg.as_mut().and_then(|(d, _)| d.get_mut(id)) {
            if on {
                c.flags |= crate::dlg::flag::DISABLED;
            } else {
                c.flags &= !crate::dlg::flag::DISABLED;
            }
        }
    }

    /// 0x4ed640 (the list's event 0): Delete and Ok off, a row per profile, the first
    /// selected (event 0xb row 0, then 0x41a600).
    fn login_fill(&mut self) {
        self.disable(7, true);
        self.disable(4, true);
        let names: Vec<Vec<u8>> = self.profiles.iter().map(|p| p.name().to_vec()).collect();
        let Some(c) = self.login_list() else { return };
        c.list.as_mut().unwrap().batch = true;
        for n in &names {
            c.list_add(n, 0);
        }
        c.list.as_mut().unwrap().batch = false;
        c.list_layout();
        if !names.is_empty() {
            self.list_event(8, 0);
            if let Some(c) = self.login_list() {
                c.list_scroll_to(0);
            }
        }
    }

    /// Event 0xe code 0xb to list box `id` of the top dialog (0x41baa0 selects `row`);
    /// the login list's proc 0x4ed770 then runs 0x4ed560.
    fn list_event(&mut self, id: i16, row: u8) {
        let top = if self.popup.is_some() { self.popup.as_mut() } else { self.dlg.as_mut() };
        let Some((dlg, open)) = top else { return };
        let open = *open;
        if let Some(c) = dlg.get_mut(id) {
            c.list_select(row);
        }
        if open == Open::Login && id == 8 {
            self.login_buttons();
        }
        if open == Open::Load {
            self.load_buttons();
        }
        if matches!(open, Open::Create(_)) {
            self.create_event(id, row);
        }
    }

    /// 0x4ed560: Ok and Delete work only with a profile selected.
    fn login_buttons(&mut self) {
        let sel = self.login_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        let off = sel == 0xff || self.profiles.is_empty();
        self.disable(7, off);
        self.disable(4, off);
    }

    /// 0x4edaa0 event 0xe code 2 (an activated control in 0x6d051c): Ok (4) takes the
    /// selected profile (0x4ed3f0) or stays, New ID (6) 0x4ed940, Delete (7) 0x4ed840,
    /// anything else ends the dialog (0x4dd040).
    fn login_activate(&mut self, files: &Storage, id: i16) {
        self.activated = id;
        match id {
            4 if !self.login_take() => {}
            6 => self.login_new_id(files),
            7 => self.login_delete(files),
            _ => {
                self.end_dialog(id as i32);
            }
        }
    }

    /// 0x4ed3f0: the selected profile's name becomes the player name (0x57ee9c,
    /// 0x66ff30 = 0). False with none.
    fn login_take(&mut self) -> bool {
        let sel = self.login_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        let Some(p) = self.profiles.get(sel as usize) else { return false };
        if sel == 0xff {
            return false;
        }
        let name = p.name().to_vec();
        self.setup.local_name = [0; 0x20];
        let n = name.len().min(0x18);
        self.setup.local_name[..n].copy_from_slice(&name[..n]);
        self.player_name = String::from_utf8_lossy(&name[..n]).into_owned();
        true
    }

    /// 0x4ed940: New ID. With 0xfa profiles listed, gluAll.tbl 0x1a says to delete one;
    /// else the name popup 0x4b6e50 (gluAll 0x17, 0x19 bytes, refusing network.tbl
    /// string 0x10) opens; `login_new_id_done` is the part after it.
    fn login_new_id(&mut self, files: &Storage) {
        let n = self.login_list().and_then(|c| c.list.as_ref()).map_or(0, |l| l.items.len());
        if n > 0xf9 {
            let msg = self.glu_all.get(0x19).cloned().unwrap_or_default();
            return self.open_popup(files, &msg);
        }
        let title = self.glu_all.get(0x16).cloned().unwrap_or_default();
        self.edit_refuse = self.network.get(0x10).map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        self.open_edit_popup(files, &title, b"", 0x19, Open::NewId);
    }

    /// 0x4ed940 after 0x4b6e50 returned `ok` with `name`: a reserved or bad file name
    /// (network.tbl 5), a name in use (gluAll 0x19) or a failed write (gluAll 0x1b) is
    /// a message; else the profile is listed in its place, selected, Ok and Delete on.
    fn login_new_id_done(&mut self, files: &Storage, ok: bool, name: Vec<u8>) {
        if !ok {
            return;
        }
        if Setup::profile_name_invalid(&name, &self.network_raw) {
            let msg = self.network.get(5).cloned().unwrap_or_default();
            return self.open_popup(files, &msg);
        }
        // 0x4ed3a0 (_mbsicmp).
        if self.profiles.iter().any(|p| p.name().eq_ignore_ascii_case(&name)) {
            let msg = self.glu_all.get(0x18).cloned().unwrap_or_default();
            return self.open_popup(files, &msg);
        }
        let Some(p) = self.setup.new_profile(&name, &self.network_raw) else {
            let msg = self.glu_all.get(0x1a).cloned().unwrap_or_default();
            return self.open_popup(files, &msg);
        };
        let i = self.login_insert(p) as u8;
        if let Some(c) = self.login_list() {
            c.list_insert(i, &name, 0);
        }
        // 0x41a850.
        let n = self.login_list().and_then(|c| c.list.as_ref()).map_or(0, |l| l.items.len());
        if (i as usize) < n {
            self.list_event(8, i);
            if let Some(c) = self.login_list() {
                c.list_scroll_to(i);
            }
        }
        self.disable(7, false);
        self.disable(4, false);
    }

    /// 0x4ed840: Delete asks (0x4b73b0, gluAll 0x18) about the selected profile;
    /// `login_delete_done` is the part after it.
    fn login_delete(&mut self, files: &Storage) {
        let sel = self.login_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        if sel == 0xff || sel as usize >= self.profiles.len() {
            return;
        }
        let msg = self.glu_all.get(0x17).cloned().unwrap_or_default();
        self.open_ok_cancel(files, &msg, Open::DeleteChar);
    }

    /// 0x4ed840 after the question: its saved games go (0x4cf7b0), then the file; when
    /// that worked the row and the profile go, with no rows left Delete and Ok are off,
    /// and the selection is reported again (0x41a850, 0x4ed560).
    fn login_delete_done(&mut self, ok: bool) {
        if !ok {
            return;
        }
        let sel = self.login_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        let Some(p) = self.profiles.get(sel as usize) else { return };
        todo_port!("0x4cf7b0", "delete the profile's saved games");
        if !self.setup.delete_profile(&p.name().to_vec()) {
            return;
        }
        if let Some(c) = self.login_list() {
            c.list_remove(sel);
        }
        self.profiles.remove(sel as usize);
        let (n, sel) = self.login_list().and_then(|c| c.list.as_ref()).map_or((0, 0xff), |l| (l.items.len(), l.sel));
        if n == 0 {
            self.disable(7, true);
            self.disable(4, true);
        }
        if (sel as usize) < n || sel == 0xff {
            self.list_event(8, sel);
            if let Some(c) = self.login_list() {
                c.list_scroll_to(sel);
            }
        }
        self.login_buttons();
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
        self.dlg = Some((dlg, Open::Create(which)));
        // The event proc 0x4af5c0's event 10 and 0 (and 0x405 when the controls have slid
        // in): glue_create.rs.
        self.create_open(files, which);
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
        // 0x58d700: the game's results (the score screen reads the game's globals).
        if let Some(g) = self.game_end.as_ref() {
            self.setup.results = g.results;
        }
        let (outcomes, local) = self.setup.player_outcomes(self.clock_ms);
        let won = local == 1;
        let i = if self.g6d5a60 == 0 {
            let race = self.setup.slots[self.setup.local_slot[0] as usize % 12][slot::RACE] as u32;
            race * 2 + won as u32
        } else {
            1
        };
        self.screen = self.exe_u32(0x512ab0 + 4 * i) as u8;
        self.music_start(self.exe_u32(0x512ac8 + 4 * i));
        // iScore.grp and tminimap.pcx from the directory 0x512a98[i] (0x4d2b30 buffer,
        // SBmpAllocLoadImage), then 0x4bca80.
        self.score_begin(files, i, outcomes, local);
        let mut dlg = self.open_glue_dialog(files, "rez/gluScore.bin");
        self.score_controls(files, &mut dlg);
        self.dlg = Some((dlg, Open::Score(won)));
        // The event proc's slide-in (0x4b4a00: 0x4dd5e0 over list 0x51a844, 500 ms).
        self.slide_in(0x51a844, 1, 500);
        self.score_init(files);
    }

    /// 0x4b4af0 (after 0x41a080): 7 (continue) goes to the provider's screen in a network
    /// game, else on with the campaign (0x4dc320 with the victory flag); anything else
    /// (or a campaign that cannot go on) records the progress (0x4dbee0) of a single-player
    /// game that is not a continued map and goes to the main menu. Then the music fades.
    fn done_score(&mut self, files: &Storage, result: i32, won: bool) {
        // 0x4b4a30 event 1 (0x4b30a0, after a Sleep(1000)): the images go, 0x6d5a60 = 0.
        self.g6d5a60 = 0;
        self.score_end();
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
                self.setup.record_mission(mission, race, &self.network_raw);
            }
        }
        if !done {
            self.screen = screen::MAIN;
        }
        self.glue_close();
        self.music_fade_out();
    }

    /// 0x4b6490 (rez\gluLoad.bin, event proc 0x4b6410), before 0x41a080: the load game
    /// screen. The list's event proc 0x4b6340: event 10 sets flag 0x20000, event 0 (0x4b61c0)
    /// is `load_fill_rows`.
    fn open_load(&mut self, files: &Storage) {
        let mut dlg = self.open_glue_dialog(files, "rez/gluLoad.bin");
        if let Some(c) = dlg.controls.iter_mut().find(|c| c.ty == 12) {
            c.flags |= 0x20000;
        }
        self.dlg = Some((dlg, Open::Load));
        // Event 10 (0x4b6410): the slide-in of list 0x512a48 (0x4b643b, AX = 3, no delay).
        self.slide_in(0x512a48, 3, 0);
        self.show_palette();
        // 0x4b61c0: Ok (4) and Delete (7) are off until a game is selected.
        self.disable(4, true);
        self.disable(7, true);
        self.load_fill_rows();
    }

    /// 0x4b6490 (after 0x41a080): 4 loads the game chosen (0x4cf950 with the name 0x599da4;
    /// 0x4cf8e0 sets the state 4 -> 1, the game start), 5 goes back to the campaign select
    /// (Brood War: +0x10), anything else to the main menu.
    fn done_load(&mut self, result: i32) {
        match result {
            4 => {
                let exe = crate::saveload::exe_dir(&self.setup.characters_dir);
                let mut g = crate::saveload::OpenGlobals { g596904: self.state as u32, ..Default::default() };
                let name = String::from_utf8_lossy(&self.save_name).into_owned();
                self.save = crate::saveload::open_save(&exe, self.player_name.as_bytes(), self.multiplayer, self.expansion, &name, &mut g);
                if self.save.is_some() && g.g596904 == state::START as u32 {
                    self.state = state::START;
                }
            }
            5 => self.screen = if self.expansion { screen::CAMPAIGN2 } else { screen::CAMPAIGN },
            _ => self.screen = screen::MAIN,
        }
        self.glue_close();
    }

    /// 0x4b6410: the load screen's event proc for an activated control: 7 (Delete) asks
    /// (0x4b62a0: gluAll.tbl 0x16, 0x4b73b0; `load_delete_done` is the rest), any other ends
    /// the dialog.
    fn load_activate(&mut self, files: &Storage, id: i16) -> bool {
        self.activated = id;
        if id == 7 {
            let msg = self.glu_all.get(0x15).cloned().unwrap_or_default();
            self.open_ok_cancel(files, &msg, Open::DeleteSave);
            return true;
        }
        self.end_dialog(id as i32)
    }

    /// 0x4b62a0 after the question: with Ok the selected game's file is deleted (0x4cf820),
    /// the list refilled (0x4b6010 per file) and the buttons set (0x4b5ef0).
    fn load_delete_done(&mut self, ok: bool) {
        if !ok {
            return;
        }
        let sel = self.load_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        if let Some(name) = self.saves.get(sel as usize).cloned() {
            self.save_delete(&name);
        }
        self.load_fill_rows();
    }

    /// The load screen's list box.
    fn load_list(&mut self) -> Option<&mut crate::dlg::Ctl> {
        self.dlg.as_mut().and_then(|(d, _)| d.controls.iter_mut().find(|c| c.ty == 12))
    }

    /// 0x4cf820: delete the saved game `name` of the profile (the save dir, created if
    /// missing; <name><extension>; a read-only file first made writable).
    fn save_delete(&self, name: &[u8]) {
        let exe = crate::saveload::exe_dir(&self.setup.characters_dir);
        let name = String::from_utf8_lossy(name);
        let Some(path) = crate::saveload::save_path(&exe, self.player_name.as_bytes(), self.multiplayer, self.expansion, false, &name) else { return };
        let path = crate::saveload::host(&path);
        if let Ok(m) = std::fs::metadata(&path) {
            let mut p = m.permissions();
            if p.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                let _ = std::fs::set_permissions(&path, p);
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    /// 0x4cfd90 (without the callback): the saved games of the profile, oldest first. The
    /// save dir (0x4ce5b0) is listed (0x4cf330) for "*<extension>" (files only, not hidden);
    /// 0x4cfb00 keeps a file whose name is under 0x20 characters and which 0x4cfa10 can read
    /// (the text up to 0x1a, the header with id 0x69), named by the file name without its last
    /// extension when that is under 0x1c characters; the list is kept in order of the file
    /// time (a new one goes before the first that is not older).
    fn save_files(&self) -> Vec<Vec<u8>> {
        use crate::saveload::{host, save_dir, save_ext, read_header};
        let exe = crate::saveload::exe_dir(&self.setup.characters_dir);
        let Some(dir) = save_dir(&exe, self.player_name.as_bytes(), self.multiplayer, false, 0x104) else { return Vec::new() };
        let ext = save_ext(self.expansion, self.multiplayer);
        let mut ents: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(host(&dir))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.metadata().ok()?.modified().ok()?)))
            .filter(|(n, _)| !n.starts_with('.') && n.len() >= ext.len() && n[n.len() - ext.len()..].eq_ignore_ascii_case(ext))
            .collect();
        ents.sort();
        let mut list: Vec<(Vec<u8>, std::time::SystemTime)> = Vec::new();
        for (n, t) in ents {
            if n.len() >= 0x20 || dir.len() + n.len() >= 0x104 {
                continue;
            }
            let Ok(f) = std::fs::read(host(&format!("{dir}{n}"))) else { continue };
            let mut i = crate::bw::save::In::new(&f);
            let head = loop {
                match i.bytes(1) {
                    None => break None,
                    Some(b) if b[0] == 0x1a => break read_header(&mut i),
                    _ => {}
                }
            };
            if head.is_none_or(|h| h.id != 0x69) {
                continue;
            }
            let mut name = n.as_bytes();
            if let Some(p) = name.iter().rposition(|&c| c == b'.') {
                name = &name[..p];
            }
            if name.len() >= 0x1c {
                continue;
            }
            let at = list.iter().position(|(_, o)| t <= *o).unwrap_or(list.len());
            list.insert(at, (name.to_vec(), t));
        }
        list.into_iter().map(|(n, _)| n).collect()
    }

    /// 0x4b61c0 / 0x4b62a0: the list cleared (0x41b890), a row per saved game (0x4b6010
    /// inserts each at row 0, so the newest is first), laid out once (0x41b410), then the
    /// buttons (0x4b5ef0).
    fn load_fill_rows(&mut self) {
        let files = self.save_files();
        self.saves.clear();
        if let Some(c) = self.load_list() {
            c.list_clear();
            c.list.as_mut().unwrap().batch = true;
            let mut rows = Vec::new();
            for n in files {
                // 0x41b620 ignores a full list.
                if c.list.as_ref().unwrap().items.len() >= crate::dlg::LIST_MAX {
                    continue;
                }
                c.list_insert(0, &n, 0);
                rows.insert(0, n);
            }
            c.list.as_mut().unwrap().batch = false;
            c.list_layout();
            self.saves = rows;
        }
        self.load_buttons();
    }

    /// 0x4b5ef0: with a game selected Ok (4) and Delete (7) are on and its name goes to
    /// 0x599da4; else both are off.
    fn load_buttons(&mut self) {
        let sel = self.load_list().and_then(|c| c.list.as_ref()).map_or(0xff, |l| l.sel);
        let rec = if sel == 0xff { None } else { self.saves.get(sel as usize).cloned() };
        self.disable(4, rec.is_none());
        self.disable(7, rec.is_none());
        if let Some(n) = rec {
            self.save_name = n;
        }
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

    /// 0x4de0b0 / 0x4bccf0: the glue sounds: the list 0x6dc2ac = 0xa5 .. 0xad (once), the sound
    /// hook 0x6d5eac = 0x4dd1d0.
    fn glue_sound_init(&mut self) {
        self.sound_list.get_or_insert([0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad]);
        // 0x4bccf0 preloads the list into the sound cache (Audio loads on demand).
    }

    /// 0x4dd1d0: the dialogs' sound hook (0x6d5eac): event 0 (hover) is sound 0xa6 at most
    /// every 200 ms (`clock_ms` is GetTickCount), event 1 (click) 0xa5, both through
    /// 0x4bc2c0.
    fn glue_sound_hook(&mut self, event: i32) {
        match event {
            0 if self.clock_ms.wrapping_sub(self.sound_tick) > 199 => {
                self.sound_tick = self.clock_ms;
                self.cmds.push(AudioCmd::Sound(0xa6));
            }
            1 => self.cmds.push(AudioCmd::Sound(0xa5)),
            _ => {}
        }
    }

    /// The glue buttons' (types 1 and 2: 0x4e17e0 / 0x4e1560, light-ups: 0x4e1a70) mouse
    /// entering (event 9 of 0x4e1390 / 0x4e1670, event 3 of 0x4e1970): hook 0.
    fn hover_sound(&mut self) {
        let top = self.popup.as_ref().or(self.dlg.as_ref());
        let hit = top.and_then(|(d, _)| d.hit(self.mouse.0, self.mouse.1).map(|i| &d.controls[i]));
        let id = hit.map(|c| c.id);
        if id != self.hover {
            self.hover = id;
            if hit.is_some_and(|c| matches!(c.ty, 1 | 2 | 14) && c.flags & crate::dlg::flag::DISABLED == 0) {
                self.glue_sound_hook(0);
            }
        }
    }

    /// The left button went down over a glue button (0x4e0d40, mouse inside, not yet
    /// pressed): hook 0, the control becomes pressed, hook 1.
    pub fn mouse_down(&mut self) {
        let top = self.popup.as_ref().or(self.dlg.as_ref());
        let hit = top.and_then(|(d, _)| d.hit(self.mouse.0, self.mouse.1).map(|i| &d.controls[i]));
        if hit.is_some_and(|c| matches!(c.ty, 1 | 2) && c.flags & crate::dlg::flag::DISABLED == 0) {
            self.glue_sound_hook(0);
            self.glue_sound_hook(1);
        }
    }

    /// 0x4a6310 / 0x4a6140 (eax = track `i`): starts the front-end track unless one plays: 0x4a6140
    /// opens it (looped: type 1) and, for tracks with the fade flag (0x512bbd) and music on,
    /// the 20 ms timer 0x4a6030 raises it from -10000 by 400 a step.
    fn music_start(&mut self, i: u32) {
        if self.music_playing {
            return;
        }
        if (1..=9).contains(&i) {
            self.music_playing = true;
            self.cmds.push(AudioCmd::Track(i));
            return;
        }
        let Some((path, fade)) = music_track(i) else { return };
        self.music_playing = true;
        self.cmds.push(AudioCmd::Music { path, fade });
    }

    /// 0x4a5f50: the music fades out and closes (a no-op when none is open).
    fn music_fade_out(&mut self) {
        if std::mem::take(&mut self.music_playing) {
            self.cmds.push(AudioCmd::FadeOutMusic);
        }
    }

    /// 0x41ea30 (`n` steps): when the palette is blank, fade it in from black to the
    /// screen's palette (0x5994e0 -> 0x6ceb40).
    fn fade_in(&mut self, n: u32) {
        if !self.pal_blank {
            return;
        }
        self.pal_blank = false;
        self.fade = Some(Fade { from: [[0; 3]; 256], to: None, ms: 0, steps: palette::fade_steps(n) });
    }

    /// 0x41e9e0 (`n` steps): when the palette is on, fade it out to black. The native call
    /// blocks 10 ms a step; the frame after it is the next screen's, so the fade is shown
    /// as the screen going dark (blank until `fade_in`).
    fn fade_out(&mut self, n: u32) {
        if self.pal_blank {
            return;
        }
        self.pal_blank = true;
        let from = self.look_pal();
        self.fade = Some(Fade { from, to: Some([[0; 3]; 256]), ms: 0, steps: palette::fade_steps(n) });
    }

    /// The palette of the screen shown (0x50e064).
    fn look_pal(&self) -> Pal {
        self.looks.get(self.shown.max(0) as usize).and_then(|l| l.as_ref()).map_or([[0; 3]; 256], |l| l.pal)
    }

    /// 0x4dcfa0: after a glue dialog: its background cache is dropped (the next one
    /// loads its own); when the next screen has another palette the old one fades out.
    fn glue_close(&mut self) {
        if self.shown != SCREEN_PALETTE.get(self.screen as usize).copied().unwrap_or(0) as i32 {
            self.fade_out(3);
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
                    self.setup.expansion = self.expansion;
                } else {
                    todo_port!("0x4db770", "single player / multiplayer check");
                    self.expansion = false;
                    self.setup.expansion = self.expansion;
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

    /// 0x4dd040: end the modal dialog with `result` (0x41a080 returns it): the buttons slide
    /// out (`slide_out`) and the 0x4dcd00 timer ends the dialog; every control loses flag
    /// 0x10.
    fn end_dialog(&mut self, result: i32) -> bool {
        if self.slide.leaving.is_some() {
            return true;
        }
        if let Some((dlg, _)) = self.dlg.as_mut() {
            dlg.this.flags &= !0x10;
            for c in &mut dlg.controls {
                c.flags &= !0x10;
            }
        }
        self.slide_out(result);
        true
    }

    /// 0x4b7180: a message popup (rez\gluPOk.bin) over the current dialog: the text in
    /// the label (id 2). An open popup is closed first.
    pub fn open_popup(&mut self, files: &Storage, msg: &str) {
        self.popup = None;
        self.popup_dialog(files, "rez/gluPOk.bin", Open::Popup, msg, "");
    }

    /// 0x4b73b0: the Ok / Cancel question (rez\gluPOkCancel.bin over pDPopup.pcx): the
    /// text in the label (id 2); Ok (1) is yes.
    fn open_ok_cancel(&mut self, files: &Storage, msg: &str, open: Open) {
        self.popup = None;
        self.popup_dialog_pcx(files, "rez/gluPOkCancel.bin", "pDPopup", open, msg, "");
    }

    /// 0x4b6e50: the text popup (rez\gluPEdit.bin over pEPopup.pcx). Event 0 is 0x4b6a20:
    /// the title in the label (2), the edit (4, no outline) holds `text` with the caret
    /// at its end (a preset text is replaced by the first key), Ok (1) is on only with a
    /// text (0x4b68c0), the edit has the focus; `max` is the buffer size (+0x26).
    fn open_edit_popup(&mut self, files: &Storage, title: &str, text: &[u8], max: u32, open: Open) {
        self.popup = None;
        self.popup_dialog_pcx(files, "rez/gluPEdit.bin", "pEPopup", open, "", "");
        let Some((dlg, _)) = self.popup.as_mut() else { return };
        if let Some(c) = dlg.get_mut(2) {
            c.text = title.as_bytes().to_vec();
        }
        if let Some(c) = dlg.get_mut(4) {
            c.flags |= 0x20000;
            c.text = text.iter().copied().take(0xff).collect();
            if let Some(e) = c.edit.as_mut() {
                e.caret = c.text.len() as u8;
                e.mode |= 1;
                e.max = max;
                e.preset = !text.is_empty();
                // 0x418830 -> event 5: focus, the caret shows (0x416790 AL = 1).
                e.blink = Some(500);
            }
            c.flags |= 0x4000_0000;
        }
        Self::edit_ok_state(dlg);
    }

    /// 0x4b68c0: the edit popup's Ok (1) works only with a text in the edit (4).
    fn edit_ok_state(dlg: &mut Dialog) {
        let empty = dlg.get(4).is_none_or(|c| c.text.is_empty());
        if let Some(c) = dlg.get_mut(1) {
            if empty {
                c.flags |= crate::dlg::flag::DISABLED;
            } else {
                c.flags &= !crate::dlg::flag::DISABLED;
            }
        }
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
        self.popup_dialog_pcx(files, bin, "pOPopup", open, first, second);
    }

    /// `popup_dialog` with the background `<palette dir>\<pcx>.pcx` (0x4b7180 pOPopup,
    /// 0x4b6e50 pEPopup, 0x4b73b0 pDPopup).
    fn popup_dialog_pcx(&mut self, files: &Storage, bin: &str, pcx: &str, open: Open, first: &str, second: &str) {
        let Ok(d) = files.read(bin) else { crate::bw::fatal_error(bin) };
        let mut dlg = Dialog::parse(&d);
        dlg.this.flags |= 4;
        dlg.bind(files);
        dlg.init_lightups(files);
        self.init_controls(files, &mut dlg);
        let p = self.palette_id();
        let dir = PALETTES[p];
        let path = if pcx.contains('/') { format!("{pcx}.pcx") } else { format!("{dir}/{pcx}.pcx") };
        let Ok(pcx) = files.read(&path) else { crate::bw::fatal_error(pcx) };
        let (w, h, px) = crate::view::pcx(&pcx);
        if self.state == state::GLUE {
            let (x, y) = ((640 - dlg.this.rect.2) / 2, (480 - dlg.this.rect.3) / 2);
            dlg.this.rect = (x, y, dlg.this.rect.2 + x, dlg.this.rect.3 + y);
        }
        if let Some(c) = dlg.get_mut(2).filter(|_| !first.is_empty()) {
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
        if self.slide.leaving.is_some() {
            return;
        }
        if let Some((dlg, open)) = self.popup.take() {
            // 0x4b6d60 / 0x4b5920 -> 0x418eb0: any control ends the popup (event 1 releases
            // its timers: 0x4dcc00) with its id; 0x4b6cc0's event 1 keeps the edit's text
            // (0x4b65d0).
            self.error_depth = 0;
            match open {
                Open::NewId => {
                    let text = dlg.get(4).map(|c| c.text.iter().copied().take(0x18).collect()).unwrap_or_default();
                    self.login_new_id_done(files, id == 1, text);
                }
                Open::DeleteChar => self.login_delete_done(id == 1),
                Open::DeleteSave => self.load_delete_done(id == 1),
                Open::MissionList => {
                    let rec = self.hist_done(&dlg, id);
                    if let Some((idx, p)) = self.hist.take() {
                        if self.campaign_chosen(files, idx, p, rec) {
                            self.end_dialog(self.activated as i32);
                            if let Some(r) = self.result.take() {
                                self.dlg.take();
                                self.done_campaign(r);
                            }
                        }
                    }
                }
                _ => {}
            }
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
                self.load_activate(files, id);
            }
            Open::Score(_) => {
                self.activated = id;
                self.score_activate(id);
            }
            Open::Create(_) => {
                self.create_activate(files, id);
            }
            Open::Login => {
                self.login_activate(files, id);
            }
            Open::Ready(_) => {
                // The event proc's activation (event 0xe code 2) is 0x46d160.
                self.activated = id;
                self.ready_46d160();
            }
            _ => {}
        }
        self.dialog_done(files);
    }

    /// The dialog ended (its result is set, after the slide out): the screen's `done_*`.
    fn dialog_done(&mut self, files: &Storage) {
        if let Some(r) = self.result.take() {
            let Some((_, open)) = self.dlg.take() else { return };
            self.slide.tail = None;
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

    /// The top dialog (the popup over the screen's).
    fn top(&mut self) -> Option<&mut (Dialog, Open)> {
        if self.popup.is_some() { self.popup.as_mut() } else { self.dlg.as_mut() }
    }

    /// The font of control `id` in the top dialog (0x41fb30 by its flags).
    fn ctl_font(&mut self, files: &Storage, id: i16) -> Option<crate::ui::Font> {
        let flags = self.top()?.0.get(id)?.flags;
        let p = self.palette_id();
        let skin = &self.look(files, p).skin;
        Some(skin.fonts[match flags & 0x4c00 {
            0x400 => 0,
            0x800 => 2,
            0x4000 => 3,
            _ => 1,
        }].clone())
    }

    /// 0x418270 (`mask`): the first control with a flag of `mask` that is shown, enabled
    /// and (with an id >= 0) has flag 0x10 is pressed (event 3).
    fn press_flagged(&mut self, files: &Storage, mask: u32) -> bool {
        let Some((dlg, _)) = self.top() else { return false };
        let Some(c) = dlg.controls.iter().find(|c| c.flags & mask != 0) else { return false };
        let f = c.flags;
        if f & crate::dlg::flag::VISIBLE == 0 || f & crate::dlg::flag::DISABLED != 0 || c.id == -1 || (c.id >= 0 && f & 0x10 == 0) {
            return false;
        }
        let id = c.id;
        self.glue_sound_hook(1);
        self.activate(files, id);
        true
    }

    /// The controls of the top dialog that take keys (0x418a10: types 0x51a17c, scroll
    /// bars, edits, lists, combos) and are shown, enabled, flag 0x10.
    fn key_controls(&mut self) -> Vec<(i16, u16)> {
        let Some((dlg, _)) = self.top() else { return Vec::new() };
        dlg.controls.iter().filter(|c| matches!(c.ty, 6 | 7 | 8 | 12 | 13) && c.flags & 0x1a == 0x18).map(|c| (c.id, c.ty)).collect()
    }

    /// A key went down (WM_KEYDOWN, event 0; `vk` the virtual key, `mods` +0xa: 8 Alt).
    /// 0x418eb0: Alt + a hotkey letter presses its button (0x418170); else the controls
    /// that take keys get it (0x418a10): a list moves its selection (0x41a9d0), an edit
    /// its caret (0x416980).
    pub fn key_down(&mut self, files: &Storage, vk: u16, mods: u8) {
        if self.credits_open_now() {
            return self.credits_key(files, vk);
        }
        if mods & 8 != 0 {
            let ch = (vk as u8).to_ascii_lowercase();
            if self.press_hotkey(files, ch) {
                return;
            }
        }
        for (id, ty) in self.key_controls() {
            let Some((dlg, open)) = self.top() else { return };
            let open = *open;
            let Some(c) = dlg.get_mut(id) else { continue };
            match ty {
                12 => {
                    let Some(r) = c.list_key(vk) else { continue };
                    if let Some(r) = r {
                        self.list_event(id, r);
                        if let Some(c) = self.top().and_then(|(d, _)| d.get_mut(id)) {
                            let sel = c.list.as_ref().map_or(0xff, |l| l.sel);
                            c.list_scroll_to(sel);
                        }
                    }
                    return;
                }
                8 => {
                    if c.edit_key(vk, mods) {
                        if open == Open::NewId {
                            // 0x4b6c30 -> 0x4b68c0.
                            Self::edit_ok_state(dlg);
                        }
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// 0x418510's second pass: the button whose hotkey (text byte 0, flag 0x200) is `ch`
    /// is pressed when shown, enabled and (id >= 0) flag 0x10.
    fn press_hotkey(&mut self, files: &Storage, ch: u8) -> bool {
        let Some((dlg, _)) = self.top() else { return false };
        let hit = dlg.controls.iter().find(|c| {
            let f = c.flags;
            f & 0x200 != 0
                && c.text.first() == Some(&ch)
                && f & crate::dlg::flag::VISIBLE != 0
                && f & crate::dlg::flag::DISABLED == 0
                && c.id != -1
                && (c.id < 0 || f & 0x10 != 0)
        });
        let Some(id) = hit.map(|c| c.id) else { return false };
        self.glue_sound_hook(1);
        self.activate(files, id);
        true
    }

    /// A typed character (WM_CHAR, event 0xf; 0x418ab0): Enter presses the button with
    /// id -2 or flag 0x80000, Escape id -3 or flag 0x40, Space the active control; else
    /// a hotkey (none while the dialog has an edit, 0x418510), else the controls that
    /// take keys (an edit types it).
    pub fn char_in(&mut self, files: &Storage, ch: u8) {
        match ch {
            9 => {
                todo_port!("0x418a60", "Tab: the next control takes the focus");
                return;
            }
            0x0d => {
                if self.press_id(files, -2) || self.press_flagged(files, 0x80000) {
                    return;
                }
            }
            0x1b => {
                if self.press_id(files, -3) || self.press_flagged(files, 0x40) {
                    return;
                }
            }
            0x20 => {
                let act = self.top().and_then(|(d, _)| d.active.map(|i| &d.controls[i])).filter(|c| matches!(c.ty, 1 | 2 | 14)).map(|c| c.id);
                if let Some(id) = act {
                    self.glue_sound_hook(1);
                    return self.activate(files, id);
                }
            }
            _ => {}
        }
        // 0x417e30: an edit that takes keys turns hotkeys off.
        let has_edit = self.top().is_some_and(|(d, _)| d.controls.iter().any(|c| c.ty == 8 && c.flags & 0x1a == 0x18));
        if !has_edit && self.press_hotkey(files, ch.to_ascii_lowercase()) {
            return;
        }
        for (id, ty) in self.key_controls() {
            if ty != 8 {
                continue;
            }
            let font = self.ctl_font(files, id);
            let refuse = self.edit_refuse.clone();
            let Some((dlg, open)) = self.top() else { return };
            let open = *open;
            let Some(c) = dlg.get_mut(id) else { continue };
            let Some(font) = font else { continue };
            if open == Open::NewId {
                // 0x4b6b10: refused characters (0x51a70c and the caller's list) do
                // nothing; a preset text goes on the first key; a full buffer takes only
                // backspace; a space does not start a name.
                const NEVER: &[u8] = b"*\\/:><|&+^?-";
                if NEVER.contains(&ch) || refuse.contains(&ch) {
                    return;
                }
                if let Some(e) = c.edit.as_mut().filter(|e| e.preset) {
                    e.preset = false;
                    e.caret = 0;
                    e.mode |= 1;
                    c.text.clear();
                }
                let max = c.edit.as_ref().map_or(0x100, |e| e.max);
                if ((c.text.len() as u32) < max.wrapping_sub(1) || ch == 8) && !(ch == 0x20 && c.text.is_empty()) {
                    c.edit_char(ch, &font);
                    c.edit_rescroll(&font);
                }
                Self::edit_ok_state(dlg);
                return;
            }
            if c.edit_char(ch, &font) {
                c.edit_rescroll(&font);
                return;
            }
        }
    }

    /// 0x4182f0: the button (type 1 / 2) with id `id` is pressed when it can be.
    fn press_id(&mut self, files: &Storage, id: i16) -> bool {
        let ok = self.top().and_then(|(d, _)| d.get(id)).is_some_and(|c| {
            matches!(c.ty, 1 | 2) && c.flags & crate::dlg::flag::VISIBLE != 0 && c.flags & crate::dlg::flag::DISABLED == 0
        });
        if ok {
            self.activate(files, id);
        }
        ok
    }

    /// The left button went down (event 4, or 6 for a second press on the same control
    /// within 500 ms): a list highlights the row under the mouse (0x41ab20) or, on a
    /// double click, presses the default button (0x418270 flag 0x80000); an edit takes
    /// the focus and puts its caret under the mouse (0x416980 event 4 / 6, 0x4164d0).
    pub fn press(&mut self, files: &Storage) {
        if self.credits_open_now() {
            return self.credits_click(files);
        }
        let (mx, my) = self.mouse;
        let clock = self.clock_ms;
        let Some((dlg, _)) = self.top() else { return };
        let Some(i) = dlg.hit(mx, my) else { return };
        let (ox, oy) = dlg.origin();
        let (id, ty) = (dlg.controls[i].id, dlg.controls[i].ty);
        let dbl = self.last_press.is_some_and(|(t, last)| last == id && clock.wrapping_sub(t) < 500);
        self.last_press = if dbl { None } else { Some((clock, id)) };
        match ty {
            12 => {
                if dbl {
                    self.press_flagged(files, 0x80000);
                    return;
                }
                let Some((dlg, _)) = self.top() else { return };
                let c = &mut dlg.controls[i];
                // 0x415af0 event 4 on the list's scroll bar child.
                if c.list_scroll_click(mx - ox, my - oy) {
                    return;
                }
                if c.list_inside(mx - ox, my - oy) {
                    let r = c.list_row_at(mx - ox, my - oy);
                    if let Some(l) = c.list.as_mut() {
                        l.hover = r;
                        l.pressed = true;
                    }
                    c.list_scroll_to(r);
                }
            }
            13 => {
                // 0x41bc40 event 4: 0x41a580 opens the drop-down, then 0x41ab20 as a list.
                let p = self.palette_id();
                self.look(files, p);
                let skin = &self.looks[p].as_ref().unwrap().skin;
                let top = if self.popup.is_some() { self.popup.as_mut() } else { self.dlg.as_mut() };
                let Some((dlg, _)) = top else { return };
                let c = &mut dlg.controls[i];
                if c.flags & crate::dlg::flag::DISABLED != 0 {
                    return;
                }
                skin.combo_proc(c, 4, 0);
                if c.list_inside(mx - ox, my - oy) {
                    let r = c.list_row_at(mx - ox, my - oy);
                    if let Some(l) = c.list.as_mut() {
                        l.hover = r;
                    }
                    c.list_scroll_to(r);
                }
                if let Some(l) = c.list.as_mut() {
                    l.pressed = true;
                }
            }
            8 => {
                let font = self.ctl_font(files, id);
                let Some((dlg, _)) = self.top() else { return };
                if let Some(font) = font {
                    dlg.controls[i].edit_click(mx - ox, &font);
                }
            }
            _ => {}
        }
    }

    /// The mouse moved with the button down (event 3 to the captured list, 0x41ae90): the
    /// row under it is highlighted.
    pub fn drag(&mut self) {
        let (mx, my) = self.mouse;
        let Some((dlg, _)) = self.top() else { return };
        let (ox, oy) = dlg.origin();
        for c in dlg.controls.iter_mut().filter(|c| c.list_scroll_active()) {
            c.list_scroll_drag(mx - ox, my - oy);
        }
        for c in dlg.controls.iter_mut().filter(|c| c.list.as_ref().is_some_and(|l| l.pressed)) {
            if c.list_inside(mx - ox, my - oy) {
                let r = c.list_row_at(mx - ox, my - oy);
                let l = c.list.as_mut().unwrap();
                if l.hover != r {
                    l.hover = r;
                    c.list_scroll_to(r);
                }
            }
        }
    }

    /// The left button went up (event 5): a list that was pressed selects the highlighted
    /// row (0x41a490: event 0xb when it differs and is a row).
    pub fn release(&mut self) {
        let (mx, my) = self.mouse;
        let Some((dlg, _)) = self.top() else { return };
        let (ox, oy) = dlg.origin();
        let mut ev = None;
        for c in dlg.controls.iter_mut() {
            if c.list_scroll_active() {
                c.list_scroll_release(mx - ox, my - oy);
            }
            // 0x41bc40 event 5: a combo closes (0x41a4e0) before 0x41a490 selects.
            if c.ty == 13 && c.list.as_ref().is_some_and(|l| l.pressed) {
                c.combo_close();
            }
            let Some(l) = c.list.as_mut().filter(|l| l.pressed) else { continue };
            l.pressed = false;
            if l.hover != l.sel && (l.hover as usize) < l.items.len() {
                ev = Some((c.id, l.hover));
            }
        }
        if let Some((id, r)) = ev {
            self.list_event(id, r);
        }
    }

    /// The mouse moved to (x, y): the dialog's active control (0x419640) lights up.
    pub fn mouse_to(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        self.hover_sound();
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
    pub fn tick(&mut self, files: &Storage, mut ms: u32) {
        // Long waits (scripts, slow frames) run as several 20 ms passes, as the message loop
        // would: the timers fire once per pass.
        while ms > 20 {
            self.tick_pass(files, 20);
            ms -= 20;
        }
        self.tick_pass(files, ms);
    }

    fn tick_pass(&mut self, files: &Storage, ms: u32) {
        self.clock_ms = self.clock_ms.wrapping_add(ms);
        self.slide_tick(files, ms);
        self.create_tick(files, ms);
        self.score_tick(ms);
        // 0x4edaa0 event 8 code 0x57: the timer goes (0x416000), New ID opens.
        if let Some(left) = self.login_timer {
            if left > ms {
                self.login_timer = Some(left - ms);
            } else if self.popup.is_none() && self.dlg.as_ref().is_some_and(|(_, o)| *o == Open::Login) {
                self.login_timer = None;
                self.login_new_id(files);
            }
        }
        for (d, _) in [self.dlg.as_mut(), self.popup.as_mut()].into_iter().flatten() {
            d.tick(ms);
        }
        if let Some(f) = self.fade.as_mut() {
            f.ms += ms;
            if f.ms / 10 >= f.steps.len() as u32 {
                self.fade = None;
            }
        }
        self.credits_tick(files, ms);
        self.ready_tick(files, ms);
        if let (Some(r), true) = (self.result, matches!(self.dlg, Some((_, Open::Ready(_))))) {
            // The idle proc 0x46d340 ended the ready room.
            self.result = None;
            if let Some((_, Open::Ready(race))) = self.dlg.take() {
                self.done_ready(files, r, race);
            }
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
        // The credits' own palette; their old page while it fades out.
        let credits = self.credits_open_now();
        let main = self.credits.old().filter(|_| credits).or(self.dlg.as_ref().map(|d| &d.0));
        for dlg in main.into_iter().chain(self.popup.as_ref().map(|d| &d.0)) {
            look.skin.draw(dlg, &mut s);
        }
        let screen_pal = if credits { *self.credits.palette() } else { look.pal };
        // 0x4be120: the frame's grp box sits at the mouse minus 0x3f.
        let n = look.arrow.frame_count().max(1);
        look.arrow.draw(self.cursor_frame % n, false, self.mouse.0 - 0x3f, self.mouse.1 - 0x3f, s.px, 640, 480, Blend::Normal);
        // 0x41e5c0: the fade's blend, then the gamma (0x41dc20).
        let pal = match &self.fade {
            Some(f) => {
                let (step, total) = f.steps[(f.ms / 10).min(f.steps.len() as u32 - 1) as usize];
                palette::blend(&f.from, f.to.as_ref().unwrap_or(&screen_pal), step, total)
            }
            None if self.pal_blank => [[0; 3]; 256],
            None => screen_pal,
        };
        palette::gamma(&pal, self.gamma)
    }
}

/// Whether a click on `c` activates it (event 0xe code 2 to the dialog): buttons,
/// option / check boxes and light-up buttons; lists, edits, images and labels do not.
fn activates(c: &crate::dlg::Ctl) -> bool {
    matches!(c.ty, 1..=4 | 14)
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
/// background of 0x4dd9e0 (BackGnd.pcx) and the fonts (0x4daec0, font set 1: font10,
/// font14, font16, font16x in 0x6ce0f4..0x6ce100).
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
            Font::parse(read_font(files, "font/font10.fnt")?),
            Font::parse(read_font(files, "font/font14.fnt")?),
            Font::parse(read_font(files, "font/font16.fnt")?),
            Font::parse(read_font(files, "font/font16x.fnt")?),
        ];
        // 0x4dd9e0: the dialog loads <dir>\BackGnd.pcx; its palette is the screen's.
        let back = files.read(&format!("{dir}/BackGnd.pcx"))?;
        let pal = pcx_palette(&back);
        let (bw, bh, bpx) = crate::view::pcx(&back);
        // 0x4bdd60 (tileset 0, as at startup): the colors the interface colors skip.
        let excluded = crate::bw::minimap::excluded_colors(0);
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
    // SCBW_SCORE_TEST=1: the score screen of a finished two player game (no game played).
    if std::env::var_os("SCBW_SCORE_TEST").is_some() {
        glue.score_test_game();
    }
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
    let mut was_down = false;
    let mut was_rdown = false;
    let mut held: Option<i16> = None;
    let mut script: std::collections::VecDeque<String> = std::env::var("SCBW_GLUE_INPUT").map(|v| v.split(';').map(str::to_string).collect()).unwrap_or_default();
    let mut popped = false;
    // Typed characters (WM_CHAR).
    struct Typed(std::rc::Rc<std::cell::RefCell<Vec<u32>>>);
    impl minifb::InputCallback for Typed {
        fn add_char(&mut self, c: u32) {
            self.0.borrow_mut().push(c);
        }
    }
    let typed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    if let Some(w) = win.as_mut() {
        w.set_input_callback(Box::new(Typed(typed.clone())));
    }
    let mut audio = if shot.is_none() { crate::audio::Audio::open(files) } else { None };
    let dat = crate::dat::Dat::load(files).ok();
    let mut last = std::time::Instant::now();
    // SCBW_CINE=<number>: plays that cinematic first (screenshot mode: its frame
    // SCBW_CINE_FRAME to the PNG).
    if let Some(i) = std::env::var("SCBW_CINE").ok().and_then(|s| s.parse::<u32>().ok()) {
        cine::play(files, i, win.as_mut(), audio.as_ref(), shot)?;
        if win.is_none() {
            return Ok(());
        }
    }
    loop {
        match glue.top_step(files) {
            Top::Glue => {}
            Top::Quit => return Ok(()),
            Top::Cine(list) => {
                // (Screenshot mode plays none.)
                let shotmode = win.is_none();
                for i in list.into_iter().filter(|_| !shotmode) {
                    cine::play(files, i as u32, win.as_mut(), audio.as_ref(), shot)?;
                }
            }
            Top::Credits => todo_port!("0x4d8fd0", "credits"),
            Top::Game => {
                // 0x4e0710: the game; the score screen / next state after it (0x4d9950's
                // result in 0x596904) are not ported.
                let Some(g) = glue.begin_game(files) else { return Ok(()) };
                // Where the in-game save / load dialogs keep the saves (0x4ce5b0's inputs).
                crate::menu::set_save_env(crate::menu::SaveEnv {
                    exe_dir: crate::saveload::exe_dir(&glue.setup.characters_dir),
                    profile: glue.player_name.as_bytes().to_vec(),
                    multi: glue.multiplayer,
                    expansion: glue.expansion,
                });
                let mut a = crate::view::Assets::load(crate::storage::Storage::open(&crate::install_dir())?)?;
                if let (Some(_), Ok(spec)) = (shot, std::env::var("SCBW_GLUE_GAME")) {
                    glue.game_end = crate::headless_game(&mut a, g, &spec)?;
                    glue.objectives = glue.game_end.as_ref().map_or(glue.objectives, |e| e.objectives);
                    match &glue.game_end {
                        Some(e) => glue.state = if e.results[e.local as usize] == 2 { 7 } else { 6 },
                        None => return Ok(()),
                    }
                    continue;
                }
                if let Some(out) = shot {
                    return crate::shot_setup(&mut a, g, out);
                }
                // 0x4e0710: the next state is the game's result 0x51ce90 (0x4d9950): 6 / 7 after
                // the victory / defeat dialog's OK (the score screen), 4 after quitting the
                // mission (0x4c9280: the campaign screen 6, or 0x16 with Brood War 0x58f440).
                glue.game_end = crate::view::run_setup(&mut a, g)?;
                glue.objectives = glue.game_end.as_ref().map_or(glue.objectives, |e| e.objectives);
                match &glue.game_end {
                    Some(e) => glue.state = if e.results[e.local as usize] == 2 { 7 } else { 6 },
                    None => {
                        glue.state = state::GLUE;
                        glue.screen = 6 + if glue.expansion { 0x10 } else { 0 };
                    }
                }
                continue;
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
        for cmd in std::mem::take(&mut glue.cmds) {
            let (Some(au), Some(dat)) = (audio.as_mut(), dat.as_ref()) else { continue };
            match cmd {
                AudioCmd::Sound(id) => au.play_pan(files, dat, id, 99, 0, None),
                AudioCmd::Music { path, fade } => {
                    if fade {
                        todo_port!("0x4a6030", "music fade-in timer: needs an Audio hook");
                    }
                    au.play_music(files, path)
                }
                AudioCmd::Track(i) => au.start_track(files, i),
                AudioCmd::FadeOutMusic => au.fade_out(),
                AudioCmd::StopSounds => au.stop_sounds(),
                AudioCmd::Wav(d) => au.play_file(d),
                AudioCmd::StopWavs => au.stop_files(),
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
            // SCBW_GLUE_INPUT="c x y;k vk;t text;w ms" (screenshot mode): clicks, keys,
            // typed text and waits, one per pass, then the shot.
            if let Some(step) = script.pop_front() {
                let a: Vec<&str> = step.split_whitespace().collect();
                let num = |i: usize| a.get(i).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
                match a.first().copied() {
                    Some("c") => {
                        glue.mouse_to(num(1), num(2));
                        glue.press(files);
                        let top = glue.popup.as_ref().or(glue.dlg.as_ref());
                        let hit = top.and_then(|(dlg, _)| dlg.hit(num(1), num(2)).filter(|&i| activates(&dlg.controls[i])).map(|i| dlg.controls[i].id));
                        glue.release();
                        if let Some(id) = hit {
                            glue.mouse_down();
                            glue.activate(files, id);
                        }
                    }
                    // d x y: the button goes down (held); m x y: the mouse moves while held;
                    // u x y: it goes up (a held button activates as on a click).
                    Some("d") => {
                        glue.mouse_to(num(1), num(2));
                        glue.press(files);
                        let top = glue.popup.as_ref().or(glue.dlg.as_ref());
                        held = top.and_then(|(dlg, _)| dlg.hit(num(1), num(2)).filter(|&i| activates(&dlg.controls[i])).map(|i| dlg.controls[i].id));
                    }
                    Some("m") => {
                        glue.mouse_to(num(1), num(2));
                        glue.drag();
                    }
                    Some("u") => {
                        glue.mouse_to(num(1), num(2));
                        glue.release();
                        let top = glue.popup.as_ref().or(glue.dlg.as_ref());
                        let hit = top.and_then(|(dlg, _)| dlg.hit(num(1), num(2)).map(|i| dlg.controls[i].id));
                        if let Some(id) = held.take().filter(|&id| hit == Some(id)) {
                            glue.activate(files, id);
                        }
                    }
                    Some("k") => {
                        let vk = u16::from_str_radix(a.get(1).unwrap_or(&"0").trim_start_matches("0x"), 16).unwrap_or(0);
                        glue.key_down(files, vk, 0);
                        if matches!(vk, 8 | 9 | 0x0d | 0x1b) {
                            glue.char_in(files, vk as u8);
                        }
                    }
                    Some("t") => {
                        for c in step[2..].bytes() {
                            glue.char_in(files, c);
                        }
                    }
                    Some("w") => glue.tick(files, num(1) as u32),
                    _ => {}
                }
                continue;
            }
            return crate::write_png(shot.unwrap(), w, h, &idx, &pal);
        };
        if !win.is_open() {
            return Ok(());
        }
        // Keys (WM_KEYDOWN with repeat: virtual key codes) and characters (WM_CHAR; Enter,
        // Escape, Backspace and Tab come as their control characters).
        {
            use minifb::{Key, KeyRepeat};
            let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
            for k in win.get_keys_pressed(KeyRepeat::Yes) {
                let vk = match k {
                    Key::PageUp => 0x21,
                    Key::PageDown => 0x22,
                    Key::End => 0x23,
                    Key::Home => 0x24,
                    Key::Left => 0x25,
                    Key::Up => 0x26,
                    Key::Right => 0x27,
                    Key::Down => 0x28,
                    Key::Insert => 0x2d,
                    Key::Delete => 0x2e,
                    Key::Enter | Key::NumPadEnter => 0x0d,
                    Key::Escape => 0x1b,
                    Key::Backspace => 8,
                    Key::Tab => 9,
                    _ => match format!("{k:?}").as_bytes() {
                        [c] if c.is_ascii_uppercase() || c.is_ascii_digit() => *c as u16,
                        _ => continue,
                    },
                };
                glue.key_down(files, vk, if alt { 8 } else { 0 });
                if matches!(vk, 8 | 9 | 0x0d | 0x1b) {
                    glue.char_in(files, vk as u8);
                }
            }
            for c in typed.borrow_mut().drain(..) {
                if (0x20..=0xff).contains(&c) && c != 0x7f && !alt {
                    glue.char_in(files, c as u8);
                }
            }
        }
        if let Some((x, y)) = win.get_mouse_pos(MouseMode::Clamp) {
            glue.mouse_to(x as i32, y as i32);
            let down = win.get_mouse_down(MouseButton::Left);
            if down && !was_down {
                glue.press(files);
            } else if down {
                glue.drag();
            } else if was_down {
                glue.release();
            }
            was_down = down;
            // Event 7 (the right button went down): the credits go on (0x4d8d60).
            let rdown = win.get_mouse_down(MouseButton::Right);
            if rdown && !was_rdown && glue.credits_open_now() {
                glue.press(files);
            }
            was_rdown = rdown;
            // 0x418eb0's button behavior: activate on release over the pressed control.
            let top = glue.popup.as_ref().or(glue.dlg.as_ref());
            let hit = top.and_then(|(dlg, _)| dlg.hit(x as i32, y as i32).filter(|&i| activates(&dlg.controls[i])).map(|i| dlg.controls[i].id));
            if win.get_mouse_down(MouseButton::Left) {
                if pressed.is_none() && hit.is_some() {
                    glue.mouse_down();
                }
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
