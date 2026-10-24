//! The in-game menus (opt_dlgs.cpp / modeless.cpp): the game menu (F10 / MENU) and the
//! dialogs it opens. Each is a `.bin` dialog opened by 0x4f57a0 with the shared event
//! proc 0x4ca240 or its own (speed 0x481fa0, sound 0x460e20, video 0x480f90); the
//! activated control id (0x6d051c) goes to the opener's callback (0x6d1234).

use crate::dlg::{flag, Ctl, Dialog, Skin, Surf};
use crate::storage::Storage;
use std::sync::{Mutex, MutexGuard};

/// 0x6cdfd4 (9 dwords, copied by the option dialogs for Cancel) and the video globals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// 0x6cdfd4: 0 slowest .. 6 fastest.
    pub game_speed: i16,
    /// 0x6cdfd8 / 0x6cdfdc: 0 .. 6.
    pub mouse_scroll: i16,
    pub key_scroll: i16,
    /// 0x6cdfe0 / 0x6cdfe4: 0 .. 100 (defaults 25 / 50, 0x4bc450).
    pub music: i16,
    pub digital: i16,
    /// 0x6cdfec: 1 unit speech, 2 acknowledgements, 4 building sounds, 0x400 subtitles.
    pub sound_flags: u32,
    /// 0x6d5c00: CPU throttling.
    pub cpu_throttle: bool,
    /// 0x51a158: 60 .. 140.
    pub gamma: i16,
    /// 0x5137d4.
    pub color_cycling: bool,
    /// 0x515224: 0 disabled, 1 still, 2 animating.
    pub portraits: i16,
    /// 0x6cdfe8: the tip shown (1-based, rez\tips.tbl).
    pub tip: i16,
}

impl Default for Settings {
    fn default() -> Self {
        // 0x420640 (table 0x519f68; the registry values 0x4206a0 are not read): speed 4,
        // scrolls 3, flags 0x200 | 0x800 | 1 | 2 | 4 | 0x100 | 0x1000 (0x400 off). Music /
        // digital are 0x4bc450's 25 / 50; the tip 0 (0x47e7b0 makes it 1, then steps).
        Settings { game_speed: 4, mouse_scroll: 3, key_scroll: 3, music: 25, digital: 50, sound_flags: 0x1b07, cpu_throttle: false, gamma: 100, color_cycling: true, portraits: 2, tip: 0 }
    }
}

/// 0x519f68 (16 entries read by 0x4206a0): kind 1 a number in `lo..=hi` (else `def`) or kind 2
/// a flag bit `mask` of 0x6cdfec, and the registry value name; the targets are 0x6cdfd4..
const OPTION_TABLE: [(u8, &str, u32, u32, u32, u32); 16] = [
    (1, "speed", 0, 6, 4, 0),
    (1, "mscroll", 0, 6, 3, 0),
    (1, "kscroll", 0, 6, 3, 0),
    (1, "m_mscroll", 0, 6, 3, 0),
    (1, "m_kscroll", 0, 6, 3, 0),
    (1, "music", 0, 100, u32::MAX, 0),
    (1, "sfx", 0, 100, u32::MAX, 0),
    (1, "tipnum", 0, u32::MAX, 0, 0),
    (2, "intro", 0, 0, 1, 0x200),
    (2, "introX", 0, 0, 1, 0x800),
    (2, "unitspeech", 0, 0, 1, 1),
    (2, "unitnoise", 0, 0, 1, 2),
    (2, "bldgnoise", 0, 0, 1, 4),
    (2, "tip", 0, 0, 1, 0x100),
    (2, "trigtext", 0, 0, 0, 0x400),
    (2, "usechatcolors", 0, 0, 1, 0x1000),
];

impl Settings {
    /// The settings at the start: the defaults (0x420640), then the registry values
    /// (0x4206a0, the "Starcraft" key), the options of 0x480c20 and the CPUThrottle of
    /// 0x481cf0. 1.16.1 never writes these values itself; they are set outside the game.
    pub fn load() -> Settings {
        let mut s = Settings::default();
        s.read_table();
        s.read_options();
        if let Some(v) = crate::registry::load_value("CPUThrottle") {
            // 0x481cf0
            s.cpu_throttle = v != 0;
        }
        s
    }

    /// 0x4206a0: each entry of 0x519f68 that has a registry value: a number out of its range
    /// becomes the entry's default (-1 for the volumes: 0x4bc450 sets 25 / 50 then, the
    /// defaults here); a flag is set or cleared by the value being nonzero.
    fn read_table(&mut self) {
        for &(kind, name, lo, hi, def, mask) in OPTION_TABLE.iter() {
            let Some(mut v) = crate::registry::load_value(name) else { continue };
            if kind == 1 {
                if v < lo || hi < v {
                    v = def;
                }
                match name {
                    "speed" => self.game_speed = v as i16,
                    "mscroll" => self.mouse_scroll = v as i16,
                    "kscroll" => self.key_scroll = v as i16,
                    // 0x6cdff0 / 0x6cdff4 (the network game's scrolling) are not kept.
                    "music" if v != u32::MAX => self.music = v as i16,
                    "sfx" if v != u32::MAX => self.digital = v as i16,
                    "tipnum" => self.tip = v as i16,
                    _ => {}
                }
            } else if v == 0 {
                self.sound_flags &= !mask;
            } else {
                self.sound_flags |= mask;
            }
        }
    }

    /// 0x480c20: Gamma (60 ..= 140, else 100), ColorCycle (default 1) and UnitPortraits
    /// (default 2, at most 2). The portrait refresh it ends with (wire_type,
    /// unit_portrait, show_talking) runs only while a game has its portrait (0x68ac98,
    /// 0x597248), which the callers here do not have.
    pub fn read_options(&mut self) {
        use crate::registry::load_value;
        if let Some(v) = load_value("Gamma") {
            self.gamma = if (60..=140).contains(&v) { v as i16 } else { 100 };
        }
        self.color_cycling = load_value("ColorCycle").unwrap_or(1) != 0;
        self.portraits = load_value("UnitPortraits").unwrap_or(2).min(2) as i16;
        crate::todo_port!("0x480c20", "the portrait refresh (wire_type, unit_portrait, show_talking) of a running game");
    }
}

/// Where the saves live: the inputs of 0x4ce5b0 / 0x4cee60 (the caller sets them once with
/// `set_save_env`; the defaults list nothing).
#[derive(Clone, Debug)]
pub struct SaveEnv {
    /// `saveload::exe_dir`.
    pub exe_dir: String,
    /// The player profile's name.
    pub profile: Vec<u8>,
    /// 0x57f0b4.
    pub multi: bool,
    /// 0x58f440: Brood War.
    pub expansion: bool,
}

/// The callback of a message dialog (0x6d124c).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cb {
    /// 0x4620e0: back to the save dialog.
    Reopen,
    /// 0x462150: the delete question.
    Delete,
    /// 0x4621f0: the overwrite question.
    Overwrite,
}

/// The globals of the save / load dialogs (0x685148.., 0x6d1220, 0x6d123c, 0x6d124c).
struct Sg {
    env: SaveEnv,
    /// 0x6d1220: rez\network.tbl as read.
    net: Vec<u8>,
    /// 0x685148: the name typed / chosen.
    name: Vec<u8>,
    /// 0x68516a: the selected row (0xff none).
    sel: u8,
    /// 0x68516c: the id of the save (header +0x28) of the question being asked.
    id: u32,
    /// 0x51a320 (0x685168 = the count): the saves found, newest first (name, id).
    saves: Vec<(Vec<u8>, u32)>,
    /// 0x6d123c / 0x6d124c.
    msg: Vec<u8>,
    cb: Option<Cb>,
    /// 0x685164: when the last save was made (multiplayer allows one per 2 minutes).
    saved_at: Option<web_time::Instant>,
    /// 0x68ec88: the error text of the in-game error message (None = null pointer).
    fatal: Option<Vec<u8>>,
    /// An error was posted from outside the game loop and its dialog is still to open.
    fatal_pending: bool,
}

static SG: Mutex<Sg> = Mutex::new(Sg {
    env: SaveEnv { exe_dir: String::new(), profile: Vec::new(), multi: false, expansion: true },
    net: Vec::new(),
    name: Vec::new(),
    sel: 0,
    id: 0,
    saves: Vec::new(),
    msg: Vec::new(),
    cb: None,
    saved_at: None,
    fatal: None,
    fatal_pending: false,
});

fn sg() -> MutexGuard<'static, Sg> {
    SG.lock().unwrap_or_else(|e| e.into_inner())
}

/// 0x461720 (negated): in multiplayer a save is refused while GetTickCount <= 0x685164 + 120000.
/// 0x685164 starts at 0, so before the first save this is the machine's uptime, which is
/// not known here (taken as past 2 minutes).
fn save_wait(ctx: &Ctx) -> bool {
    ctx.multiplayer && sg().saved_at.is_some_and(|t| t.elapsed().as_millis() <= 120_000)
}

/// 0x4e4a10 (ECX = the message, None = null; EAX = the debug text, "" at the callers): the
/// in-game error message to open (`Kind::Fatal`). `msg` is network.tbl string `id` as
/// 0x4bb300 picks it: None for id 0, "" past the table. The caller opens the dialog (the
/// game loop does it at the next frame when `take_fatal_pending`).
/// 0x51a0e9 (set: no message) is never set here.
pub fn set_fatal(msg: Option<Vec<u8>>) {
    let mut g = sg();
    g.fatal = msg;
    g.fatal_pending = true;
}

