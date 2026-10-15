//! The in-game menus (opt_dlgs.cpp / modeless.cpp): the game menu (F10 / MENU) and the
//! dialogs it opens. Each is a `.bin` dialog opened by 0x4f57a0 with the shared event
//! proc 0x4ca240 or its own (speed 0x481fa0, sound 0x460e20, video 0x480f90); the
//! activated control id (0x6d051c) goes to the opener's callback (0x6d1234).

use crate::dlg::{flag, Ctl, Dialog, Skin, Surf};
use crate::storage::Storage;

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
}

impl Default for Settings {
    fn default() -> Self {
        // ponytail: the registry defaults for speed / scroll / sound flags are not
        // traced; fastest and middle scroll speeds, all sounds on.
        Settings { game_speed: 6, mouse_scroll: 3, key_scroll: 3, music: 25, digital: 50, sound_flags: 7, cpu_throttle: false, gamma: 100, color_cycling: true, portraits: 2 }
    }
}

/// What opened the game is running as (0x6d0f14 replay, 0x57f0b4 multiplayer).
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub replay: bool,
    pub multiplayer: bool,
    /// 0x6d0f6a: a replay's speed, shown on the (disabled) game speed slider.
    pub replay_speed: i16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Game,
    Options,
    Speed,
    Sound,
    Video,
    Help,
    Objectives,
    EndMission,
    Restart,
    QuitMission,
    Quit,
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
            Kind::Objectives => "rez/objctdlg.bin",
            Kind::EndMission => "rez/abrtmenu.bin",
            Kind::Restart => "rez/restart.bin",
            // 0x4ca2f0
            Kind::QuitMission if replay => "rez/quitrepl.bin",
            Kind::QuitMission => "rez/quit2mnu.bin",
            Kind::Quit => "rez/quit.bin",
        }
    }
}

/// What the game does after a menu action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    None,
    /// The menu closed (0x419ea0).
    Closed,
    /// Game menu 7 / 8 (0x4c0450 / 0x4c0420).
    Pause,
    Resume,
    /// The settings changed (OK in an option dialog).
    Apply,
    Restart,
    /// Quit the mission / replay.
    QuitMission,
    ExitProgram,
}

/// The open menu (0x6d5bf4) and the depth counter 0x6d1224.
pub struct Menu {
    pub kind: Kind,
    pub dlg: Dialog,
    /// 0x6d1224: how many menus deep (Previous / OK / Cancel go back while > 1).
    depth: u8,
    /// The settings when the option dialog opened (its +0x26 copy, restored by Cancel).
    saved: Settings,
    /// The control the left button went down on (pressed until released).
    pressed: Option<usize>,
    /// A slider being dragged.
    dragging: Option<usize>,
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

impl Menu {
    /// 0x4f57a0 + the dialog's init (event 0xe, user 0).
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
        // (0x481e80, 0x460d60, 0x480e70) use styles 3 and 5.
        let (ok, cancel) = if matches!(kind, Kind::Speed | Kind::Sound | Kind::Video) { (3, 5) } else { (4, 4) };
        for (id, style) in [(-2, ok), (-3, cancel)] {
            if let Some(c) = find(&mut dlg, id) {
                c.graphic = GRAPHICS[style];
                c.flags |= flag::WIDE;
            }
        }
        let depth = depth + 1;
        match kind {
            Kind::Game => {
                // 0x4c9a90 (multiplayer or replay): no save / load / pause in a replay.
                if ctx.multiplayer || ctx.replay {
                    show(&mut dlg, 2, false);
                    show(&mut dlg, 1, false);
                    show(&mut dlg, 7, !ctx.replay && !ctx.multiplayer);
                    show(&mut dlg, 8, false);
                }
            }
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
            _ => {}
        }
        // Check / option boxes: 0x4e1b50 / 0x4e1bc0 (user 10): graphic 0xb, at least
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
        Some(Menu { kind, dlg, depth, saved: *set, pressed: None, dragging: None })
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
        // 0x418eb0: the control under the pointer becomes active.
        self.dlg.active = hit.filter(|&i| matches!(self.dlg.controls[i].ty, 1..=4 | 6));
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
                if self.dlg.controls[i].ty == 6 {
                    self.dragging = Some(i);
                    self.drag(skin, i, x, set);
                } else {
                    self.pressed = Some(i);
                }
                None
            }
            (false, Some(p)) => {
                self.pressed = None;
                (hit == Some(p)).then(|| self.activate(p, set)).flatten()
            }
            _ => None,
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
                self.store(set);
                None
            }
            3 => {
                for c in self.dlg.controls.iter_mut().filter(|c| c.ty == 3) {
                    c.checked = c.id == id;
                }
                self.store(set);
                None
            }
            1 | 2 => Some(id),
            _ => None,
        }
    }

    /// The dialog's controls into the live settings (the option dialogs apply as they
    /// change: 0x481b00 / 0x460a80 / 0x480b20 and the box handlers).
    fn store(&self, set: &mut Settings) {
        let d = &self.dlg;
        let v = |id: i16| d.get(id).map(|c| c.value.0);
        let ck = |id: i16| d.get(id).map(|c| c.checked);
        match self.kind {
            Kind::Speed => {
                if d.get(1).is_some_and(|c| c.flags & flag::DISABLED == 0) {
                    set.game_speed = v(1).unwrap_or(set.game_speed);
                }
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
            Kind::Video => {
                set.gamma = v(1).unwrap_or(set.gamma);
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
        let i = self.dlg.controls.iter().position(|c| {
            c.flags & flag::VISIBLE != 0
                && c.flags & flag::DISABLED == 0
                && matches!(c.ty, 1..=4)
                && if ch == b'\r' { c.ty == 1 } else { c.flags & flag::HOTKEY != 0 && c.text.first().map(|h| h.to_ascii_lowercase()) == Some(ch.to_ascii_lowercase()) }
        })?;
        self.activate(i, set)
    }

    /// The opener's callback (0x6d1234) for activated control `id`: the next menu to
    /// open (or none) and what the game does.
    pub fn on_activate(&self, id: i16, set: &mut Settings, ctx: Ctx) -> (Option<(Kind, u8)>, Action) {
        let back = |k: Kind| (Some((k, self.depth.saturating_sub(2))), Action::None);
        match (self.kind, id) {
            // 0x4ca450
            (Kind::Game, -3) => (None, Action::Closed),
            (Kind::Game, 3) => (Some((Kind::Options, self.depth)), Action::None),
            (Kind::Game, 4) => (Some((Kind::Help, self.depth)), Action::None),
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
            // replay keeps its speed); both return to the options.
            (Kind::Speed | Kind::Sound | Kind::Video, -2 | -3) => {
                if id == -3 {
                    let speed = set.game_speed;
                    *set = self.saved;
                    if ctx.replay {
                        set.game_speed = speed;
                    }
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
            (Kind::QuitMission, -2) => (None, Action::QuitMission),
            (Kind::Quit, -2) => (None, Action::ExitProgram),
            (Kind::Restart | Kind::QuitMission | Kind::Quit, -3) => back(Kind::EndMission),
            (_, -3) => (None, Action::Closed),
            // ponytail: save / load (0x462600 / 0x462100), keystroke help, tips and the
            // network options are not ported.
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