/// Whether `set_fatal` posted a message not opened yet (taken).
pub fn take_fatal_pending() -> bool {
    std::mem::take(&mut sg().fatal_pending)
}

/// Where the saves are (what `set_save_env` gave).
pub fn save_env() -> SaveEnv {
    sg().env.clone()
}

/// Tells the save / load dialogs where the saves are (`saveload::exe_dir`, the profile).
pub fn set_save_env(env: SaveEnv) {
    sg().env = env;
}

/// 0x41f1b0 with one string argument, in a 0x100-byte buffer.
fn sprintf_s(fmt: &[u8], arg: &[u8]) -> Vec<u8> {
    let mut o = Vec::new();
    let mut i = 0;
    let mut used = false;
    while i < fmt.len() {
        if fmt[i] == b'%' && fmt.get(i + 1) == Some(&b's') && !used {
            o.extend_from_slice(arg);
            used = true;
            i += 2;
        } else {
            o.push(fmt[i]);
            i += 1;
        }
    }
    o.truncate(0xff);
    o
}

/// 0x4cfa10: the name and id of a save file (None when the file is no save of this
/// version): the text up to 0x1a, the u32 with 0x69 in its high half and the packed
/// 0xb5-byte header (its first word 0x69); the name is the file name without the extension
/// (under 0x1c characters), the id the header's +0x28.
fn save_head(path: &str, fname: &str) -> Option<(Vec<u8>, u32)> {
    let f = std::fs::read(crate::saveload::host(path)).ok()?;
    let mut i = crate::bw::save::In::new(&f);
    // 0x4ce470
    while i.bytes(1)?[0] != 0x1a {}
    // 0x4cf0f0
    if i.u32()? & 0xffff_0000 != 0x0069_0000 {
        return None;
    }
    let h = i.packed(0xb5)?;
    if u16::from_le_bytes([h[0], h[1]]) != 0x69 {
        return None;
    }
    let mut n: Vec<u8> = fname.bytes().take(0x1f).collect();
    if let Some(dot) = n.iter().rposition(|&c| c == b'.') {
        n.truncate(dot);
    }
    (n.len() < 0x1c).then(|| (n, u32::from_le_bytes(h[0x28..0x2c].try_into().unwrap())))
}

/// 0x4cfd90 with the callbacks 0x4cfb00 and 0x461800: the saves of the save directory
/// (files "*<ext>", names under 0x20 characters, hidden ones skipped), sorted by write
/// time with the newest first, at most 0x80.
fn read_saves(env: &SaveEnv) -> Vec<(Vec<u8>, u32)> {
    use crate::saveload as sl;
    let Some(dir) = sl::save_dir(&env.exe_dir, &env.profile, env.multi, false, 0x104) else { return Vec::new() };
    let ext = sl::save_ext(env.expansion, env.multi);
    let Ok(rd) = std::fs::read_dir(sl::host(&dir)) else { return Vec::new() };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    // 0x4cfb00: each file is put before the first one not older, so the list ascends.
    let mut found: Vec<(std::time::SystemTime, Vec<u8>, u32)> = Vec::new();
    for e in ents {
        let Ok(fname) = e.file_name().into_string() else { continue };
        if fname.starts_with('.') || fname.len() >= 0x20 || !fname.to_ascii_lowercase().ends_with(ext) || !e.path().is_file() {
            continue;
        }
        let path = format!("{}{}", dir, fname);
        let Some((name, id)) = save_head(&path, &fname) else { continue };
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        let at = found.iter().position(|f| t <= f.0).unwrap_or(found.len());
        found.insert(at, (t, name, id));
    }
    // 0x461800 puts each at the head of its list.
    let mut saves = Vec::new();
    for (_, name, id) in found {
        if saves.len() < 0x80 {
            saves.insert(0, (name, id));
        }
    }
    saves
}

/// 0x4617c0: the save called `name` (case insensitive).
fn find_save<'a>(saves: &'a [(Vec<u8>, u32)], name: &[u8]) -> Option<&'a (Vec<u8>, u32)> {
    saves.iter().find(|s| s.0.eq_ignore_ascii_case(name))
}

/// 0x4cf820: delete the save `name` (the save dir created when missing).
fn delete_save(env: &SaveEnv, name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name);
    let Some(p) = crate::saveload::save_path(&env.exe_dir, &env.profile, env.multi, env.expansion, false, &name) else { return false };
    let p = crate::saveload::host(&p);
    if let Ok(m) = std::fs::metadata(&p) {
        let mut perm = m.permissions();
        if perm.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(&p, perm);
        }
    }
    std::fs::remove_file(p).is_ok()
}

/// 0x418e00: the control can be used again.
fn enable(dlg: &mut Dialog, id: i16) {
    if let Some(c) = find(dlg, id) {
        c.flags &= !flag::DISABLED;
    }
}

/// 0x416650: the edit's text is replaced (the caret goes to its end).
fn edit_set(skin: &Skin, c: &mut Ctl, text: &[u8]) {
    c.text = text.iter().copied().take(0xff).collect();
    let font = skin.font_of(c).clone();
    if let Some(e) = c.edit.as_mut() {
        e.caret = c.text.len() as u8;
        e.mode |= 1;
    }
    c.edit_rescroll(&font);
}

/// 0x461a90: the delete button works when a row is selected.
fn delete_state(dlg: &mut Dialog) {
    let on = dlg.get(1).and_then(|c| c.list.as_ref()).is_some_and(|l| !l.items.is_empty() && l.sel != 0xff);
    if on {
        enable(dlg, 3);
    } else {
        disable(dlg, 3);
    }
}

/// 0x4618f0: the edit shows the text of the selected row.
fn sync_edit(skin: &Skin, dlg: &mut Dialog) {
    let t = dlg.get(1).and_then(|c| c.list.as_ref()).filter(|l| !l.items.is_empty() && l.sel != 0xff).and_then(|l| l.items.get(l.sel as usize)).map(|i| i.0.clone());
    if let (Some(t), Some(e)) = (t, find(dlg, 2).filter(|c| c.edit.is_some())) {
        edit_set(skin, e, &t);
    }
}

/// 0x461e80 event 0xb: the list selects `row` (0x41baa0), then the edit and the delete
/// button follow.
fn list_event(skin: &Skin, dlg: &mut Dialog, row: u8) {
    if let Some(c) = find(dlg, 1) {
        c.list_select(row);
    }
    sync_edit(skin, dlg);
    delete_state(dlg);
}

fn trim_end(mut t: Vec<u8>) -> Vec<u8> {
    while t.last() == Some(&b' ') {
        t.pop();
    }
    t
}

/// What opened the game is running as (0x6d0f14 replay, 0x57f0b4 multiplayer).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ctx {
    pub replay: bool,
    pub multiplayer: bool,
    /// 0x6d0f6a: a replay's speed, shown on the (disabled) game speed slider.
    pub replay_speed: i16,
    /// 0x51268c: the local player's storm id.
    pub local_storm: u32,
    /// 0x66fe20 (stride 0x22): per storm id, the net slot is connected (1).
    pub net_slot: [u8; 8],
    /// 0x596877 (game template +0x12): set, the game cannot be saved.
    pub no_save: u8,
    /// 0x6d1264 == 0 (no sound device): the sound dialog's sliders and boxes 3..5 are disabled
    /// (0x460bb0 / 0x460b20). Negated so that `Default` keeps the sound on.
    pub no_sound: bool,
    /// 0x6509c4: the game is paused (the game menu's Pause / Resume, 0x4c9890).
    pub paused: bool,
    /// 0x57eee8 (stride 0x24) of the local slot 0x512688 (2: a human player).
    pub player_type: u8,
    /// 0x66fbfa (the game menu shows Pause in multiplayer above 8).
    pub g66fbfa: u8,
    /// 0x58d718 of the local slot: the pauses left.
    pub pauses_left: u8,
    /// 0x58d700 of the local slot 0x512684 (3: won).
    pub local_result: u8,
    /// 0x57f0b4 with 0x59688c == 'BNET'.
    pub bnet: bool,
    /// 0x4a2ca0: elapsed game seconds when the menu opens.
    pub elapsed_secs: u32,
    /// 0x4c4220's result of the local player (`setup.player_outcomes`), read by the victory dialog.
    pub outcome: u32,
    /// 0x581d61 == 2 (a team game: the victory dialog's team lines).
    pub team_result: bool,
}

impl Ctx {
    /// 0x4c3db0: the lowest storm id of a connected human (the local player's in a single
    /// player game).
    pub fn lowest_human(&self) -> u32 {
        if !self.multiplayer {
            return self.local_storm;
        }
        (0..8).find(|&i| self.net_slot[i] == 1).unwrap_or(8) as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Game,
    Options,
    Speed,
    Sound,
    Video,
    Help,
    /// Opened by 0x47ea30 (0x658ae0 = eax; dialog 0x5045f8, init 0x47e9a0) at the start of a
    /// single player game by 0x4d9950 when 0x6cdfec & 0x100 (view.rs, at the first tick).
    Tips,
    Objectives,
    EndMission,
    Restart,
    QuitMission,
    Quit,
    /// 0x461430: the defeat dialog (0x4615b0).
    Defeat,
    /// 0x461510: the victory (and draw) dialog (0x461640).
    Victory,
    /// 0x462600: rez\savegame.bin (event proc 0x462580).
    SaveGame,
    /// 0x462100: rez\loadgame.bin (event proc 0x462060).
    LoadGame,
    /// 0x4f6050: the message dialog rez\okcancel.bin (event proc 0x4f5cb0); the text is
    /// 0x6d123c, the callback 0x6d124c (`Cb`).
    OkCancel,
    /// 0x4f6050 with rez\ok.bin.
    Ok,
    /// 0x4e4a10: the in-game error message, rez\dlgFatal.bin (event proc 0x4e4980).
    Fatal,
    /// 0x4913d0: the minimap's replay vision button (control 4, 0x4a54f0): rezllyfltr.bin
    /// (0x5042bc), event proc 0x491310, init 0x490e90 (`Menu::vision_init`).
    ReplayVision,
}

impl Kind {
    fn bin(self, replay: bool) -> &'static str {
        match self {
            Kind::Game => "rez/gamemenu.bin",
            Kind::Options => "rez/options.bin",
            Kind::Speed => "rez/spd_dlg.bin",
            Kind::Sound => "rez/snd_dlg.bin",
            Kind::Video => "rez/video.bin",
            Kind::Help => "rez/helpmenu.bin",
            Kind::Tips => "rez/tips_dlg.bin",
            Kind::Objectives => "rez/objctdlg.bin",
            Kind::EndMission => "rez/abrtmenu.bin",
            Kind::Restart => "rez/restart.bin",
            // 0x4ca2f0
            Kind::QuitMission if replay => "rez/quitrepl.bin",
            Kind::QuitMission => "rez/quit2mnu.bin",
            Kind::Quit => "rez/quit.bin",
            // 0x504880 / 0x504894
            Kind::Defeat => "rez/lmission.bin",
            Kind::Victory => "rez/wmission.bin",
            // 0x501680 / 0x504818 / 0x502ab8 / 0x502acc
            Kind::SaveGame => "rez/savegame.bin",
            Kind::LoadGame => "rez/loadgame.bin",
            Kind::OkCancel => "rez/okcancel.bin",
            Kind::Ok => "rez/ok.bin",
            Kind::Fatal => "rez/dlgFatal.bin",
            // 0x5042bc (0x5042d0 rez	eamfltr.bin in a team game, not ported)
            Kind::ReplayVision => "rez/allyfltr.bin",
        }
    }
}

/// What the game does after a menu action.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// The menu closed (0x419ea0).
    Closed,
    /// Game menu 7 / 8 (0x4c0450 / 0x4c0420).
    Pause,
    Resume,
    /// The settings changed (OK in an option dialog).
    Apply,
    /// 0x481ef0 (the speed dialog's OK): send command 0x0f with this speed (0x485bd0,
    /// `Scene::send(&[0x0f, speed])`); the settings changed too.
    GameSpeed(u8),
    Restart,
    /// Quit the mission / replay.
    QuitMission,
    /// 0x4c9280 in a multiplayer game: quit to the score screen (0x51ce90 = 6 won / 7 else).
    QuitScore(u8),
    ExitProgram,
    /// OK in the defeat / victory dialog (0x460f30 / 0x460fa0): the game ends and the
    /// score screen follows (0x51ce90 = 7 / 6).
    EndGame,
    /// 0x4c0600 (from 0x4623c0 / 0x4621f0): the save command 6 with the name (<= 27
    /// characters, no extension) the player typed; the dialog is closed. The caller
    /// writes <save dir>\<name><ext> (`saveload::save_path`, `State::save_state`).
    SaveGame(String),
    /// 0x4619a0: the load command 7: the name of the chosen save (no extension) and its
    /// id (the header's +0x28, the game info's 0x596814). The dialog is closed. The caller
    /// opens it (`saveload::open_save`) and loads the state.
    LoadGame(String, u32),
    /// 0x460fa0 case 1 (the victory dialog's button 1): the dialog closes and 0x48d0c0 adds
    /// a message line (7 s, sound 0x17): rez\network.tbl string 0x10 (index 0xf) when the
    /// table has more than 0xf strings, else the empty string 0x501b7d (sound only).
    Message,
    /// 0x490fe0 (the replay vision dialog's OK): the shown players (0x63ff74) and the all
    /// players box; the caller runs `State::set_replay_vision`.
    ReplayVision(u16, bool),
}

/// The open menu (0x6d5bf4) and the depth counter 0x6d1224.
///
/// 0x4c93a0 (the in-game dialogs' teardown, run by 0x460c30 / 0x480d00 / 0x481d30 / 0x4e8920):
/// frees the +0x26 buffer (`saved` drops with the Menu), unpauses with 0x488790 when not
/// multiplayer (view.rs `unpause_game(true)` once `menu` is None), clears 0x6d5bf4 (the
/// Option is None). Not modelled: with 0x596904 == 3 it restores 0x6d120c = 0x596898 and
/// 0x5968e0 = 0x5968fc, and clears 0x6d1214 (replay globals, no Rust state yet).
pub struct Menu {
    pub kind: Kind,
    pub dlg: Dialog,
    /// 0x6d1224: how many menus deep (Previous / OK / Cancel go back while > 1).
    depth: u8,
    /// The settings when the option dialog opened (its +0x26 copy, restored by Cancel).
    saved: Settings,
    /// The control the left button went down on (pressed until released).
    pressed: Option<usize>,
    /// A button was released on itself: the click sound plays (taken by the window loop).
    pub click: bool,
    /// A slider being dragged.
    dragging: Option<usize>,
    /// The left button was down at the last `mouse` (list boxes and edits act on the edges).
    was_down: bool,
    /// Milliseconds ticked (`tick`); None until the caller ticks (no double click then).
    clock: Option<u32>,
    /// The last press on a list: when and the control (a second within 500 ms is a double click).
    last_press: Option<(u32, i16)>,
    /// The dialog's timers (0x416090) and the tick count (GetTickCount) they run by.
    timers: crate::dlg::DlgTimers<()>,
    tick_count: u32,
    /// Milliseconds since the message pump last sent event 0xd (every 100 ms, 0x4d1bf0).
    pump_ms: u32,
    /// What the menu opened with (the 200 ms refreshes of the game and end-mission menus read it).
    ctx: Ctx,
    /// 0x63ff74: the replay vision dialog's shown players (a bit per player).
    vision: u16,
}

/// 0x512768: button graphics by style (0 minimap, 3 OK, 4 menu buttons, 5 Cancel, ...).
const GRAPHICS: [u16; 8] = [3, 83, 0, 103, 112, 121, 130, 140];

fn find(dlg: &mut Dialog, id: i16) -> Option<&mut Ctl> {
    dlg.get_mut(id)
}

fn show(dlg: &mut Dialog, id: i16, on: bool) {
    if let Some(c) = find(dlg, id) {
        // 0x4186a0 / 0x418700
        if on {
            c.flags |= flag::VISIBLE;
        } else {
            c.flags &= !flag::VISIBLE;
        }
    }
}

fn disable(dlg: &mut Dialog, id: i16) {
    // 0x418640
    if let Some(c) = find(dlg, id) {
        c.flags |= flag::DISABLED;
    }
}

/// 0x4e18c0 (ESI = the control): the first box of its option run, found by stepping to the
/// sibling whose id is one below while that one is a type 3 control.
fn option_first(dlg: &Dialog, mut i: usize) -> usize {
    loop {
        let id = dlg.controls[i].id;
        match dlg.controls.iter().position(|c| c.id == id.wrapping_sub(1)) {
            Some(j) if dlg.controls[j].ty == 3 => i = j,
            _ => return i,
        }
    }
}

/// 0x4e1900 (EAX = ESI = the clicked option box): clears the boxes of the run from its
/// first one (0x4e18c0) up through the following ids while they are type 3 (marking each
/// for redraw, flag 1 + 0x41c400), then selects the clicked one.
fn option_click(dlg: &mut Dialog, i: usize) {
    let mut j = option_first(dlg, i);
    loop {
        let c = &mut dlg.controls[j];
        c.checked = false;
        c.flags |= 1;
        let next = c.id.wrapping_add(1);
        match dlg.controls.iter().position(|c| c.id == next) {
            Some(k) if dlg.controls[k].ty == 3 => j = k,
            _ => break,
        }
    }
    dlg.controls[i].checked = true;
}

/// What `option_event` leaves to the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OptionAct {
    /// Handled, return 1.
    Done,
    /// The option click 0x4e1900 (the box is selected, the others of its group cleared).
    Click,
    /// Not handled: the default dispatcher `Ctl::default_event` (0x4e1a70).
    Default,
}

/// 0x4e1bc0 (ECX = the option box, EDX = the event record): `sub` = the record's word at +0xc,
/// `ev` = its number at +0, `data` = its dword at +4. Event 10 raises the box's height to
/// the option graphic's (`op` = 0x6d0520); 2 and 0xb (with `data` set) click it.
pub fn option_event(c: &mut Ctl, sub: u16, ev: u32, data: u32, op: i32) -> OptionAct {
    if sub == 0xe {
        match ev {
            2 => return OptionAct::Click,
            10 => c.rect.3 = c.rect.3.max(c.rect.1 + op),
            0xb => return if data == 0 { OptionAct::Done } else { OptionAct::Click },
            _ => {}
        }
    }
    OptionAct::Default
}

impl Menu {
    /// 0x4f57a0 + the dialog's init (event 0xe, user 0). The option dialogs' open is 0x481e80 /
    /// 0x460d60 / 0x480e70 (`saved` = the copy of the 9 settings dwords).
    /// 0x4cda30: the Cancel / Previous (-3) graphic and the WIDE flag (inline here).
    pub fn open(kind: Kind, depth: u8, files: &Storage, skin: &Skin, ctx: Ctx, set: &Settings) -> Option<Menu> {
        let mut dlg = Dialog::parse(&files.read(kind.bin(ctx.replay)).ok()?);
        dlg.this.flags |= flag::VISIBLE;
        // 0x4c9cc0 / 0x4c90c0
        skin.menu_background(&mut dlg);
        dlg.this.flags |= 0x1004_0000;
        // 0x418100 with 0x51a404: buttons 1..32 get 0x4cdb40 (graphic 0x512770, wide).
        for c in dlg.controls.iter_mut() {
            if (1..=32).contains(&c.id) && matches!(c.ty, 1 | 2) {
                c.graphic = GRAPHICS[4];
                c.flags |= flag::WIDE;
            }
        }
        // 0x4cda80 / 0x4cda30: OK (-2) and Cancel / Previous (-3); the option dialogs
        // (0x481e80, 0x460d60, 0x480e70) use styles 3 and 5. The tips dialog (0x47e8f0,
        // call 0x47e96e) restyles only OK, with style 3.
        let (ok, cancel) = match kind {
            Kind::Tips => (Some(3), None),
            Kind::ReplayVision | Kind::Speed | Kind::Sound | Kind::Video | Kind::SaveGame | Kind::LoadGame | Kind::OkCancel | Kind::Ok => (Some(3), Some(5)),
            _ => (Some(4), Some(4)),
        };
        for (id, style) in [(-2, ok), (-3, cancel)] {
            if let (Some(style), Some(c)) = (style, find(&mut dlg, id)) {
                c.graphic = GRAPHICS[style];
                c.flags |= flag::WIDE;
            }
        }
        let depth = depth + 1;
        let mut timers = crate::dlg::DlgTimers::default();
        match kind {
            Kind::Game => {
                // 0x4ca160 -> 0x4c9a90 (multiplayer or replay)
                if ctx.multiplayer || ctx.replay {
                    game_menu_open(&mut dlg, &ctx);
                    // not in a replay: the 200 ms refresh 0x4c9890
                    if !ctx.replay {
                        timers.set(0, 0, 1, 200, None);
                    }
                }
            }
            // 0x4ca160 -> 0x4c9440 (multiplayer or replay): the restart button goes, the quit
            // button's text follows the game's state (0x4c9150).
            Kind::EndMission if ctx.multiplayer || ctx.replay => {
                show(&mut dlg, 1, false);
                end_mission_label(&mut dlg, &ctx, ctx.elapsed_secs);
                if ctx.bnet {
                    show(&mut dlg, 3, false);
                    timers.set(0, 0, 1, 200, None);
                }
            }
            // 0x4ca160 with 0x4c95a0 (quit2mnu.bin, not a replay): control 1 hidden (0x418700).
            Kind::QuitMission if !ctx.replay => show(&mut dlg, 1, false),
            // 0x4c94f0: no network options in single player.
            Kind::Options if !ctx.multiplayer => show(&mut dlg, 4, false),
            Kind::Speed => {
                // 0x481b80
                let speed = if ctx.replay { ctx.replay_speed } else { set.game_speed };
                for (id, v) in [(1, speed), (2, set.mouse_scroll), (3, set.key_scroll)] {
                    Self::slider(&mut dlg, id, v, 0, 6);
                }
                if ctx.multiplayer || ctx.replay {
                    disable(&mut dlg, 1);
                }
                if let Some(c) = find(&mut dlg, 4) {
                    c.checked = set.cpu_throttle;
                }
            }
            Kind::Sound => {
                // 0x460bb0 / 0x460b20
                Self::slider(&mut dlg, 1, set.music, 0, 100);
                Self::slider(&mut dlg, 2, set.digital, 0, 100);
                for (id, bit) in [(3, 1), (4, 2), (5, 4), (6, 0x400)] {
                    if let Some(c) = find(&mut dlg, id) {
                        c.checked = set.sound_flags & bit != 0;
                    }
                }
                // 0x460bb0 (ids 1, 2) / 0x460b20 (ids 3..5, not 6): 0x418640 without a sound device.
                if ctx.no_sound {
                    for id in 1..=5 {
                        disable(&mut dlg, id);
                    }
                }
            }
            Kind::Video => {
                // 0x480bd0 / 0x480d90 / 0x480ae0
                Self::slider(&mut dlg, 1, set.gamma, 0x3c, 0x8c);
                if let Some(c) = find(&mut dlg, 2) {
                    c.checked = set.color_cycling;
                }
                for id in 3..=5 {
                    if let Some(c) = find(&mut dlg, id) {
                        c.checked = id == set.portraits + 3;
                    }
                }
            }
            Kind::Defeat => {
                // 0x461020: -20 unless a multiplayer team result (0x581d61 == 2), which shows -21;
                // button 1 is hidden. Then a 20 s dialog timer (0x416090). Also: plays "sound\%s" of the
                // string pointer 0x68daf8 when set; nothing in the exe writes it (its only reference is
                // this read), so it is null and no sound plays.
                let team = ctx.multiplayer && ctx.team_result;
                show(&mut dlg, 1, false);
                show(&mut dlg, -0x14, !team);
                show(&mut dlg, -0x15, team);
                timers.set(0, 0, 0, 20000, None);
            }
            Kind::Victory => {
                // 0x461160: button 1 (continue playing) in a live multiplayer game only.
                show(&mut dlg, 1, ctx.multiplayer && !ctx.replay);
                // Team result: -21; else the local player's outcome 3 (not in a replay): -23;
                // else -20.
                let team = ctx.multiplayer && ctx.team_result;
                let won3 = !team && ctx.outcome == 3 && !ctx.replay;
                let line20 = !team && !won3;
                let glu = files.read("rez/gluAll.tbl").unwrap_or_default();
                show(&mut dlg, -0x14, line20);
                if ctx.replay {
                    copy_str(&mut dlg, -0x14, crate::setup::tbl_str(&glu, 0xb5 - 1));
                }
                show(&mut dlg, -0x15, team);
                show(&mut dlg, -0x17, won3);
                // The OK text: a replay's (gluAll 0xbc); in multiplayer -10, or -22 when
                // line 20 is not the one shown (0x4c4220 team / outcome 3).
                if ctx.replay {
                    copy_str(&mut dlg, -2, crate::setup::tbl_str(&glu, 0xbc - 1));
                } else if ctx.multiplayer {
                    copy_text(&mut dlg, -2, if line20 { -10 } else { -0x16 });
                }
            }
            Kind::Tips => {
                if let Some(c) = find(&mut dlg, 2) {
                    c.checked = set.sound_flags & 0x100 != 0;
                }
            }
            Kind::SaveGame | Kind::LoadGame => {
                // 0x461ed0 (event 0xe user 0 of 0x462580 / 0x462060).
                let (saves, sel, name) = {
                    let mut g = sg();
                    g.net = files.read("rez/network.tbl").unwrap_or_default();
                    // 0x4cfd90 with 0x461800
                    g.saves = read_saves(&g.env);
                    (g.saves.iter().map(|s| s.0.clone()).collect::<Vec<_>>(), g.sel, g.name.clone())
                };
                // 0x461d80 (the list's event 0xe user 0): the rows, then the buttons.
                if let Some(c) = find(&mut dlg, 1).filter(|c| c.ty == 12) {
                    skin.list_init(c);
                    c.list.as_mut().unwrap().batch = true;
                    for n in &saves {
                        c.list_add(n, 0);
                    }
                    c.list.as_mut().unwrap().batch = false;
                    c.list_layout();
                }
                if saves.is_empty() {
                    disable(&mut dlg, -2);
                }
                delete_state(&mut dlg);
                // 0x461d40 (the edit's user 0, margin 3) and the 0x416450 init.
                if let Some(c) = find(&mut dlg, 2).filter(|c| c.ty == 8) {
                    skin.edit_init(c);
                    c.flags |= 0x8000_0000 | 0x4000_0000;
                    if let Some(e) = c.edit.as_mut() {
                        e.margin = 3;
                        e.blink = Some(500);
                    }
                }
                // 0x461fc7: the list selects the row of 0x68516a; then the edit shows the
                // typed name, else that row (0x462006).
                list_event(skin, &mut dlg, sel);
                let row = dlg.get(1).and_then(|c| c.list.as_ref()).and_then(|l| l.items.get(sel as usize).map(|i| i.0.clone()));
                if let Some(e) = find(&mut dlg, 2).filter(|c| c.edit.is_some()) {
                    if !name.is_empty() {
                        edit_set(skin, e, &name);
                    } else if let Some(t) = row {
                        edit_set(skin, e, &t);
                    }
                }
            }
            Kind::OkCancel | Kind::Ok => {
                // 0x4f5b70: the text of label 1 is 0x6d123c.
                let msg = sg().msg.clone();
                if let Some(c) = find(&mut dlg, 1) {
                    c.text = msg;
                    c.update = crate::dlg::Update::Align(0x28);
                }
            }
            Kind::Fatal => {
                dlg.bg = None;
                // 0x4e4820 (event 0xe user 0 of 0x4e4980): the dialog's draw is 0x4e46a0.
                dlg.this.update = crate::dlg::Update::Fatal;
                // Label -2 (the button) is drawn by 0x4e4770; labels -10 (the message) and
                // -0x14 (the debug text, empty at the callers) by 0x4e4750 when given.
                if let Some(c) = find(&mut dlg, -2) {
                    c.update = crate::dlg::Update::Fatal;
                }
                let msg = sg().fatal.clone();
                if let Some(c) = msg.and_then(|m| find(&mut dlg, -10).map(|c| (c, m))) {
                    c.0.text = c.1;
                    c.0.update = crate::dlg::Update::Align(0x22);
                }
                if let Some(c) = find(&mut dlg, -0x14) {
                    c.text = Vec::new();
                    c.update = crate::dlg::Update::Align(0x22);
                }
            }
            _ => {}
        }
        // Check / option boxes: 0x4e1b50 / 0x4e1bc0 (user 10, `option_event`): graphic 0xb, at least
        // one box tall.
        let (ck, op) = (skin_check(skin), skin_option(skin));
        for c in dlg.controls.iter_mut() {
            match c.ty {
                4 => {
                    c.graphic = 0xb;
                    c.rect.3 = c.rect.3.max(c.rect.1 + ck);
                }
                3 => c.rect.3 = c.rect.3.max(c.rect.1 + op),
                _ => {}
            }
        }
        // The edit has the focus (0x418580).
        dlg.active = dlg.controls.iter().position(|c| c.ty == 8 && c.flags & flag::VISIBLE != 0);
        Some(Menu { kind, dlg, depth, saved: *set, pressed: None, dragging: None, click: false, was_down: false, clock: None, last_press: None, timers, tick_count: 0, pump_ms: 0, ctx, vision: 0 })
    }

    /// 0x490e90 (the replay vision dialog's init) with 0x490d70 (event 10) in a replay, and
    /// 0x4913d0's 0x63ff74 = `mask` (0x6d0f18). `order` is 0x490000's player order
    /// (`State::vision_player_order`); `names` / `colors` are per player (0x57eeeb, 0x581dd6);
    /// `glu` is rez\gluAll.tbl. 0x490300 pops the order from its end: row k (label -10-k, ally
    /// box 1+k, vision box 13+k) shows that player: the name with its color swatch (0x490250,
    /// 0x4901f0), the ally box hidden (0x418700), the vision box shown, graphic 0xb, checked by
    /// the mask bit (0x490850 event 0). Not a replay (alliance rows) and team games
    /// (0x490570) are not ported.
    pub fn vision_init(&mut self, order: &[u32], names: &[Vec<u8>; 12], colors: &[u8; 12], mask: u16, replay: bool, glu: &[u8]) {
        self.vision = mask;
        if !replay {
            crate::todo_port!("0x490300", "the alliance rows outside a replay");
            return;
        }
        // 0x490d70: label 0x1a takes gluAll 0xb6, label 0x1b is hidden; 0x490c40 event 10: box
        // 0x19 takes gluAll 0xb7.
        if let Some(c) = find(&mut self.dlg, 0x1a) {
            c.text = crate::setup::tbl_str(glu, 0xb6 - 1).to_vec();
        }
        show(&mut self.dlg, 0x1b, false);
        if let Some(c) = find(&mut self.dlg, 0x19) {
            c.text = crate::setup::tbl_str(glu, 0xb7 - 1).to_vec();
        }
        for (k, &p) in order.iter().rev().enumerate() {
            let p = p as usize & 15;
            if let Some(c) = find(&mut self.dlg, -10 - k as i16) {
                c.text = names[p].clone();
                c.value.0 = p as i16;
                c.update = crate::dlg::Update::Player(colors[p]);
                c.flags |= flag::VISIBLE;
            }
            show(&mut self.dlg, 1 + k as i16, false);
            if let Some(c) = find(&mut self.dlg, 13 + k as i16) {
                c.flags |= flag::VISIBLE;
                c.graphic = 0xb;
                c.value.0 = p as i16;
                c.checked = mask >> p & 1 != 0;
            }
        }
    }

    /// 0x47e7b0 (the tips open): a campaign mission's tips at the game start (0x47ea30 set
    /// 0x658ae0; not multiplayer) start at its entry of 0x513ba0, shown as it is; otherwise
    /// the tip kept (1 when 0 or past the last) steps to the next.
    pub fn open_tip(&mut self, skin: &Skin, tips: &[String], set: &mut Settings, mission: Option<u16>, expansion: bool) {
        // 0x513ba0: the first tip of each mission (0x57f244); 0xffff: none.
        const MISSION_TIP: [u16; 0x40] = [
            0xffff, 1, 2, 4, 8, 11, 14, 17, 20, 23, 26, 29, 32, 35, 38, 41, 44, 47, 50, 53, 56, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78,
            81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 0xffff, 96, 97, 0xffff, 98, 99, 99, 99, 99, 99, 99, 100, 101, 102, 103, 0xffff, 0xffff,
        ];
        let mut d = 1;
        if let Some(m) = mission.filter(|&m| m != 0) {
            set.tip = MISSION_TIP.get(m as usize).copied().unwrap_or(0xffff) as i16;
            d = 0;
        }
        if set.tip == 0 || set.tip as u16 >= if expansion { 0x68 } else { 0x51 } {
            set.tip = 1;
        }
        self.show_tip(skin, tips, set, d, expansion);
    }

    /// 0x47e480: step the tip by `d` (1..=0x67 in the expansion, 0x50 without, wrapping)
    /// and show it in label -10, in font16 when it fits in its lines but one, else font10.
    pub fn show_tip(&mut self, skin: &Skin, tips: &[String], set: &mut Settings, d: i16, expansion: bool) {
        let last = if expansion { 0x67 } else { 0x50 };
        set.tip += d;
        if set.tip > last {
            set.tip = 1;
        } else if set.tip == 0 {
            set.tip = last;
        }
        let text = tips.get(set.tip as usize - 1).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
        let font = &skin.fonts[2];
        let Some(c) = find(&mut self.dlg, -10) else { return };
        let (w, h) = (c.rect.2 - c.rect.0, c.rect.3 - c.rect.1);
        c.flags = if font.width(&text) < (h / font.height.max(1) - 1) * w { c.flags & !0x4400 | 0x800 } else { c.flags & !0x4c00 };
        c.text = text.into_bytes();
        c.update = crate::dlg::Update::Tip;
    }

    /// The tips dialog's buttons (0x47e630): next (1), previous (3); the box (2) is
    /// "show tips at startup" (0x6cdfec 0x100).
    pub fn tips_button(&mut self, skin: &Skin, tips: &[String], id: i16, set: &mut Settings, expansion: bool) -> bool {
        match id {
            1 => self.show_tip(skin, tips, set, 1, expansion),
            3 => self.show_tip(skin, tips, set, -1, expansion),
            _ => return false,
        }
        true
    }

    /// Replaces control `id`'s text (a label's +0x14).
    pub fn set_text(&mut self, id: i16, text: &str) {
        if let Some(c) = find(&mut self.dlg, id) {
            c.text = text.as_bytes().to_vec();
        }
    }

    /// A tick slider (0x481d80 / 0x460d00 / 0x480e10 user 10: flag 0x2000000, then
    /// 0x414c00: thumb 100, step 1, height of frame 99) set to `v` in lo..=hi.
    fn slider(dlg: &mut Dialog, id: i16, v: i16, lo: i16, hi: i16) {
        if let Some(c) = find(dlg, id) {
            c.flags |= flag::TICK_SLIDER;
            c.thumb = 100;
            c.step = 1;
            c.value = (v.clamp(lo, hi), lo, hi);
        }
    }

    pub fn draw(&self, skin: &Skin, s: &mut Surf) {
        let mut d = self.dlg.clone();
        for (i, c) in d.controls.iter_mut().enumerate() {
            if self.pressed == Some(i) {
                c.flags |= flag::PRESSED;
            }
        }
        skin.draw(&d, s);
    }

    /// The left button / pointer at (x, y). Returns an activated control id.
    pub fn mouse(&mut self, skin: &Skin, x: i32, y: i32, down: bool, set: &mut Settings) -> Option<i16> {
        let hit = self.dlg.hit(x, y).filter(|&i| self.dlg.controls[i].flags & flag::DISABLED == 0);
        // 0x418eb0: the control under the pointer becomes active; an edit keeps the focus.
        let edit = self.dlg.controls.iter().position(|c| c.ty == 8 && c.flags & flag::VISIBLE != 0);
        self.dlg.active = hit.filter(|&i| matches!(self.dlg.controls[i].ty, 1..=4 | 6)).or(edit);
        // 0x418900 -> 0x418790: the control under the pointer has 0x80 (its text in color 4,
        // 0x4ef870), the one before loses it.
        let under = self.dlg.hit(x, y);
        for (i, c) in self.dlg.controls.iter_mut().enumerate() {
            if under == Some(i) { c.flags |= 0x80 } else { c.flags &= !0x80 }
        }
        if let Some(id) = self.list_edit_mouse(skin, x, y, down, hit) {
            return id;
        }
        if let Some(i) = self.dragging {
            if down {
                self.drag(skin, i, x, set);
                return None;
            }
            self.dragging = None;
            return None;
        }
        match (down, self.pressed) {
            (true, None) => {
                let i = hit?;
                if matches!(self.dlg.controls[i].ty, 8 | 12) {
                    return None;
                }
                if self.dlg.controls[i].ty == 6 {
                    self.dragging = Some(i);
                    self.drag(skin, i, x, set);
                } else {
                    self.pressed = Some(i);
                    self.dlg.controls[i].flags |= flag::PRESSED;
                }
                None
            }
            (false, Some(p)) => {
                self.pressed = None;
                // 0x4e19f0: a button (types 1 / 2) released on itself acts, then the sound
                // hook 0x6d5eac(2) (in game 0x4ee0f0 -> 0x4bbcf0: the button click).
                let was_pressed = self.dlg.controls[p].press_release();
                if was_pressed && hit == Some(p) && matches!(self.dlg.controls[p].ty, 1 | 2) {
                    self.click = true;
                }
                (hit == Some(p)).then(|| self.activate(p, set)).flatten()
            }
            _ => None,
        }
    }

    /// List boxes and edits (types 12 and 8) on the mouse edges. Some(r) when the event is
    /// theirs: the left button went down on a list (0x41ab20: the row under it is
    /// highlighted, or on a double click the default button 0x80000 is pressed, 0x418270;
    /// its scroll bar 0x415af0 takes the press first) or on an edit (0x4164d0: the caret
    /// goes under the pointer); while it is down the highlight follows (0x41ae90); when it
    /// goes up the highlighted row is selected (0x41a490: event 0xb when it differs).
    fn list_edit_mouse(&mut self, skin: &Skin, x: i32, y: i32, down: bool, hit: Option<usize>) -> Option<Option<i16>> {
        let (edge, up) = (down && !self.was_down, !down && self.was_down);
        self.was_down = down;
        let (ox, oy) = self.dlg.origin();
        let (rx, ry) = (x - ox, y - oy);
        if edge {
            let i = hit?;
            let (id, ty) = (self.dlg.controls[i].id, self.dlg.controls[i].ty);
            match ty {
                12 => {
                    let now = self.clock;
                    let dbl = now.is_some_and(|n| self.last_press.is_some_and(|(t, last)| last == id && n.wrapping_sub(t) < 500));
                    self.last_press = if dbl { None } else { now.map(|n| (n, id)) };
                    if dbl {
                        // 0x418270 with flag 0x80000.
                        let b = self.dlg.controls.iter().position(|c| c.flags & 0x80000 != 0 && c.flags & flag::VISIBLE != 0 && c.flags & flag::DISABLED == 0);
                        return Some(b.and_then(|b| self.activate_button(b)));
                    }
                    let c = &mut self.dlg.controls[i];
                    if c.list_scroll_click(rx, ry) {
                        self.click |= c.list_scroll_take_click();
                        return Some(None);
                    }
                    if c.list_inside(rx, ry) {
                        let r = c.list_row_at(rx, ry);
                        if let Some(l) = c.list.as_mut() {
                            l.hover = r;
                            l.pressed = true;
                        }
                        c.list_scroll_to(r);
                    }
                    return Some(None);
                }
                8 => {
                    let c = &mut self.dlg.controls[i];
                    let font = skin.font_of(c).clone();
                    c.edit_click(rx, &font);
                    return Some(None);
                }
                _ => {}
            }
        } else if down {
            for c in self.dlg.controls.iter_mut() {
                if c.list_scroll_active() {
                    c.list_scroll_drag(rx, ry);
                    self.click |= c.list_scroll_take_click();
                }
                if c.list.as_ref().is_some_and(|l| l.pressed) && c.list_inside(rx, ry) {
                    let r = c.list_row_at(rx, ry);
                    let l = c.list.as_mut().unwrap();
                    if l.hover != r {
                        l.hover = r;
                        c.list_scroll_to(r);
                    }
                }
            }
        } else if up {
            let mut ev = None;
            for c in self.dlg.controls.iter_mut() {
                if c.list_scroll_active() {
                    c.list_scroll_release(rx, ry);
                }
                let Some(l) = c.list.as_mut().filter(|l| l.pressed) else { continue };
                l.pressed = false;
                if l.hover != l.sel && (l.hover as usize) < l.items.len() {
                    ev = Some(l.hover);
                }
            }
            if let Some(r) = ev {
                list_event(skin, &mut self.dlg, r);
            }
        }
        None
    }

    /// The press of button `i` (types 1 / 2): its id.
    fn activate_button(&mut self, i: usize) -> Option<i16> {
        matches!(self.dlg.controls[i].ty, 1 | 2).then(|| self.dlg.controls[i].id)
    }

    /// Time passes (the caller's frame time in ms): the edit's caret blinks, the message
    /// pump runs (0x4d1bf0: every 100 ms event 0xd, which repeats a held list scroll bar
    /// arrow; the dialog timers 0x415dc0), and double clicks can be told. Some(id): the
    /// activation a fired timer stands for (the defeat dialog's 20 s: event 8 id 0 sets the
    /// result 7 and closes it, as the OK button -2 does).
    #[allow(dead_code)]
    pub fn tick(&mut self, ms: u32) -> Option<i16> {
        self.clock = Some(self.clock.unwrap_or(0).wrapping_add(ms));
        self.tick_count = self.tick_count.wrapping_add(ms);
        self.dlg.tick(ms);
        self.pump_ms += ms;
        if self.pump_ms >= 100 {
            self.pump_ms = 0;
            for c in self.dlg.controls.iter_mut().filter(|c| c.list_scroll_active()) {
                c.list_scroll_tick();
                // 0x4bbcf0: a held arrow's re-press clicks.
                self.click |= c.list_scroll_take_click();
            }
        }
        let mut act = None;
        let mut refresh = false;
        let kind = self.kind;
        self.timers.run(self.tick_count, |_, t| {
            // 0x461430 event 8 (0x461470): only id 0.
            if kind == Kind::Defeat && t.callback.is_none() && t.id == 0 {
                act = Some(-2);
            }
            // 0x4c9890 / 0x4c9150: the 200 ms timers of the game and end-mission menus.
            if matches!(kind, Kind::Game | Kind::EndMission) && t.id == 1 {
                refresh = true;
            }
        });
        if refresh {
            match kind {
                Kind::Game => game_menu_refresh(&mut self.dlg, &self.ctx),
                _ => end_mission_label(&mut self.dlg, &self.ctx, self.ctx.elapsed_secs + self.tick_count / 1000),
            }
        }
        act
    }

    /// A key went down (WM_KEYDOWN, event 0; `vk` the virtual key, `mods` the shift bits
    /// +0xa): 0x418a10 gives it to the list (0x41a9d0: the selection moves, Home / End /
    /// Page / arrows) then the edit (0x416980: caret, Insert, Delete).
    #[allow(dead_code)]
    pub fn key_down(&mut self, skin: &Skin, vk: u16, mods: u8) {
        for i in 0..self.dlg.controls.len() {
            let c = &mut self.dlg.controls[i];
            if !matches!(c.ty, 8 | 12) || c.flags & (flag::VISIBLE | flag::DISABLED) != flag::VISIBLE {
                continue;
            }
            if c.ty == 12 {
                let Some(r) = c.list_key(vk) else { continue };
                if let Some(r) = r {
                    list_event(skin, &mut self.dlg, r);
                    if let Some(c) = find(&mut self.dlg, 1) {
                        let sel = c.list.as_ref().map_or(0xff, |l| l.sel);
                        c.list_scroll_to(sel);
                    }
                }
                return;
            }
            if c.edit_key(vk, mods) {
                return;
            }
        }
    }

    /// A typed character (WM_CHAR, event 0xf) for the edit (0x461d40 -> 0x461c20): the
    /// characters of network.tbl string 0x10 do nothing; the name holds 0x1b characters
    /// and does not start with a space; else the edit takes it (0x416980). Then OK works
    /// with a text (0x418e00 / 0x418640) and the list loses its selection (event 0xb, row
    /// 0xff), which disables delete.
    #[allow(dead_code)]
    pub fn type_char(&mut self, skin: &Skin, ch: u8) {
        let bad = crate::setup::tbl_str(&sg().net, 0x10).to_vec();
        let Some(c) = find(&mut self.dlg, 2).filter(|c| c.edit.is_some()) else { return };
        if ch == 0 || bad.contains(&ch) {
            return;
        }
        if ch == 8 || (c.text.len() <= 0x1a && !(ch == 0x20 && c.text.is_empty())) {
            let font = skin.font_of(c).clone();
            if c.edit_char(ch, &font) {
                c.edit_rescroll(&font);
            }
        }
        if c.text.is_empty() {
            disable(&mut self.dlg, -2);
        } else {
            enable(&mut self.dlg, -2);
        }
        let selected = self.dlg.get(1).and_then(|c| c.list.as_ref()).is_some_and(|l| !l.items.is_empty() && l.sel != 0xff);
        if selected {
            list_event(skin, &mut self.dlg, 0xff);
        }
    }

    /// A slider follows the pointer (0x415910), snapping to whole values.
    fn drag(&mut self, skin: &Skin, i: usize, x: i32, set: &mut Settings) {
        let r = self.dlg.abs(&self.dlg.controls[i]);
        let range = skin.slider_range(r).max(1);
        let c = &mut self.dlg.controls[i];
        let (lo, hi) = (c.value.1 as i32, c.value.2 as i32);
        let half = skin_thumb_half(skin);
        let v = (lo + ((x - r.0 - half) * (hi - lo) + range / 2).div_euclid(range)).clamp(lo, hi);
        c.value.0 = v as i16;
        self.store(set);
    }

    /// The control's activation: buttons report their id; boxes toggle (0x4e1b50 user 2,
    /// option boxes 0x4e1900).
    fn activate(&mut self, i: usize, set: &mut Settings) -> Option<i16> {
        let c = &self.dlg.controls[i];
        let (id, ty) = (c.id, c.ty);
        match ty {
            4 => {
                let c = &mut self.dlg.controls[i];
                c.checked = !c.checked;
                // 0x490850 user 2 (the vision boxes, ids 13..24): 0x63ff74 ^= 1 << player (+0x26).
                if self.kind == Kind::ReplayVision && (13..=24).contains(&id) {
                    self.vision ^= 1 << (c.value.0 & 15);
                }
                self.store(set);
                None
            }
            3 => {
                option_click(&mut self.dlg, i);
                self.store(set);
                None
            }
            1 | 2 => Some(id),
            _ => None,
        }
    }

    /// 0x460a90: the dialog's controls into the live settings (the option dialogs apply as they
    /// change: 0x481b00 / 0x460a80 / 0x480b20 and the box handlers).
    fn store(&self, set: &mut Settings) {
        let d = &self.dlg;
        let v = |id: i16| d.get(id).map(|c| c.value.0);
        let ck = |id: i16| d.get(id).map(|c| c.checked);
        match self.kind {
            Kind::Speed => {
                // The game speed slider does not change 0x6cdfd4 itself: OK sends command
                // 0x0f (0x481ef0, `Action::GameSpeed`).
                set.mouse_scroll = v(2).unwrap_or(set.mouse_scroll);
                set.key_scroll = v(3).unwrap_or(set.key_scroll);
                set.cpu_throttle = ck(4).unwrap_or(set.cpu_throttle);
            }
            Kind::Sound => {
                set.music = v(1).unwrap_or(set.music);
                set.digital = v(2).unwrap_or(set.digital);
                for (id, bit) in [(3, 1), (4, 2), (5, 4), (6, 0x400)] {
                    match ck(id) {
                        Some(true) => set.sound_flags |= bit,
                        Some(false) => set.sound_flags &= !bit,
                        None => {}
                    }
                }
            }
            Kind::Tips => match ck(2) {
                Some(true) => set.sound_flags |= 0x100,
                Some(false) => set.sound_flags &= !0x100,
                None => {}
            },
            Kind::Video => {
                set.gamma = v(1).map_or(set.gamma, |g| crate::palette::set_gamma(g as i32));
                set.color_cycling = ck(2).unwrap_or(set.color_cycling);
                if let Some(p) = (3..=5).find(|&id| ck(id) == Some(true)) {
                    set.portraits = p - 3;
                }
            }
            _ => {}
        }
    }

    /// A key: Esc is the hotkey of the -3 buttons (text byte 0x1b), Enter the default
    /// (type 1) button, else the hotkey byte of a visible, enabled control.
    pub fn key(&mut self, ch: u8, set: &mut Settings) -> Option<i16> {
        // 0x418ab0: hotkeys other than Esc need a dialog without an edit (letters are `type_char`).
        let has_edit = self.dlg.controls.iter().any(|c| c.ty == 8);
        if has_edit && !matches!(ch, b'\r' | 0x1b) {
            return None;
        }
        let i = self.dlg.controls.iter().position(|c| {
            c.flags & flag::VISIBLE != 0
                && c.flags & flag::DISABLED == 0
                && matches!(c.ty, 1..=4)
                && if ch == b'\r' { c.ty == 1 } else { c.flags & flag::HOTKEY != 0 && c.text.first().map(|h| h.to_ascii_lowercase()) == Some(ch.to_ascii_lowercase()) }
        })?;
        self.activate(i, set)
    }

    /// 0x68516a = 0, 0x68516c = 0, 0x685148 = "" (before a save / load dialog opens).
    fn clear_choice() {
        let mut g = sg();
        g.sel = 0;
        g.id = 0;
        g.name.clear();
    }

    /// 0x4f6050 (the message ESI, the callback `cb`, rez\ok.bin or rez\okcancel.bin):
    /// the message dialog.
    fn popup(kind: Kind, msg: Vec<u8>, cb: Cb) -> (Option<(Kind, u8)>, Action) {
        let mut g = sg();
        // refused while a message is up
        if g.cb.is_some() || !g.msg.is_empty() {
            return (None, Action::None);
        }
        g.msg = msg;
        g.cb = Some(cb);
        (Some((kind, 0)), Action::None)
    }

    /// 0x462510 (save) / 0x461a30 (load) with the dialog's destroy event 0x461ae0.
    /// 0x462310: delete the selected save (inline here): copies its id (0x68516c) and name
    /// (0x685148) from the list entry (0x4617c0), asks with string 3 of 0x6d1220 (okcancel.bin).
    fn save_activate(&self, id: i16) -> (Option<(Kind, u8)>, Action) {
        let save = self.kind == Kind::SaveGame;
        let list = self.dlg.get(1).and_then(|c| c.list.as_ref());
        let rows = list.map_or(0, |l| l.items.len());
        // the selected row's text (0x48 of the list when it has rows, else 0xff)
        let sel_text = list.filter(|_| rows != 0).and_then(|l| l.items.get(l.sel as usize)).map(|i| i.0.clone());
        let edit = self.dlg.get(2).filter(|c| c.ty == 8).map(|c| trim_end(c.text.clone()));
        let mut g = sg();
        let r = match id {
            // 0x462310: delete the selected save (after asking)
            3 if save => match sel_text.as_deref().and_then(|t| find_save(&g.saves, t).map(|n| (t.to_vec(), n.clone()))) {
                Some((t, (name, sid))) => {
                    g.id = sid;
                    g.name = name;
                    let msg = sprintf_s(crate::setup::tbl_str(&g.net, 3), &t);
                    drop(g);
                    let r = Self::popup(Kind::OkCancel, msg, Cb::Delete);
                    g = sg();
                    r
                }
                None => (None, Action::None),
            },
            // 0x4623c0 (0x462510 first sets 0x6d1224 = 0)
            -2 if save => {
                g.name = edit.clone().unwrap_or_default();
                let name = g.name.clone();
                if let Some(n) = find_save(&g.saves, &name) {
                    g.id = n.1;
                    let msg = sprintf_s(crate::setup::tbl_str(&g.net, 2), &name);
                    drop(g);
                    let r = Self::popup(Kind::OkCancel, msg, Cb::Overwrite);
                    g = sg();
                    r
                } else if g.saves.len() >= 0x80 {
                    let msg = crate::setup::tbl_str(&g.net, 4).to_vec();
                    drop(g);
                    let r = Self::popup(Kind::Ok, msg, Cb::Reopen);
                    g = sg();
                    r
                } else if crate::setup::Setup::profile_name_invalid(&name, &g.net) {
                    // 0x4f3f20
                    let msg = crate::setup::tbl_str(&g.net, 5).to_vec();
                    drop(g);
                    let r = Self::popup(Kind::Ok, msg, Cb::Reopen);
                    g = sg();
                    r
                } else {
                    // 0x4c0600, 0x685164 = GetTickCount
                    g.saved_at = Some(web_time::Instant::now());
                    (None, Action::SaveGame(String::from_utf8_lossy(&name).into_owned()))
                }
            }
            // 0x4619a0: the load command (7, the id, the name) and the dialog closes
            -2 => match sel_text.as_deref().and_then(|t| find_save(&g.saves, t).map(|n| (t.to_vec(), n.1))) {
                Some((t, sid)) => (None, Action::LoadGame(String::from_utf8_lossy(&t[..t.len().min(0x1b)]).into_owned(), sid)),
                None => (None, Action::None),
            },
            // 0x462510 / 0x461a30: Previous goes back to the game menu (0x4ca450)
            -3 if self.depth > 1 => (Some((Kind::Game, self.depth.saturating_sub(2))), Action::None),
            -3 => (None, Action::Closed),
            _ => (None, Action::None),
        };
        // 0x461ae0 (event 1 as the dialog closes): the row and the typed name are kept for
        // the next time, the list is freed.
        g.sel = if rows == 0 { 0xff } else { list.map_or(0xff, |l| l.sel) };
        if let Some(e) = edit {
            g.name = e;
        }
        g.saves.clear();
        r
    }

    /// 0x4f5990: the message dialog closes and its callback runs with the button id.
    fn popup_activate(id: i16) -> (Option<(Kind, u8)>, Action) {
        let (cb, name, net) = {
            let mut g = sg();
            g.msg.clear();
            (g.cb.take(), g.name.clone(), g.net.clone())
        };
        // 0x4620e0 and the tails of 0x462150 / 0x4621f0: the save dialog again.
        let reopen = (Some((Kind::SaveGame, 0)), Action::None);
        match cb {
            Some(Cb::Reopen) => reopen,
            // 0x462150
            Some(Cb::Delete) => {
                if id == -2 {
                    let env = sg().env.clone();
                    if !delete_save(&env, &name) {
                        let msg = sprintf_s(crate::setup::tbl_str(&net, 0), &name);
                        return Self::popup(Kind::Ok, msg, Cb::Reopen);
                    }
                    let mut g = sg();
                    g.sel = 0;
                    g.id = 0;
                    g.name.clear();
                }
                reopen
            }
            // 0x4621f0
            Some(Cb::Overwrite) => {
                if id != -2 {
                    return reopen;
                }
                let env = sg().env.clone();
                if !delete_save(&env, &name) {
                    let msg = sprintf_s(crate::setup::tbl_str(&net, 1), &name);
                    return Self::popup(Kind::Ok, msg, Cb::Reopen);
                }
                // 0x4c0600, 0x685164 = GetTickCount
                sg().saved_at = Some(web_time::Instant::now());
                (None, Action::SaveGame(String::from_utf8_lossy(&name).into_owned()))
            }
            None => (None, Action::Closed),
        }
    }

    /// The opener's callback (0x6d1234) for activated control `id`: the next menu to
    /// open (or none) and what the game does.
    pub fn on_activate(&self, id: i16, set: &mut Settings, ctx: Ctx) -> (Option<(Kind, u8)>, Action) {
        let back = |k: Kind| (Some((k, self.depth.saturating_sub(2))), Action::None);
        match (self.kind, id) {
            (Kind::SaveGame | Kind::LoadGame, _) => return self.save_activate(id),
            (Kind::OkCancel | Kind::Ok, _) => return Self::popup_activate(id),
            // 0x4e4980 event 0xe code 2 (0x419ea0): the dialog closes.
            // 0x491310 event 0xe code 2 -> 0x490fe0 (OK, id -2): the shown players and the all-players
            // box (0x19) set the vision; Cancel (-3) just closes it (0x4f5700).
            (Kind::ReplayVision, -2) => (None, Action::ReplayVision(self.vision, self.dlg.get(0x19).is_some_and(|c| c.checked))),
            (Kind::ReplayVision, -3) => (None, Action::Closed),
            (Kind::Fatal, _) => {
                sg().fatal = None;
                return (None, Action::Closed);
            }
            // 0x4ca450
            (Kind::Game, -3) => (None, Action::Closed),
            (Kind::Game, 3) => (Some((Kind::Options, self.depth)), Action::None),
            (Kind::Game, 4) => (Some((Kind::Help, self.depth)), Action::None),
            // 0x4ca010
            (Kind::Help, 2) => (Some((Kind::Tips, self.depth)), Action::None),
            // 0x47e690 (OK) / 0x47e630 (next, previous): back to the help menu, or closed when
            // the tips are the only menu (depth 1: shown at the game start, 0x47ea30).
            (Kind::Tips, -2 | -3) if self.depth > 1 => back(Kind::Help),
            (Kind::Tips, -2 | -3) => (None, Action::Closed),
            // 0x462600
            (Kind::Game, 1) => {
                // Only the lowest human saves (0x4c3db0), and 0x596877 must be 0.
                if ctx.lowest_human() != ctx.local_storm || ctx.no_save != 0 {
                    return (None, Action::Closed);
                }
                // 0x461720: in multiplayer one save per 2 minutes
                if save_wait(&ctx) || ctx.replay {
                    return (None, Action::Closed);
                }
                Self::clear_choice();
                (Some((Kind::SaveGame, self.depth)), Action::None)
            }
            // 0x462100
            (Kind::Game, 2) => {
                if ctx.multiplayer || ctx.replay {
                    return (None, Action::Closed);
                }
                Self::clear_choice();
                // 0x461e50 (inline: not needed here; when 0x596904 == 4, i.e. a save was chosen): pump messages and
                // redraw (0x41ca00, 0x4d1bf0(3)) until the open-dialog count (0x6d5eb4)
                // drops below its entry value. This menu is event-driven: the pump runs in
                // the front end's loop, and the load action is only taken once the dialog
                // has closed, so there is nothing to wait for here.
                (Some((Kind::LoadGame, self.depth)), Action::None)
            }
            (Kind::Game, 5) => (Some((Kind::Objectives, self.depth)), Action::None),
            (Kind::Game, 6) => (Some((Kind::EndMission, self.depth)), Action::None),
            (Kind::Game, 7) => (None, Action::Pause),
            (Kind::Game, 8) => (None, Action::Resume),
            // 0x4ca0a0
            (Kind::Options, 1) => (Some((Kind::Speed, self.depth)), Action::None),
            (Kind::Options, 2) => (Some((Kind::Sound, self.depth)), Action::None),
            (Kind::Options, 3) => (Some((Kind::Video, self.depth)), Action::None),
            (Kind::Options | Kind::Help | Kind::Objectives | Kind::EndMission, -3) if self.depth > 1 => back(Kind::Game),
            // 0x481ef0 / 0x460dd0 / 0x480ee0 with 0x4c9780: Cancel restores the copy (a
            // replay keeps its speed) and returns to the options; OK sets the depth to 1
            // (0x6d1224) so the dialog just closes.
            // 0x481ef0: OK with the speed slider (1) enabled sends command 0x0f with its value
            // (0x485bd0), which sets 0x6cdfd4 when the turn runs (0x4bf7f0).
            (Kind::Speed, -2) => match self.dlg.get(1) {
                Some(c) if c.flags & flag::DISABLED == 0 => {
                    (None, Action::GameSpeed(c.value.0.clamp(0, 255) as u8))
                }
                _ => (None, Action::Apply),
            },
            (Kind::Sound | Kind::Video, -2) => (None, Action::Apply),
            (Kind::Speed | Kind::Sound | Kind::Video, -3) => {
                let speed = set.game_speed;
                *set = self.saved;
                if ctx.replay {
                    set.game_speed = speed;
                }
                if self.depth > 1 {
                    (Some((Kind::Options, self.depth.saturating_sub(2))), Action::Apply)
                } else {
                    (None, Action::Apply)
                }
            }
            // 0x4ca370
            (Kind::EndMission, 1) => (Some((Kind::Restart, self.depth)), Action::None),
            (Kind::EndMission, 2) => (Some((Kind::QuitMission, self.depth)), Action::None),
            (Kind::EndMission, 3) => (Some((Kind::Quit, self.depth)), Action::None),
            (Kind::Restart, -2) => (None, Action::Restart),
            // 0x4c9280: a multiplayer game sets the next screen 0x51ce90 to 6 (local result 0x58d700
            // == 3) or 7, the score screen.
            (Kind::QuitMission, -2) if ctx.multiplayer => (None, Action::QuitScore(if ctx.local_result == 3 { 6 } else { 7 })),
            (Kind::QuitMission, -2) => (None, Action::QuitMission),
            (Kind::Quit, -2) => (None, Action::ExitProgram),
            // 0x460f30 (defeat; every activation sets 0x68517c) / 0x460fa0 (victory).
            (Kind::Defeat | Kind::Victory, -2) => (None, Action::EndGame),
            (Kind::Defeat | Kind::Victory, 1) => {
                if self.kind == Kind::Victory {
                    return (None, Action::Message);
                }
                (None, Action::Closed)
            }
            (Kind::Restart | Kind::QuitMission | Kind::Quit, -3) => back(Kind::EndMission),
            (_, -3) => (None, Action::Closed),
            // ponytail: keystroke help, tips and the network options are not ported.
            _ => (None, Action::None),
        }
    }
}

fn skin_check(skin: &Skin) -> i32 {
    skin.grp.frame(15).map_or(16, |f| f.w.max(f.h))
}

fn skin_option(skin: &Skin) -> i32 {
    skin.grp.frame(10).map_or(19, |f| f.w.max(f.h))
}

fn skin_thumb_half(skin: &Skin) -> i32 {
    skin.grp.frame(0x63).map_or(5, |f| f.w / 2)
}

/// 0x4c9150 (the tail): control `dst`'s text is `text` and the control is shown (0x41c400).
fn copy_str(dlg: &mut Dialog, dst: i16, text: &[u8]) {
    if let Some(c) = find(dlg, dst) {
        c.text = text.to_vec();
        c.flags |= flag::VISIBLE;
    }
}

/// 0x4c9150 (the tail): control `dst` takes the text of control `src`.
fn copy_text(dlg: &mut Dialog, dst: i16, src: i16) {
    if let Some(t) = find(dlg, src).map(|c| c.text.clone()) {
        copy_str(dlg, dst, &t);
    }
}

/// 0x4c9150 (called by 0x4c9440 with the end-mission menu, then every 200 ms in a BNET game):
/// control 2 shows the text of label -13 in a replay; else -12 in a battle.net game in its
/// first 120 s (`secs` = 0x4a2ca0), else -11 when the local result (0x58d700) is 3, else -10.
fn end_mission_label(dlg: &mut Dialog, ctx: &Ctx, secs: u32) {
    let src = if ctx.replay {
        -13
    } else if !(ctx.bnet && secs <= 0x78) {
        if ctx.local_result == 3 { -11 } else { -10 }
    } else {
        -12
    };
    copy_text(dlg, 2, src);
}

/// 0x4c9a90 (multiplayer or replay game menu): the load button goes; the save button
/// shows (disabled during the 2 minute wait, 0x461720) for the lowest human of a savable
/// live game, else it goes; Pause (7) / Resume (8) show by the pause state.
fn game_menu_open(dlg: &mut Dialog, ctx: &Ctx) {
    show(dlg, 2, false);
    if ctx.lowest_human() == ctx.local_storm && ctx.no_save == 0 && !ctx.replay {
        if save_wait(ctx) {
            disable(dlg, 1);
        } else {
            enable(dlg, 1);
        }
    } else {
        show(dlg, 1, false);
    }
    show(dlg, 7, !ctx.replay && (!ctx.multiplayer || (ctx.g66fbfa > 8 && ctx.player_type == 2 && ctx.pauses_left != 0)) && !ctx.paused);
    show(dlg, 8, !ctx.replay && ctx.player_type == 2 && ctx.paused);
}

/// 0x4c9890 (the game menu's 200 ms timer, not in a replay): Pause / Resume by the pause
/// state, the save button disabled during a multiplayer game's 2 minute wait.
fn game_menu_refresh(dlg: &mut Dialog, ctx: &Ctx) {
    show(dlg, 7, (!ctx.multiplayer || (ctx.g66fbfa > 8 && ctx.player_type == 2 && ctx.pauses_left != 0)) && !ctx.paused);
    show(dlg, 8, ctx.paused);
    if save_wait(ctx) {
        disable(dlg, 1);
    } else {
        enable(dlg, 1);
    }
}
