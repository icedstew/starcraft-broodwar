//! Playable map viewer on the faithful engine (`bw::State`): terrain, fog of war,
//! sprites, selection, the console HUD and player commands.

use crate::bw::{self, list::NIL};
use crate::storage::Storage;
use crate::todo_port;
use crate::dat::{self, Dat};
use crate::grp::{Blend, Grp};
use crate::map::{self, Chk, Tileset};
use std::cell::RefCell;
use std::io;

pub fn pcx(d: &[u8]) -> (usize, usize, Vec<u8>) {
    let r16 = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]) as usize;
    let (w, h, bpl) = (r16(8) - r16(4) + 1, r16(10) - r16(6) + 1, r16(66));
    let mut out = Vec::with_capacity(bpl * h);
    let mut p = 128;
    while out.len() < bpl * h && p < d.len() {
        let b = d[p];
        p += 1;
        if b >= 0xC0 {
            let v = d.get(p).copied().unwrap_or(0);
            p += 1;
            out.extend(std::iter::repeat_n(v, (b & 0x3F) as usize));
        } else {
            out.push(b);
        }
    }
    out.resize(bpl * h, 0);
    let px = (0..h).flat_map(|y| out[y * bpl..y * bpl + w].to_vec()).collect();
    (w, h, px)
}

pub struct Assets {
    pub files: Storage,
    /// Engine data (dat tables, iscript, GRP sizes, LO files).
    pub bw: bw::load::Loaded,
    /// The scenario archive (.scm/.scx) of the map being played, if it came from one.
    pub map_mpq: Option<crate::mpq::Mpq>,
    image_names: Vec<String>,
    grps: RefCell<Vec<Option<Option<std::rc::Rc<Grp>>>>>,
}

impl Assets {
    pub fn load(files: Storage) -> io::Result<Assets> {
        let bw = bw::load::Loaded::load(&files)?;
        let image_names = dat::parse_tbl(&files.read("arr/images.tbl")?);
        Ok(Assets { files, bw, map_mpq: None, image_names, grps: RefCell::new(vec![None; dat::Images::COUNT]) })
    }

    pub fn dat(&self) -> &Dat {
        &self.bw.dat
    }

    pub fn grp(&self, image: u16) -> Option<std::rc::Rc<Grp>> {
        let mut g = self.grps.borrow_mut();
        let slot = g.get_mut(image as usize)?;
        slot.get_or_insert_with(|| {
            let idx = self.bw.dat.images.grp[image as usize] as usize;
            let name = self.image_names.get(idx.checked_sub(1)?)?;
            self.files.read(&format!("unit/{name}")).ok().map(|d| std::rc::Rc::new(Grp::parse(d)))
        })
        .clone()
    }
}

/// The 1.16.1 minimap for a map, with the tileset's graphics from the install.
pub fn load_minimap(files: &Storage, chk: &Chk) -> std::io::Result<bw::minimap::Minimap> {
    let name = map::TILESETS[chk.tileset()];
    let rd = |p: String| files.read(&p);
    let wpe = rd(format!("tileset/{name}.wpe"))?;
    let mut pal = [[0u8; 3]; 256];
    for (i, p) in pal.iter_mut().enumerate() {
        p.copy_from_slice(&wpe[i * 4..i * 4 + 3]);
    }
    let dark = pcx(&rd(format!("tileset/{name}/dark.pcx"))?).2;
    let vx4 = rd(format!("tileset/{name}.vx4"))?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let vr4 = rd(format!("tileset/{name}.vr4"))?;
    let tm = pcx(&rd("game/tminimap.pcx".into())?).2;
    let mut colors = [0u8; 12];
    colors.copy_from_slice(&tm[..12]);
    let (w, h) = chk.dims();
    let mut mm = bw::minimap::Minimap::new(w as u16, h as u16, chk.tileset() as u16, &pal, &dark, vx4, vr4, colors);
    mm.set_blink(crate::grp::Grp::parse(rd("game/blink.grp".into())?), pcx(&rd("game/tblink.pcx".into())?).2);
    Ok(mm)
}

/// Run `f` with the engine's `Data` view over `a`.
fn with_data<R>(a: &Assets, f: impl FnOnce(&bw::Data) -> R) -> R {
    let g = |i: u16| a.bw.grp_size(i);
    let d = bw::load::data(&a.bw, &g);
    f(&d)
}

pub struct Scene<'a> {
    pub st: bw::State,
    /// 0x6d0c64: the tileset's creep edge pieces (`Tileset\<name>.grp`, 0x414100).
    creep_grp: Option<crate::grp::Grp>,
    /// Unit voices (selection and acknowledgement lines).
    pub voices: crate::voices::Voices,
    /// The console portrait's video (UI state).
    pub portrait: std::cell::RefCell<crate::portrait::Player>,
    /// 0x68ac4c-ish: ms left of a unit's talk (0x45efe0) holding the console portrait.
    talk_left: u32,
    /// The portrait control's unit (0x68ac98 +0x26) while a unit talks; None for a
    /// transmission or none.
    talk_unit: Option<u32>,
    /// 0x45edd0: ms left on the real-time transmission timer (0x45e610).
    trans_left: Option<u32>,
    /// 0x59724c: the selection last sent to the simulation as select commands.
    sent_selection: Vec<u32>,
    /// Command card submenu (button set index) opened by a Menu button, e.g. SCV "Build".
    pub card_menu: Option<u16>,
    /// Building type being placed, and whether the spot under the cursor is valid.
    pub placing: Option<(u8, u16)>,
    /// Targeting mode: orders for a ground / unit / fog click (0x641691..3).
    pub targeting: Option<(u8, u8, u8)>,
    /// Last refused button or click (stat_txt message id, 0 for none), put on the
    /// error line by text_tick.
    pub error: u16,
    /// The message lines, timed by `clock_ms` (GetTickCount in the game).
    pub msgs: crate::msgs::Messages,
    pub clock_ms: u32,
    /// The options' gamma (0x656190, 60..=140; 100 leaves the palette as it is).
    pub gamma: i16,
    /// 0x6509c8: `clock_ms` when the game was paused; 0x6d5be8: the replay's pause flag
    /// (0x6d11b0) then.
    pub pause_tick: u32,
    pub pause_replay_saved: bool,
    /// The chat line (0x4f32d0).
    pub chat: crate::chat::Chat,
    pub place_ok: bool,
    /// 0x640890: the placement tile under the cursor (set by check_placement).
    pub place_tile: (i16, i16),
    /// 0x640958: the placement check result at `place_tile` (0 = free).
    pub place_err: u8,
    /// Screen-space drag rectangle while box-selecting.
    pub drag_box: Option<(i32, i32, i32, i32)>,
    pub a: &'a Assets,
    pub chk: Chk,
    pub ts: Tileset,
    pub terrain: (usize, usize, Vec<u8>),
    pub player_colors: [[u8; 8]; 12],
    pub minimap: bw::minimap::Minimap,
    local: u8,
    shadow: [u8; 256],
    /// The tileset's dark.pcx: 32 darkness levels x 256 colors (0x5973a0).
    dark: Vec<u8>,
    fog: crate::fog::Fog,
    /// game/tselect.pcx: selection circle colors, 8 per color (0x5240b8).
    tselect: Vec<u8>,
    /// game/thpbar.pcx: health bar colors (0x66fbe4).
    thpbar: [u8; 19],
    remaps: [Vec<u8>; 7],
}

/// Melee maps: only start locations and neutral units.
pub fn is_melee_map(chk: &Chk) -> bool {
    chk.section(b"UNIT").unwrap_or(&[]).chunks_exact(36).all(|r| r[16] >= 8 || u16::from_le_bytes([r[8], r[9]]) == 214)
}

impl<'a> Scene<'a> {
    /// (Call `prepare_map` on the assets first.)
    pub fn new(a: &'a Assets, chk: Chk) -> io::Result<Scene<'a>> {
        Scene::start(a, chk, None, None)
    }

    /// The game the glue menus set up (0x4e0710). (Call `prepare_setup` on the assets first.)
    pub fn from_setup(a: &'a Assets, g: &GameSetup) -> io::Result<Scene<'a>> {
        Scene::start(a, Chk::parse(&g.chk), None, Some(g))
    }

    /// The game a replay recorded, set up as the replay's header says.
    /// (Call `prepare_replay` on the assets first.)
    pub fn from_replay(a: &'a Assets, r: &crate::replay::Replay) -> io::Result<Scene<'a>> {
        let mut s = Scene::start(a, Chk::parse(&r.chk), Some(&r.header), None)?;
        s.st.replay_frames = r.header.frames;
        Ok(s)
    }

    fn start(a: &'a Assets, chk: Chk, rep: Option<&crate::replay::Header>, gs: Option<&GameSetup>) -> io::Result<Scene<'a>> {
        let ts = Tileset::load(&a.files, chk.tileset())?;
        let terrain = map::render_terrain(&chk, &ts);
        let (tw, _, tunit) = pcx(&a.files.read("game/tunit.pcx")?);
        let colr = match rep {
            // A replay's header keeps each player's color as the game assigned it.
            Some(h) => h.slots.iter().take(8).map(|s| s.color as u8).collect(),
            None => match gs {
                // 0x57f21c: the colors 0x49b360 dealt.
                Some(g) => g.colors.to_vec(),
                None => chk.section(b"COLR").unwrap_or(&[0, 1, 2, 3, 4, 5, 6, 7]).to_vec(),
            },
        };
        let mut player_colors = [[0u8; 8]; 12];
        for (p, pc) in player_colors.iter_mut().enumerate() {
            let c = if p < 8 { colr.get(p).copied().unwrap_or(p as u8) as usize } else { p };
            for k in 0..8 {
                pc[k] = tunit.get((c * 8 + k) % tw.max(1)).copied().unwrap_or(0);
            }
        }
        let tsname = map::TILESETS[chk.tileset()];
        let tbl = |n: &str| a.files.read(&format!("tileset/{tsname}/{n}.pcx")).map(|d| pcx(&d).2).unwrap_or_default();
        // Shadows (draw function 10, 0x40b6f0) use 0x5985a0: dark.pcx (loaded at
        // 0x5973a0 by 0x4bddd0) row 0x12.
        let dark = tbl("dark");
        let mut shadow = [0u8; 256];
        for (i, s) in shadow.iter_mut().enumerate() {
            *s = dark.get(0x12 * 256 + i).copied().unwrap_or(i as u8);
        }
        // images.dat remapping ids 1..=7
        let remaps = ["ofire", "gfire", "bfire", "bexpl", "trans50", "red", "green"].map(tbl);
        let tselect = pcx(&a.files.read("game/tselect.pcx")?).2;
        let mut thpbar = [0u8; 19];
        for (t, &c) in thpbar.iter_mut().zip(pcx(&a.files.read("game/thpbar.pcx")?).2.iter()) {
            *t = c;
        }
        let mut st = new_game(a, &chk, rep, gs)?;
        let local = st.local_player;
        let mut minimap = load_minimap(&a.files, &chk)?;
        // 0x581dd6: each player's minimap color is the tminimap.pcx entry of its color.
        let tm = pcx(&a.files.read("game/tminimap.pcx")?).2;
        for p in 0..12 {
            let c = if p < 8 { colr.get(p).copied().unwrap_or(p as u8) as usize } else { p };
            minimap.player_colors[p] = tm.get(c).copied().unwrap_or(0);
        }
        st.interface_colors = minimap.colors;
        with_data(a, |d| {
            // 0x4d9950 -> 0x4d9530: the game steps twice before the frame loop counts.
            // 0x4d9950 first sets the 100-frame counter 0x51ce98 to 1, so the first step is a
            // 100th frame (vision reset) and the resets come every 100 steps from there.
            st.counter_100 = 1;
            st.game_step(d);
            st.game_step(d);
            minimap.refresh(d, &mut st)
        });
        let creep_grp = a.files.read(&format!("tileset/{}.grp", map::TILESETS[chk.tileset()])).ok().map(crate::grp::Grp::parse);
        Ok(Scene { st, creep_grp, minimap, voices: crate::voices::Voices::new(), portrait: Default::default(), talk_left: 0, talk_unit: None, trans_left: None, sent_selection: Vec::new(), card_menu: None, placing: None, targeting: None, error: 0, msgs: Default::default(), chat: Default::default(), clock_ms: 0, gamma: 100, pause_tick: 0, pause_replay_saved: false, place_ok: false, place_tile: (0, 0), place_err: 0, drag_box: None, a, chk, ts, terrain, player_colors, local, shadow, dark, fog: crate::fog::Fog::new(), tselect, thpbar, remaps })
    }

    /// Send the local selection to the simulation (0x4c0860) and return the local
    /// player's selection as the simulation sees it (0x6284e8).
    fn sim_selection(&mut self) -> Vec<u32> {
        let me = self.local;
        let mut sent = std::mem::take(&mut self.sent_selection);
        let pkts = self.st.selection_packets(&mut sent, &self.st.selected);
        self.sent_selection = sent;
        let st = &mut self.st;
        with_data(self.a, |d| {
            for p in &pkts {
                st.execute_command(d, me, &[], p);
            }
        });
        bw::select::selection_units(&self.st.selections[me as usize]).to_vec()
    }

    /// 0x4967e0: the screen position centered on the selected group: the units' mean
    /// position less half the screen (0x140, 0xc8), on 8-pixel steps.
    pub fn group_centre(&self) -> Option<(i32, i32)> {
        let n = self.st.selected.len() as i32;
        if n == 0 {
            return None;
        }
        let (sx, sy) = self.st.selected.iter().fold((0i32, 0i32), |(ax, ay), &u| {
            let (x, y) = self.st.unit_sprite_pos(u);
            (ax + x as i32, ay + y as i32)
        });
        Some(((sx / n - 0x140) & !7, (sy / n - 0xc8) & !7))
    }

    /// Hotkey group command 0x13 (`action` 0 set, 1 select, 2 add); the local selection
    /// follows the simulation's.
    pub fn hotkey(&mut self, action: u8, g: u8) {
        self.sim_selection();
        let me = self.local;
        let st = &mut self.st;
        with_data(self.a, |d| st.execute_command(d, me, &[], &[0x13, action, g]));
        let sel = bw::select::selection_units(&self.st.selections[me as usize]).to_vec();
        if sel != self.st.selected {
            self.card_menu = None;
            self.placing = None;
        }
        self.set_selected(sel);
        self.sent_selection = self.st.selected.clone();
    }

    /// 0x49ae40: make `units` the local selection (0x6284b8).
    pub fn set_selected(&mut self, mut units: Vec<u32>) {
        let n = units.len();
        let st = &mut self.st;
        with_data(self.a, |d| st.selection_set(d, &mut units, n));
    }

    /// 0x48ccb0: put stat_txt line `id` (1-based) up for 7 seconds (at the next
    /// text_tick).
    pub fn show_error(&mut self, id: u16) {
        self.error = id;
    }

    /// 0x488800: pause the game (0x6509c4) when it is not.
    pub fn pause_game(&mut self) {
        if self.st.trig.paused {
            return;
        }
        self.st.trig.paused = true;
        // 0x484cc0 (-> 0x4d2ff0): clears the 0x5968a0 handler table, ClipCursor(0x6cddb0) when
        // 0x6d5dd0, and installs the in-game handlers (0x4846e0 hotkeys, 0x46ff70 / 0x4564e0
        // mouse). scbw reads input directly (no handler tables, no Win32 cursor), so nothing to reset.
        self.pause_tick = self.clock_ms;
        if self.st.replay {
            self.pause_replay_saved = self.st.replay_paused;
            // 0x4deb90: the replay panel refreshes (drawn every frame here).
        }
    }

    /// 0x488790: unpause; `adjust` takes the paused real time off the game-time counter.
    pub fn unpause_game(&mut self, adjust: bool) {
        if !self.st.trig.paused {
            return;
        }
        if adjust {
            todo_port!("0x488790", "subtract the paused seconds from the real-time game timer 0x6d5bcc (0x57f240)");
        }
        self.st.trig.paused = false;
        // 0x4deb90 (replay panel refresh when 0x6d5be8 == 0) is drawn every frame here.
    }

    /// Advance the message clock `ms`: pending errors and trigger texts go up
    /// (`subtitles`: 0x6cdfec 0x400), expired lines go blank (0x48cc70).
    pub fn text_tick(&mut self, ui: &crate::ui::Ui, ms: u32, subtitles: bool) {
        if !self.msgs.ready() {
            self.msgs.reset(&ui.font);
        }
        let now = self.clock_ms;
        if let Some(e) = ui.stat_txt.get((std::mem::take(&mut self.error) as usize).wrapping_sub(1)) {
            self.msgs.error(e.split('\0').next().unwrap_or("").as_bytes(), now);
        }
        for (id, always, min_ms) in std::mem::take(&mut self.st.shown_text) {
            if let Some(t) = self.chk.string_bytes(id).filter(|_| always || subtitles) {
                self.msgs.trigger_text(&ui.font, t, now, crate::msgs::reading_ms(t).max(min_ms));
            }
        }
        for (fmt, p) in std::mem::take(&mut self.st.notices) {
            // 0x41f1b0 formats the network.tbl line; 0x48d0c0 shows it 7 s.
            let Some(f) = ui.network.get(fmt as usize) else { continue };
            let f = f.split('\0').next().unwrap_or("");
            let name = self.st.player_names.get(p as usize).map_or("", String::as_str);
            let t = f.replacen("%s", name, 1).replacen("%d", &(p as u32 + 1).to_string(), 1);
            self.msgs.print(&ui.font, t.as_bytes(), now, 0, 0x17, &mut self.st.ui_sounds);
        }
        for (t, x, y) in std::mem::take(&mut self.st.unplaced) {
            // 0x41f1b0 formats network.tbl line 0x11 with the unit's name; 0x48d140 shows it
            // 7 s in color 3 and plays sound 2.
            let Some(f) = ui.network.get(0x11) else { continue };
            let f = f.split('\0').next().unwrap_or("");
            let name = with_data(self.a, |d| crate::statdata::unit_name(d, &ui.stat_txt, t));
            let line = f.replacen("%s", &name, 1).replacen("%d", &x.to_string(), 1).replacen("%d", &y.to_string(), 1);
            self.msgs.print(&ui.font, line.as_bytes(), now, 0, 2, &mut self.st.ui_sounds);
        }
        for (p, text) in std::mem::take(&mut self.st.chat) {
            let st = &self.st;
            let tbl = ui.network.get(0x1a).map_or("", |s| s.split('\0').next().unwrap_or(""));
            let (line, color) = crate::msgs::chat_line(p, &text, st.local_player, st.player_types.get(p as usize).copied().unwrap_or(0), st.player_names.get(p as usize).map_or("", String::as_str), tbl);
            self.msgs.add(&ui.font, &line, color, now.wrapping_add(7000), 0);
            self.st.ui_sounds.push(0x17);
        }
        self.clock_ms = now.wrapping_add(ms);
        self.msgs.expire(self.clock_ms);
    }

    /// 0x4d14d0: which cursor shows with the mouse at screen (x, y): scrolling at the
    /// screen edges (0x4d12a0), else over the minimap (0x6556fc) or the map (0x4d1460),
    /// where units under it pick a magnifier or target color (0x4d13e0).
    pub fn cursor_kind(&mut self, x: i32, y: i32, vx: i32, vy: i32, dragging: bool) -> usize {
        let (mw, mh) = (self.st.map_px.0 as i32, self.st.map_px.1 as i32);
        let dx = if x < 2 && vx > 0 { -1 } else if x >= 0x27e && vx < mw - SCREEN_W { 1 } else { 0 };
        let dy = if y < 2 && vy > 0 { -1 } else if y >= 0x1de && vy < mh - (SCREEN_H - 80) { 1 } else { 0 };
        if dx != 0 || dy != 0 {
            // 0x51275c, indexed by dx + 3 * dy.
            const SCROLL: [usize; 9] = [18, 11, 12, 17, 0, 13, 16, 15, 14];
            return SCROLL[(dx + 1 + (dy + 1) * 3) as usize];
        }
        if on_minimap_pt(x, y) {
            return if self.targeting.is_some() { 2 } else { 0 };
        }
        if !(0..SCREEN_W).contains(&x) || !(0..400).contains(&y) {
            return 0;
        }
        if dragging {
            return 9;
        }
        if self.placing.is_some() {
            return 0;
        }
        let u = self.unit_at(x + vx, y + vy);
        let enemy = |st: &bw::State, u: u32| st.alliances[st.local_player as usize][st.units[u as usize].player as usize] == 0 && !st.replay;
        match (self.targeting.is_some(), u) {
            (true, None) => 5,
            (true, Some(u)) if self.st.units[u as usize].player == 11 => 2,
            (true, Some(u)) => if enemy(&self.st, u) { 3 } else { 4 },
            (false, Some(u)) if self.st.units[u as usize].player == 11 => 8,
            (false, Some(u)) => 6 + enemy(&self.st, u) as usize,
            (false, None) => 0,
        }
    }

    /// Advance one frame; returns sounds (sfx id, x, y) requested this frame.
    pub fn step(&mut self) -> Vec<(u16, i32, i32)> {
        let st = &mut self.st;
        with_data(self.a, |d| {
            // 0x4d9670: the frame counter advances before each step.
            st.frame += 1;
            st.game_step(d);
            // 0x4d9670: then the triggers, with the RNG on, given the frame time of the
            // game speed (0x5124d8).
            st.rng.enabled = true;
            st.run_triggers(d, bw::triggers::FRAME_MS[st.replay_speed.min(6) as usize]);
            st.rng.enabled = false;
            self.minimap.update(d, st);
        });
        // 0x4bd580: the game screen is drawn every frame, its visible tiles refreshed
        // from MTXM first (creep).
        self.st.refresh_screen_tiles();
        self.st.alert_tick();
        self.voices.tick();
        // 0x4a4e40 runs on a 200 ms timer: about every fifth frame at 42 ms.
        if self.st.frame % 5 == 0 {
            self.minimap.blink_phase = !self.minimap.blink_phase;
        }
        // 0x4a4f10 runs on a 100 ms timer: every 2nd or 3rd frame.
        if self.st.frame % 5 == 0 || self.st.frame % 5 == 2 {
            self.minimap.tick_pings();
        }
        if self.st.shown_error != 0 {
            let e = std::mem::take(&mut self.st.shown_error);
            self.show_error(e);
        }
        // 0x49f7a0 / 0x4ed000 emptied the selection: placement and targeting end (0x48d9a0,
        // 0x48e310, 0x48ca10: the button set goes back to 0xe4).
        if std::mem::take(&mut self.st.selection_emptied) && (self.placing.is_some() || self.targeting.is_some()) {
            self.placing = None;
            self.targeting = None;
            self.card_menu = None;
        }
        self.st.sounds.drain(..).map(|(id, x, y)| (id, x as i32, y as i32)).collect()
    }

    /// Create a finished unit (debug spawning).
    pub fn spawn(&mut self, kind: u16, owner: u8, x: i16, y: i16) -> Option<u32> {
        let st = &mut self.st;
        with_data(self.a, |d| {
            let u = st.create_unit(d, kind, x, y, owner)?;
            st.finish_unit_pre(d, u);
            if st.place_unit(d, u) {
                st.finish_unit_post(d, u);
            }
            Some(u)
        })
    }

    pub fn local_player(&self) -> u8 {
        self.local
    }

    /// Local player's race (0 zerg, 1 terran, 2 protoss): 0x57f1e2, which 0x4ee180 sets
    /// to terran (1) when watching a replay (dialog graphics dlgs\terran.grp, the music).
    pub fn local_race(&self) -> usize {
        if self.st.replay {
            return 1;
        }
        (self.st.player_races[self.local as usize] as usize).min(2)
    }

    /// (used, max) supply in whole units, for the HUD.
    pub fn supply(&self, player: u8, race: usize) -> (u32, u32) {
        let p = &self.st.players;
        let (used, max, prov) = (p.supply_used[race][player as usize], p.supply_max[race][player as usize], p.supply_provided[race][player as usize]);
        ((used.max(0) as u32).div_ceil(2), (prov.min(max).max(0) / 2) as u32)
    }

    fn sprite_seen(&self, s: u32) -> bool {
        let sp = &self.st.sprites[s as usize];
        sp.flags & bw::sprite::sflag::HIDDEN == 0 && sp.visibility & self.st.local_vision_mask != 0
    }

    /// (x, y, unit) for every selectable unit the local player can see.
    pub fn unit_positions(&self) -> Vec<(i32, i32, u32)> {
        let flags = &self.a.dat().units.flags;
        let mut v = Vec::new();
        let mut u = self.st.visible_units.head;
        while u != NIL {
            let un = &self.st.units[u as usize];
            if flags[un.unit_type as usize] & 0x10 == 0 && self.st.unit_alive(u) && self.sprite_seen(un.sprite) {
                let s = &self.st.sprites[un.sprite as usize];
                v.push((s.x as i32, s.y as i32, u));
            }
            u = un.link.next;
        }
        v
    }

    /// Units whose units.dat bounds intersect the map-space rectangle.
    pub fn units_in_rect(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<u32> {
        let d = &self.a.dat().units.dimensions;
        self.unit_positions()
            .into_iter()
            .filter(|&(x, y, u)| {
                let dm = d[self.st.units[u as usize].unit_type as usize];
                let (l, t, r, b) = ((dm & 0xFFFF) as i32, (dm >> 16 & 0xFFFF) as i32, (dm >> 32 & 0xFFFF) as i32, (dm >> 48) as i32);
                x - l <= x1.max(x0) && x + r >= x0.min(x1) && y - t <= y1.max(y0) && y + b >= y0.min(y1)
            })
            .map(|(_, _, u)| u)
            .collect()
    }

    /// Draw the view at (vx, vy) into an indexed buffer of vw*vh.
    pub fn draw(&self, vx: i32, vy: i32, vw: i32, vh: i32, out: &mut [u8]) {
        // Terrain from the live megatile map (0x628494), which creep changes update.
        let (tw, th) = (self.terrain.0 as i32, self.terrain.1 as i32);
        let mw = self.st.terrain.width as i32;
        let mut tile = [0u8; 32 * 32];
        let (x0, y0) = (vx.clamp(0, tw - 1) >> 5, vy.clamp(0, th - 1) >> 5);
        let (x1, y1) = ((vx + vw - 1).clamp(0, tw - 1) >> 5, (vy + vh - 1).clamp(0, th - 1) >> 5);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let i = (ty * mw + tx) as usize;
                let raw = self.st.terrain.megatiles[i];
                self.ts.draw_megatile(raw as usize & 0x7fff, &mut tile, 32);
                // 0x49b9f0: a flagged tile next to creep gets its edge piece.
                let edge = self.st.terrain.creep_edges[i];
                if raw & 0x8000 != 0 && edge != 0 {
                    if let Some(g) = &self.creep_grp {
                        g.draw(edge as usize - 1, false, 0, 0, &mut tile, 32, 32, crate::grp::Blend::Normal);
                    }
                }
                for py in 0..32 {
                    let oy = ty * 32 + py - vy;
                    if oy < 0 || oy >= vh {
                        continue;
                    }
                    for px in 0..32 {
                        let ox = tx * 32 + px - vx;
                        if ox >= 0 && ox < vw {
                            out[(oy * vw + ox) as usize] = tile[(py * 32 + px) as usize];
                        }
                    }
                }
            }
        }
        // Sprites by elevation, then tile row (row lists as the game keeps them).
        let mut order: Vec<u32> = Vec::new();
        for l in &self.st.sprite_rows {
            let mut s = l.head;
            while s != NIL {
                order.push(s);
                s = self.st.sprites[s as usize].link.next;
            }
        }
        order.retain(|&s| {
            let sp = &self.st.sprites[s as usize];
            self.sprite_seen(s) && (sp.x as i32 - vx).abs() < vw + 256 && (sp.y as i32 - vy).abs() < vh + 256
        });
        // 0x42d4c0 keys each sprite, 0x42d400 / 0x42d370 / 0x42d460 heap-sort them and
        // 0x498d40 draws in ascending key order.
        let keys = order.iter().map(|&s| (sprite_draw_key(&self.st.sprites[s as usize], s), s)).collect();
        let mut order = heap_draw_order(keys);
        // 0x4bd580: sprites (0x498d40), then fog of war over terrain and sprites alike
        // (0x4808e0), then the right-click marker (0x488180) unshaded.
        let fogged = order.len();
        // 0x488180: the right-click marker is drawn on top while shown, though its
        // sprite stays hidden.
        if let (true, Some(t)) = (self.st.marker_shown, self.st.marker_thingy) {
            let s = self.st.thingies[t as usize].sprite;
            if s != NIL {
                order.push(s);
            }
        }
        // Selection circles (0x4d7070 / 0x4d6810): image 0x231 + the sprite's circle size,
        // below the sprite, colored from tselect.pcx by 0x581d6a (State::circle_color; own 0, ally 1, enemy 2).
        let sp_dat = &self.a.dat().sprites;
        let circled: std::collections::HashMap<u32, (u8, u32)> = self
            .st
            .selected
            .iter()
            .map(|&u| {
                let un = &self.st.units[u as usize];
                (un.sprite, (self.st.circle_color[un.player as usize], u))
            })
            .collect();
        // A clicked sprite's flash circle (0x4ebbd0, sprite flag 1) is drawn like a selected one.
        let flashed: Vec<(u32, u8)> = order
            .iter()
            .filter(|&&s| self.st.sprites[s as usize].flags & 1 != 0 && self.st.sprites[s as usize].selection_timer != 0)
            .map(|&s| {
                let p = self.st.sprites[s as usize].owner as usize;
                (s, self.st.circle_color[p])
            })
            .collect();
        let total = order.len();
        for (k, s) in order.into_iter().enumerate() {
            if k == fogged {
                self.fog.shade(&self.st, &self.dark, vx, vy, out, vw as usize, vh as usize);
            }
            let sp = &self.st.sprites[s as usize];
            if let Some(c) = circled.get(&s).map(|&(c, _)| c).or_else(|| flashed.iter().find(|f| f.0 == s).map(|f| f.1)) {
                let id = sp.sprite_id as usize;
                if let Some(g) = self.a.grp(0x231 + sp_dat.selection_circle[id] as u16) {
                    let mut table = [0u8; 256];
                    for (k, t) in table.iter_mut().enumerate() {
                        *t = if k < 8 { self.tselect.get(c as usize * 8 + k).copied().unwrap_or(k as u8) } else { k as u8 };
                    }
                    let x = sp.x as i32 - g.w / 2 - vx;
                    let y = sp.y as i32 + sp_dat.selection_offset[id] as i32 - g.h / 2 - vy;
                    g.draw(0, false, x, y, out, vw, vh, Blend::Colors(&table));
                }
            }
            let mut i = sp.images.tail;
            while i != NIL {
                let img = &self.st.images[i as usize];
                i = img.link.prev;
                if img.flags & bw::sprite::iflag::HIDDEN != 0 {
                    continue;
                }
                let Some(g) = self.a.grp(img.image_id) else { continue };
                let x = sp.x as i32 + img.x as i32 - g.w / 2 - vx;
                let y = sp.y as i32 + img.y as i32 - g.h / 2 - vy;
                // 0x4d5cd0 (0x4d5c90 flipped): draw function 12 fills the image's shape with
                // frame `coloring` of image 0xd2's GRP (DAT_0051d218).
                let warp = (img.modifier == 12).then(|| self.a.grp(0xd2).and_then(|t| t.pixels(img.coloring as u32 as usize))).flatten();
                let blend = match img.modifier {
                    12 => match &warp {
                        Some((px, w, h)) => Blend::Texture(px, *w, *h, &self.player_colors[(sp.owner as usize).min(11)]),
                        None => Blend::Normal,
                    },
                    10 => Blend::Remap(&self.shadow),
                    9 => match self.a.dat().images.remapping[img.image_id as usize] {
                        r @ 1..=7 => Blend::Table(&self.remaps[r as usize - 1]),
                        _ => Blend::Normal,
                    },
                    _ => Blend::Player(&self.player_colors[(sp.owner as usize).min(11)]),
                };
                g.draw(img.frame as usize, img.flags & bw::sprite::iflag::FLIPPED != 0, x, y, out, vw, vh, blend);
            }
            // Health bar (0x4d6420 / 0x4d6010), above the sprite's images.
            if let Some(&(_, u)) = circled.get(&s) {
                let t = self.st.units[u as usize].unit_type as usize;
                if self.a.dat().units.flags[t] & 0x2000_0000 == 0 {
                    let id = sp.sprite_id as usize;
                    let ch = self.a.grp(0x231 + sp_dat.selection_circle[id] as u16).and_then(|g| g.frame(0).map(|f| f.h)).unwrap_or(0);
                    let st = &self.st;
                    with_data(self.a, |d| {
                        let (w, h, dy) = crate::hpbar::layout(st, d, u, ch);
                        // 0x4d57b0: map position, clipped to the 640x400 game screen.
                        let (mut x, mut y) = (sp.x as i32 - w / 2 - vx, sp.y as i32 + dy - h / 2 - vy);
                        let mut clip = crate::hpbar::Clip { left: 0, top: 0, w, h };
                        if x < 0 {
                            clip.w += x;
                            clip.left = -x;
                            x = 0;
                        }
                        clip.w = clip.w.min(vw - x);
                        if y < 0 {
                            clip.h += y;
                            clip.top = -y;
                            y = 0;
                        }
                        clip.h = clip.h.min(vh.min(400) - y);
                        let mut surf = crate::hpbar::Surface { px: out, w: vw, h: vh };
                        crate::hpbar::draw(&mut surf, &self.thpbar, st, d, u, x, y, clip);
                    });
                }
            }
        }
        if total == fogged {
            self.fog.shade(&self.st, &self.dark, vx, vy, out, vw as usize, vh as usize);
        }
    }

    pub fn palette(&self) -> &[[u8; 3]; 256] {
        &self.ts.palette
    }

    /// The unit under map point (x, y), if any (0x46f3a0).
    pub fn unit_at(&mut self, x: i32, y: i32) -> Option<u32> {
        let st = &mut self.st;
        let u = with_data(self.a, |d| st.unit_at_point(d, x as i16, y as i16));
        (u != NIL).then_some(u)
    }

    /// Right-click at map point `p` (0x4c2370).
    pub fn right_click(&mut self, p: (i32, i32), queued: bool) {
        let t = self.unit_at(p.0, p.1).unwrap_or(NIL);
        self.click_feedback(p, t);
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        with_data(self.a, |d| st.command_right_click(d, me, &sel, (p.0 as i16, p.1 as i16), t, 0xe4, queued));
        let (active, selected) = (self.client_selection().active, self.st.selected.clone());
        let st = &mut self.st;
        if with_data(self.a, |d| st.right_click_accepted(d, &selected, active, t)) {
            self.voice_yes();
        }
    }

    /// 0x458d50 (cached by 0x458e90 / 0x459a30): the icon of the card button drawn lit
    /// (0x68c1bc, state 2 in 0x458900): orders.dat highlight of the active unit's order,
    /// or of its subunit's order when that has none (or the former is 0xe5).
    pub fn card_highlight(&self) -> Option<u16> {
        let a = self.client_selection().active;
        if a == NIL {
            return None;
        }
        let un = &self.st.units[a as usize];
        let hl = |o: u8| with_data(self.a, |d| d.dat.orders.highlight[o as usize]);
        let sub = (un.subunit != NIL).then(|| hl(self.st.units[un.subunit as usize].order));
        let s2 = sub.unwrap_or(0xffff);
        let mut s3 = hl(un.order);
        if s3 == 0xffff || (s2 != 0xffff && s3 == 0xe5) {
            s3 = s2;
        }
        (s3 != 0xffff).then_some(s3)
    }

    /// 0x4563a0 / 0x48f4d0: the status unit (first selected) acknowledges an order.
    pub fn voice_yes(&mut self) {
        let Some(&u) = self.st.selected.first() else { return };
        let (st, v) = (&self.st, &mut self.voices);
        if let Some(id) = with_data(self.a, |d| v.yes(st, d, u)) {
            self.st.ui_sounds.push(id);
            self.st.talked.push((u, id));
        }
    }

    /// 0x49aef0 / 0x48f910: the highest-ranked selected unit (units.dat rank + its
    /// rank increase, 0x49a350) answers a new selection.
    pub fn voice_what(&mut self) {
        let st = &self.st;
        let best = with_data(self.a, |d| {
            self.st.selected.iter().copied().max_by_key(|&u| {
                let un = &st.units[u as usize];
                d.dat.units.rank[un.unit_type as usize] as u32 + un.unknown_8e as u32
            })
        });
        let Some(u) = best else { return };
        let (st, v) = (&self.st, &mut self.voices);
        if let Some(id) = with_data(self.a, |d| v.what(st, d, u)) {
            self.st.ui_sounds.push(id);
            self.st.talked.push((u, id));
        }
    }

    /// The status bar's selection (active unit, count).
    pub fn client_selection(&self) -> bw::buttons::Selection {
        with_data(self.a, |d| self.st.client_selection(d, &self.st.selected))
    }

    /// 0x48ca10: end targeting: the card goes back to the unit's own set (0xe4). Whether it was on.
    /// (The cancel button's state, cursor and dirty rects are drawn from the state each frame.)
    pub fn end_targeting(&mut self) -> bool {
        let was = self.targeting.take().is_some();
        if was {
            self.card_menu = None;
        }
        was
    }

    /// 0x4599a0
    /// 0x458de0: the button set on the card: an open menu, the cancel sets while targeting
    /// (0xe5) or placing (0xe6), else the active unit's set (+0x94).
    pub fn card_set(&self, s: &bw::buttons::Selection) -> Option<usize> {
        // 0x458de0: a replay always shows its controls (0xf8 while paused).
        if self.st.replay {
            return Some(if self.st.replay_paused { 0xf8 } else { 0xf9 });
        }
        if self.targeting.is_some() {
            return Some(0xe5);
        }
        if self.placing.is_some() {
            return Some(0xe6);
        }
        if let Some(m) = self.card_menu {
            return Some(m as usize);
        }
        // 0x4599a0: 0xe4 shows 0x458bc0's set for several units, else the active unit's.
        if s.active == NIL {
            return None;
        }
        let multi = if s.count > 1 { with_data(self.a, |d| self.st.multi_card_set(d, s)) } else { 0xe4 };
        Some(if multi != 0xe4 { multi as usize } else { self.st.units[s.active as usize].button_set as usize })
    }

    /// The command card (0x4591d0).
    pub fn card(&self) -> bw::buttons::Card {
        let s = self.client_selection();
        match self.card_set(&s) {
            Some(set) => with_data(self.a, |d| self.st.command_card(d, set, self.local, &s)),
            None => [None; 9],
        }
    }

    /// Press a command card button.
    pub fn press(&mut self, btn: &bw::buttons::Button, queued: bool) {
        use bw::buttons::ButtonAction::*;
        let s = self.client_selection();
        let act = with_data(self.a, |d| self.st.button_action(d, btn, self.local, &s, queued));
        match act {
            None => {}
            Send(p) => {
                let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
                with_data(self.a, |d| st.execute_command(d, me, &sel, &p));
            }
            Target { ground, unit, fog } => self.targeting = Some((ground, unit, fog)),
            Place { order, unit_type } => self.placing = Some((order, unit_type)),
            // 0x459af0 / 0x4599a0: set 0xe4 means the selected unit's own button set.
            Menu(m) => self.card_menu = (m != 0xe4).then_some(m),
            CancelTargeting => {
                self.end_targeting();
            }
            CancelPlacement => self.placing = Option::None,
            SelectLarvae(l) => self.set_selected(l),
            Error(Option::None) => self.show_error(btn.disabled_str),
            Error(Some((sound, msg))) => {
                self.st.ui_sounds.push(sound);
                self.show_error(msg);
            }
            ReplayPause => self.st.replay_paused = !self.st.replay_paused,
            ReplayFaster => {
                let st = &mut self.st;
                if st.replay_speed < 6 {
                    st.replay_speed += 1;
                } else if st.replay_mult < 16 {
                    st.replay_mult *= 2;
                }
            }
            ReplaySlower => {
                let st = &mut self.st;
                if st.replay_mult > 1 {
                    st.replay_mult /= 2;
                } else if st.replay_speed > 0 {
                    st.replay_speed -= 1;
                }
            }
        }
    }

    /// A map click while targeting (0x46f5b0).
    /// 0x456490: the clicked unit's selection circle flashes (sprite +0xf), or the
    /// marker shows where the ground was clicked. (Fog sprites, 0x487a10, are not
    /// ported.)
    fn click_feedback(&mut self, p: (i32, i32), t: u32) {
        if t != NIL {
            let s = self.st.units[t as usize].sprite;
            self.st.sprites[s as usize].selection_timer = 0x1f;
            return;
        }
        let st = &mut self.st;
        with_data(self.a, |d| st.show_marker(d, p.0 as i16, p.1 as i16));
    }

    pub fn target_click(&mut self, p: (i32, i32), queued: bool) {
        let Some(modes) = self.targeting.take() else { return };
        let t = self.unit_at(p.0, p.1).unwrap_or(NIL);
        self.click_feedback(p, t);
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        let r = with_data(self.a, |d| st.target_click(d, me, &sel, modes, (p.0 as i16, p.1 as i16), t, 0xe4, queued));
        if let Err(e) = r {
            self.show_error(e);
        }
        self.card_menu = Option::None;
        self.voice_yes();
    }

    /// 0x48d660: the placement tile (0x640890) and its screen position for the cursor `c`
    /// (screen px) with the screen at `v`: the screen cell under the cursor, counted from
    /// the screen's first tile; a tile left of / above the screen moves on by one.
    /// None outside the 640x400 map view.
    pub fn placement_cursor(c: (i32, i32), v: (i32, i32)) -> Option<((i16, i16), (i32, i32))> {
        if !(0..0x280).contains(&c.0) || !(0..400).contains(&c.1) {
            return None;
        }
        let mut t = ((c.0 >> 5) + (v.0 >> 5), (c.1 >> 5) + (v.1 >> 5));
        let mut p = (t.0 * 32 - v.0, t.1 * 32 - v.1);
        if p.0 < 0 {
            t.0 += 1;
            p.0 += 32;
        }
        if p.1 < 0 {
            t.1 += 1;
            p.1 += 32;
        }
        Some(((t.0 as i16, t.1 as i16), p))
    }

    /// 0x48dce0: the placement check at the cursor tile (grid slot 1, remembered creep and
    /// fog). An addon (units.dat flag 0x2) also needs its parent building's spot, the
    /// addon offset (0x662538) back, free (slot 0); the addon's failure wins.
    fn placement_result(&mut self, t: u16) -> u8 {
        let (tx, ty) = self.place_tile;
        let (u, me) = (self.client_selection().active, self.local);
        let st = &mut self.st;
        with_data(self.a, |d| {
            let r = st.placement_check(d, u, me, tx, ty, t, 1, true, false, false);
            if d.dat.units.flags[t as usize] & 2 == 0 || u == NIL {
                return r;
            }
            let v = d.dat.units.addon_position[t as usize];
            let tile = |p: i32| ((p + ((p >> 31) & 0x1f)) >> 5) as i16;
            let r2 = st.placement_check(d, u, me, tx - tile(v as u16 as i16 as i32), ty - tile((v >> 16) as u16 as i16 as i32), st.units[u as usize].unit_type, 0, true, false, false);
            if r == 0 { r2 } else { r }
        })
    }

    /// 0x48ddc0: update the placement box for the cursor `c` (screen px) with the screen at
    /// `v`: the tile under it (0x48d660) and the check result there (0x48dce0, kept in
    /// `place_err`). With the cursor off the map view the boxes are hidden (0x48d7b0) and
    /// nothing changes.
    pub fn check_placement(&mut self, c: (i32, i32), v: (i32, i32)) {
        let Some(_) = self.placing else { return };
        let Some((tile, _)) = Self::placement_cursor(c, v) else { return };
        self.check_placement_tile(tile);
    }

    /// 0x48dce0 for the placement tile `tile` itself (debug commands).
    pub fn check_placement_tile(&mut self, tile: (i16, i16)) {
        let Some((_, t)) = self.placing else { return };
        self.place_tile = tile;
        self.place_err = self.placement_result(t);
        self.place_ok = self.place_err == 0;
    }

    /// 0x48e5d0: the click while placing: a refused spot (0x48e4e0 with the result in
    /// 0x640958) gets the error voice and line (0x48f7b0) and placing goes on; a free one
    /// the race's placement sound (0x48eff0) and the build command (0x0c) at the tile,
    /// and placing ends (0x48e310).
    pub fn build(&mut self) {
        let Some((order, t)) = self.placing else { return };
        let (tx, ty) = self.place_tile;
        let (sel, me) = (self.sim_selection(), self.local);
        if self.place_err != 0 {
            if order == 0x47 {
                let u = self.client_selection().active;
                let st = &mut self.st;
                if u != NIL {
                    with_data(self.a, |d| st.land_refused_sound(d, u));
                }
            } else {
                let (u, r) = (self.client_selection().active, self.place_err);
                let st = &mut self.st;
                if u != NIL {
                    with_data(self.a, |d| st.placement_blocked_sound(d, u, bw::build::PLACEMENT_ERRORS.get(r as usize).copied().unwrap_or(0)));
                }
            }
            return;
        }
        self.placing = Option::None;
        let mut pkt = vec![0x0c, order];
        pkt.extend_from_slice(&(tx as u16).to_le_bytes());
        pkt.extend_from_slice(&(ty as u16).to_le_bytes());
        pkt.extend_from_slice(&t.to_le_bytes());
        // 0x48eff0: the building race's placement sound (sfx 4 + race).
        let race = with_data(self.a, |d| bw::State::supply_race_of(d, t));
        self.st.ui_sounds.push(4 + race);
        let st = &mut self.st;
        with_data(self.a, |d| st.execute_command(d, me, &sel, &pkt));
        self.card_menu = Option::None;
    }

    /// The selected building's statdata panel, if it shows one.
    pub fn status_panel(&self, ui: &crate::ui::Ui) -> Option<(crate::statdata::Panel, Vec<(i16, crate::statdata::Show)>)> {
        if self.st.selected.len() > 1 {
            return Some((crate::statdata::Panel::Group, crate::statdata::group_panel(&self.st.selected)));
        }
        let [u] = self.st.selected[..] else { return None };
        let st = &self.st;
        with_data(self.a, |d| {
            if let Some(p) = crate::statdata::transport_panel(st, d, &ui.stat_txt, u) {
                return Some((crate::statdata::Panel::Unit, p));
            }
            if let Some(p) = crate::statdata::morph_panel(st, d, &ui.stat_txt, u) {
                return Some((crate::statdata::Panel::Training, p));
            }
            crate::statdata::building_panel(st, d, &ui.stat_txt, u).or_else(|| Some((crate::statdata::Panel::Unit, crate::statdata::unit_panel(st, d, &ui.stat_txt, u))))
        })
    }

    /// The portrait follows the status unit (the first selected). 0x45efe0: a unit that
    /// talks (`st.talked`) takes it over, talking, for its sound's length (at least 1000 ms,
    /// 0x45ef50) unless a talk runs; then 0x45ec40 restores the selection's. `mode` is the
    /// video option (0 none, 1 still, 2 animating).
    pub fn portrait_tick(&mut self, ui: &crate::ui::Ui, audio: Option<&crate::audio::Audio>, ms: u32, mode: i16) {
        let talked = std::mem::take(&mut self.st.talked);
        let dropped = std::mem::take(&mut self.st.portrait_dropped);
        // A front end with a clock drives the real-time timers of trigger waits (headless
        // runs, and the screenshot mode with ms 0, have none).
        self.st.ui_attached = ms != 0;
        // 0x4c6de0: the run loop moves the camera; front ends without one end the scroll at
        // once (0x4894a0).
        if self.st.center_view.take().is_some() {
            self.st.trigger_wait_end();
        }
        // 0x45edd0: a transmission replaces the running timer (id 4); the talking portrait
        // (timer 3, 0x45ec40) and the trigger wait (0x45e610) end with it.
        let started = std::mem::take(&mut self.st.transmissions);
        if let Some(&(_, len)) = started.last() {
            self.trans_left = Some(len);
        }
        if let Some(left) = self.trans_left {
            if left <= ms {
                self.trans_left = None;
                self.talk_left = 0;
                self.st.trigger_wait_end();
            } else {
                self.trans_left = Some(left - ms);
            }
        }
        let Some(ps) = ui.portraits.as_ref() else { return };
        self.portrait.borrow_mut().animate = mode == 2;
        if let Some(&(t, len)) = started.last() {
            if let Some(p) = Some(self.a.dat().units.portrait[t as usize]).filter(|&p| p < 110 && mode != 0) {
                self.talk_left = len;
                self.talk_unit = None;
                self.portrait.borrow_mut().show_talking(ps, &self.a.files, p);
            }
        }
        // 0x45ea30: the generic advisor portrait (0x52) takes the unit owner's color.
        let portrait = |s: &Self, u: u32| {
            let p = s.a.dat().units.portrait[s.st.units[u as usize].unit_type as usize];
            let p = if p == 0x52 { s.st.advisor_portrait(s.st.sprites[s.st.units[u as usize].sprite as usize].owner) } else { p };
            Some(p).filter(|&p| p < 110 && mode != 0)
        };
        let files = &self.a.files;
        let mut pl = self.portrait.borrow_mut();
        if self.talk_left == 0 {
            if let Some(&(u, sfx)) = talked.first() {
                if let Some(p) = portrait(self, u) {
                    let len = audio.and_then(|au| au.duration_ms(files, self.a.dat(), sfx)).unwrap_or(0);
                    // ponytail: the callers' duration (EDI) also adds a per-sfx extra
                    // (0x68cd38) that is not read here.
                    self.talk_left = (len / 1000 * 1000).max(1000);
                    self.talk_unit = Some(u);
                    pl.show_talking(ps, files, p);
                }
            }
        } else {
            self.talk_left = self.talk_left.saturating_sub(ms);
            if self.talk_left == 0 {
                self.talk_unit = None;
                pl.portrait = None;
            }
        }
        // 0x49eb70: the talking unit died; the portrait hides until the talk timer ends
        // (0x45ec40 event 3 then shows the selection's again). The selection's own
        // portrait goes with the selection update (0x45ebc0).
        if self.talk_left != 0 && self.talk_unit.is_some_and(|u| dropped.contains(&u)) {
            self.talk_unit = None;
            pl.hide();
        }
        if self.talk_left == 0 {
            let p = self.st.selected.first().and_then(|&u| portrait(self, u));
            pl.show(ps, files, p);
        }
        pl.tick(ps, files, ms, mode == 2);
    }

    /// A click at screen (x, y) on a status panel button (0x4573a0); true if handled.
    pub fn status_click(&mut self, ui: &crate::ui::Ui, x: i32, y: i32, shift: bool, ctrl: bool) -> bool {
        let Some((panel, ctls)) = self.status_panel(ui) else { return false };
        for (id, show) in ctls {
            let Some((l, t, r, b)) = ui.statdata.screen_rect(id) else { continue };
            if (l..=r).contains(&x) && (t..=b).contains(&y) {
                let pkt = match show {
                    // A passenger: unload it (command 0x29 with its handle).
                    crate::statdata::Show::Cargo { unit, .. } => {
                        let h = self.st.unit_handle(unit) as u16;
                        Some(vec![0x29, h as u8, (h >> 8) as u8])
                    }
                    // 0x458220, a unit of a multiple selection: shift drops it, ctrl keeps
                    // the shown units of its type, else it alone stays selected.
                    crate::statdata::Show::GroupWire(unit) => {
                        let shown: Vec<u32> = self.st.selected.iter().copied().take(12).collect();
                        let ty = self.st.units[unit as usize].unit_type;
                        let sel = if shift {
                            shown.into_iter().filter(|&u| u != unit).collect()
                        } else if ctrl {
                            shown.into_iter().filter(|&u| self.st.units[u as usize].unit_type == ty).collect()
                        } else {
                            vec![unit]
                        };
                        self.set_selected(sel);
                        self.card_menu = None;
                        self.placing = None;
                        self.voice_what();
                        return true;
                    }
                    _ => crate::statdata::click(panel, id),
                };
                if let Some(pkt) = pkt {
                    self.command(|st, d, me, sel| st.execute_command(d, me, sel, &pkt));
                    return true;
                }
            }
        }
        false
    }

    /// 0x487100: run a replay command from map player `player` with that player's
    /// current selection.
    pub fn replay_command(&mut self, player: u8, p: &[u8]) {
        let sel = bw::select::selection_units(&self.st.selections[player as usize]).to_vec();
        let st = &mut self.st;
        with_data(self.a, |d| st.execute_command(d, player, &sel, p));
    }

    pub fn command(&mut self, f: impl FnOnce(&mut bw::State, &bw::Data, u8, &[u32])) {
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        with_data(self.a, |d| f(st, d, me, &sel));
    }
}

/// What 0x4e0710 hands the game: the glue's game setup (the slot table 0x57eee0 after the
/// races are dealt and the players placed, the template 0x596865, the colors 0x57f21c).
pub struct GameSetup {
    /// staredit\\scenario.chk of the map.
    pub chk: Vec<u8>,
    /// The map archive (its sounds), moved to the assets by `prepare_setup`.
    pub mpq: Option<crate::mpq::Mpq>,
    pub types: [u8; 12],
    pub races: [u8; 12],
    pub names: [String; 12],
    pub colors: [u8; 8],
    pub template: [u8; 0x20],
    /// The local player's slot.
    pub local: u8,
    /// 0x51ca10: the game seed.
    pub seed: u32,
}

/// Engine state for a new game on `chk` (map load, players, melee starting units), or
/// for the game replay header `rep` recorded.
fn new_game(a: &Assets, chk: &Chk, rep: Option<&crate::replay::Header>, gs: Option<&GameSetup>) -> io::Result<bw::State> {
    let (w, h) = chk.dims();
    let mut st = bw::State::new((w, h), chk.tileset() as u16, 1700);
    let name = map::TILESETS[chk.tileset()];
    let vf4 = a.files.read(&format!("tileset/{name}.vf4"))?;
    let ts = bw::map::TilesetData { vf4: vf4.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(), cv5: a.files.read(&format!("tileset/{name}.cv5"))? };
    #[cfg(not(target_arch = "wasm32"))]
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32);
    #[cfg(target_arch = "wasm32")] // std has no clock there
    let secs = (js_sys::Date::now() / 1000.0) as u32;
    // Debug: SCBW_SEED fixes the seed (reproducible runs).
    st.rng.seed = std::env::var("SCBW_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(secs);
    with_data(a, |d| {
        st.search.init(d);
        st.init_tech_defaults(d);
        st.init_acquisition_ranges(d);
        st.init_psi(&a.bw.psi_grp);
        st.init_thingies(d);
        st.load_terrain(&ts, w as u16, h as u16, &chk.mtxm());
        let melee = rep.is_none() && gs.is_none() && is_melee_map(chk) && std::env::var("SCBW_NOMELEE").is_err();
        st.template = game_template(a, chk, rep, gs);
        let modes = bw::chk_load::ScenarioModes::of_template(&st.template);
        let units = chk.section(b"UNIT").unwrap_or(&[]); // start locations: the last UNIT handler call sets them
        if let Some(h) = rep {
            // 0x4ef100: a replay's game uses the seed in its header (0x6d0f38).
            st.rng.seed = h.start_time;
            // The header's player table is 0x57eee0 as it was when the game started.
            for (p, s) in h.slots.iter().enumerate().take(8) {
                st.player_types[p] = s.kind;
                st.player_races[p] = s.race;
                st.player_names[p] = s.name.clone();
                // Melee starting resources go to all eight players, empty slots too (as
                // the real game's memory shows; todo: the routine applying game settings).
                st.players.minerals[p] = 50;
            }
            st.replay = true;
            // 0x4ef100: 0x59bda8 from the header (0x6d11a1).
            st.melee_map_units.copy_from_slice(&h.raw[0x271..0x279]);
            // Header byte 0: 1 = Brood War (0x58f440).
            st.is_expansion = h.engine == 1;
            st.local_player = h.slots.iter().position(|s| s.kind == 2).unwrap_or(0) as u8;
        } else if let Some(g) = gs {
            // 0x4ef100 / 0x4eee00 read the slot table 0x57eee0 the glue filled (races dealt by
            // 0x4a9a30, types by 0x4a9d80).
            st.rng.seed = g.seed;
            for p in 0..12 {
                st.player_types[p] = g.types[p];
                st.player_races[p] = g.races[p];
                st.player_names[p] = g.names[p].clone();
            }
            if modes.flag_59686d || modes.flag_596871 || modes.flag_596877 {
                // As for a replay: the melee starting resources go to all eight players.
                st.players.minerals[..8].fill(50);
            }
            st.local_player = g.local;
        } else if melee {
            // ponytail: local player is slot 0 playing SCBW_RACE (default terran); other
            // start locations get idle computer slots cycling the races (no AI yet).
            let starts: Vec<u8> = units.chunks_exact(36).filter(|r| u16::from_le_bytes([r[8], r[9]]) == 0xd6 && r[16] < 8).map(|r| r[16]).collect();
            let mine = match std::env::var("SCBW_RACE").as_deref() {
                Ok("zerg") => 0,
                Ok("protoss") => 2,
                _ => 1,
            };
            for &p in &starts {
                st.player_types[p as usize] = if p == 0 { 2 } else { 1 };
                st.player_races[p as usize] = if p == 0 { mine } else { p % 3 };
                st.players.minerals[p as usize] = 50;
            }
            st.local_player = 0;
        } else {
            let (owners, races) = (chk.owners(), chk.races());
            for p in 0..12 {
                st.player_types[p] = match owners[p] {
                    6 => 2,
                    5 => 1,
                    o => o,
                };
                st.player_races[p] = races[p].min(2);
            }
            st.local_player = owners.iter().position(|&o| o == 6).unwrap_or(0) as u8;
        }
        st.local_vision_mask = match rep {
            // Watching a replay shows every player's vision.
            Some(h) => h.slots.iter().take(8).enumerate().filter(|(_, s)| s.kind == 2 || s.kind == 1).fold(0, |m, (p, _)| m | 1 << p),
            None => 1 << st.local_player,
        };
        st.init_alliances(modes);
        // 0x4eed10 -> 0x49b870: the selection circle colors.
        st.selection_circle_colors(d);
        // 0x4eee00: the AI's player data (0x447680) before the scenario loads.
        st.init_player_ai();
        // 0x4eec30 -> 0x489b30: trigger state for a new game (0x489790: who is in it).
        st.init_trigger_players();
        // 0x4ef100: srand(0x51ca10, the game seed) for the CRT rand creep tiles use; then
        // 0x4ee2d0 draws once for the music variant 0x51a3b4 (outside campaigns, in the
        // process's first game: 0x51a3b4 starts at -1). Games here are never campaign
        // missions (0x57f244 = 0).
        st.crt_seed = st.rng.seed;
        st.crt_rand();
        // 0x4eee00 loads the scenario right after 0x4ef100 seeds the RNG, with it on.
        // Handler order (table 0x5001e8): MTXM, THG2 (its unit doodads draw from the
        // RNG too), UNIT.
        st.rng.enabled = true;
        for doodads in chk.sections(b"THG2") {
            st.chk_doodads(d, doodads, Default::default());
        }
        if !(modes.flag_59686d || modes.flag_596871 || modes.flag_596877) {
            for b in chk.sections(b"MASK") {
                st.chk_mask(b);
            }
            for b in chk.sections(b"PUNI") {
                st.chk_puni(b);
            }
            for b in chk.sections(b"PUPx") {
                st.chk_pupx(b);
            }
            for b in chk.sections(b"PTEx") {
                st.chk_ptex(b);
            }
        }
        for units in chk.sections(b"UNIT") {
            st.chk_units(d, units, modes);
        }
        let melee_sections = modes.flag_59686d || modes.flag_596871 || modes.flag_596877;
        if !melee_sections {
            // The rest of the Use Map Settings list (0x5004a8; UNIx / UPGx / TECx went
            // to the dat tables in apply_map_dat).
            for b in chk.sections(b"UPRP") {
                st.chk_uprp(b);
            }
            for m in chk.sections(b"MRGN") {
                st.load_locations(m);
            }
            for t in chk.sections(b"TRIG") {
                st.load_triggers(t);
            }
        }
        st.rng.enabled = false;
        // 0x4bf520: 0x49da40 starting units (team melee with shared bases 0x49d8e0 is
        // not ported).
        st.rng.enabled = true;
        st.melee_start_units(d, st.template[0xc]);
        st.rng.enabled = false;
        // 0x4eee00 tail: 0x437a70(1) AI regions for the map's region count.
        st.init_ai_regions();
        st.init_resource_areas(d);
        // 0x4ef100 -> 0x4abaf0: the game type's computer scripts and own triggers.
        st.start_game_type_ai(d);
        if let Some(name) = bw::triggers::game_type_triggers(&st.template) {
            if let Ok(t) = a.files.read(name) {
                st.load_triggers(&t);
            }
        }
    });
    Ok(st)
}

/// 0x596865: the game's template (a replay keeps its game type and sub type; a map
/// played here is Melee, or Use Map Settings when it is not a melee map).
fn game_template(a: &Assets, chk: &Chk, rep: Option<&crate::replay::Header>, gs: Option<&GameSetup>) -> [u8; 0x20] {
    if let Some(g) = gs {
        return g.template;
    }
    let melee = rep.is_none() && is_melee_map(chk) && std::env::var("SCBW_NOMELEE").is_err();
    let (gt, sub) = match rep {
        Some(h) => (h.game_type, h.sub_type),
        None if melee => (2, 1),
        None => (10, 1),
    };
    load_template(a, gt, sub)
}

/// Use Map Settings games: UNIx / UPGx / TECx rewrite the dat tables when the scenario
/// loads (0x5004a8 handler list).
pub fn prepare_map(a: &mut Assets, chk: &Chk) {
    apply_map_dat(a, chk, None, None);
}

/// `prepare_map` for a game the glue set up; the map archive goes to the assets.
pub fn prepare_setup(a: &mut Assets, g: &mut GameSetup) {
    apply_map_dat(a, &Chk::parse(&g.chk), None, Some(g));
    a.map_mpq = g.mpq.take();
}

pub fn prepare_replay(a: &mut Assets, r: &crate::replay::Replay) {
    apply_map_dat(a, &Chk::parse(&r.chk), Some(&r.header), None);
}

fn apply_map_dat(a: &mut Assets, chk: &Chk, rep: Option<&crate::replay::Header>, gs: Option<&GameSetup>) {
    a.bw.dat.map_tbl = chk.section(b"STR ").map(|s| s.to_vec());
    let m = bw::chk_load::ScenarioModes::of_template(&game_template(a, chk, rep, gs));
    if m.flag_59686d || m.flag_596871 || m.flag_596877 {
        return;
    }
    for b in chk.sections(b"UNIx") {
        bw::chk_load::apply_dat_settings(&mut a.bw.dat, Some(b), None, None);
    }
    for b in chk.sections(b"UPGx") {
        bw::chk_load::apply_dat_settings(&mut a.bw.dat, None, Some(b), None);
    }
    for b in chk.sections(b"TECx") {
        bw::chk_load::apply_dat_settings(&mut a.bw.dat, None, None, Some(b));
    }
}

/// 0x4aaea0: the template (Templates\\*.got, listed in templates.lst) of a game type
/// and sub type; game type 0xff when there is none.
pub fn load_template(a: &Assets, game_type: u16, sub_type: u16) -> [u8; 0x20] {
    let mut t = [0u8; 0x20];
    t[0] = 0xff;
    let Ok(list) = a.files.read("Templates\\templates.lst") else { return t };
    for name in String::from_utf8_lossy(&list).lines() {
        let name = name.trim().trim_matches('"');
        let Ok(f) = a.files.read(&format!("Templates\\{name}")) else { continue };
        if f.len() < 0x61 || f[0] != 3 {
            continue;
        }
        let g = &f[0x41..0x61];
        if u16::from_le_bytes([g[0], g[1]]) == game_type && u16::from_le_bytes([g[2], g[3]]) == sub_type {
            t.copy_from_slice(g);
            return t;
        }
    }
    t
}

/// 0x42d4c0: draw-order key: elevation, then y (elevation < 5 only), sprite flag 0x10,
/// then the sprite's pool index (+0x10).
fn sprite_draw_key(sp: &bw::sprite::Sprite, index: u32) -> u32 {
    let e = sp.elevation as u32;
    let y = if e < 5 { sp.y as i32 as u32 } else { 0 };
    ((((e << 13) | y) << 5 | (sp.flags & 0x10) as u32) << 9) | (index & 0xffff)
}

/// 0x42d400: sift the last entry of the 1-based max-heap `h` up.
fn heap_sift_up(h: &mut [(u32, u32)]) {
    let e = h[h.len() - 1];
    let mut i = h.len() - 1;
    while i > 1 {
        let p = i >> 1;
        if e.0 <= h[p].0 {
            break;
        }
        h[i] = h[p];
        i = p;
    }
    h[i] = e;
}

/// The draw order 0x498d40 gets from the sprite heap: entries pushed in row-list order
/// (0x42d4c0 / 0x42d400, max-heap), popped largest first (0x42d460 / 0x42d370) into
/// 0x6c2318 and drawn from the last popped back to the first.
fn heap_draw_order(entries: Vec<(u32, u32)>) -> Vec<u32> {
    // 1-based heap as at 0x6c4a30.
    let mut h: Vec<(u32, u32)> = vec![(0, 0)];
    for e in entries {
        h.push(e);
        heap_sift_up(&mut h);
    }
    let mut popped = Vec::with_capacity(h.len());
    let mut n = h.len() - 1;
    while n != 0 {
        popped.push(h[1].1);
        let moved = h[n];
        n -= 1;
        let half = n >> 1;
        let mut i = 1;
        if half != 0 {
            loop {
                let mut c = i * 2;
                if c < n && h[c].0 < h[c + 1].0 {
                    c += 1;
                }
                if h[c].0 <= moved.0 {
                    break;
                }
                h[i] = h[c];
                i = c;
                if c > half {
                    break;
                }
            }
        }
        h[i] = moved;
    }
    popped.reverse();
    popped
}

pub const SCREEN_W: i32 = 640;
pub const SCREEN_H: i32 = 480;
const MINIMAP: (i32, i32, i32) = (6, 348, 128); // x, y, size
/// A grp frame blitted without its offsets (0x40abbe) at (x, y), at most cw x ch,
/// through `colors`.
#[allow(clippy::too_many_arguments)]
fn blit_frame(g: &crate::grp::Grp, frame: usize, x: i32, y: i32, cw: i32, ch: i32, colors: Option<&[u8; 256]>, out: &mut [u8], w: i32, h: i32) {
    let Some(f) = g.frame(frame) else { return };
    let mut tmp = vec![0u8; (g.w * g.h) as usize];
    g.draw(frame, false, -f.x, -f.y, &mut tmp, g.w, g.h, Blend::Normal);
    for yy in 0..f.h.min(ch) {
        for xx in 0..f.w.min(cw) {
            let c = tmp[(yy * g.w + xx) as usize];
            if c != 0 && (0..w).contains(&(x + xx)) && (0..h).contains(&(y + yy)) {
                out[((y + yy) * w + x + xx) as usize] = colors.map_or(c, |t| t[c as usize]);
            }
        }
    }
}

/// statdata.cpp's icon buttons: 0x456d30 (the race's cmdbtns.grp frame `gr`, centered,
/// clipped to the control) and 0x456c30 (the icon centered, through ticon.pcx `row`);
/// 0x4574e0 (info buttons) draws the icon first, 0x457480 the frame first.
#[allow(clippy::too_many_arguments)]
fn stat_button(ui: &crate::ui::Ui, gr: u16, icons: &crate::grp::Grp, icon: u16, row: usize, icon_first: bool, r: (i32, i32, i32, i32), out: &mut [u8]) {
    let (w, h) = (SCREEN_W, SCREEN_H);
    let (l, t, rr, b) = r;
    let (cw, ch) = (rr - l + 1, b - t + 1);
    let mut colors = [0u8; 256];
    for (i, v) in colors.iter_mut().enumerate() {
        *v = ui.ticon.get(row * 16 + (i & 15)).copied().unwrap_or(i as u8);
    }
    let draw_icon = |out: &mut [u8]| {
        if let Some(f) = icons.frame(icon as usize) {
            blit_frame(icons, icon as usize, l + (cw - f.w) / 2, t + (ch - f.h) / 2, f.w, f.h, Some(&colors), out, w, h);
        }
    };
    let draw_bg = |out: &mut [u8]| {
        if let Some(f) = ui.cmdbtns.frame(gr as usize) {
            let (fw, fh) = (f.w.min(cw), f.h.min(ch));
            blit_frame(&ui.cmdbtns, gr as usize, l + (cw - fw) / 2, t + (ch - fh) / 2, fw, fh, None, out, w, h);
        }
    };
    if icon_first {
        draw_icon(out);
        draw_bg(out);
    } else {
        draw_bg(out);
        draw_icon(out);
    }
}

/// Screen rect of command card position `pos` (1..=9): statbtn.bin's buttons 1..9.
pub fn card_rect(ui: &crate::ui::Ui, pos: u16) -> (i32, i32, i32, i32) {
    ui.statbtn.screen_rect(pos as i16).unwrap_or((0, 0, -1, -1))
}

/// 0x458900: a command button: the race's cmdbtns.grp frame (1 pressed, else 0; not
/// while watching a replay), then the icon centered, through the ticon.pcx row for
/// its state (1 disabled, 2 pressed or the highlighted icon 0x68c1bc, else 0).
/// Frames are blitted without their offsets (0x40abbe).
#[allow(clippy::too_many_arguments)]
fn draw_cmd_button(ui: &crate::ui::Ui, replay: bool, icon: u16, disabled: bool, pressed: bool, lit: bool, r: (i32, i32, i32, i32), out: &mut [u8], w: i32, h: i32) {
    let (l, t, rr, b) = r;
    let (cw, ch) = (rr - l + 1, b - t + 1);
    let p = pressed as i32;
    if let Some(f) = ui.cmdbtns.frame(p as usize).filter(|_| !replay) {
        let mut tmp = vec![0u8; (ui.cmdbtns.w * ui.cmdbtns.h) as usize];
        ui.cmdbtns.draw(p as usize, false, -f.x, -f.y, &mut tmp, ui.cmdbtns.w, ui.cmdbtns.h, Blend::Normal);
        for y in 0..f.h.min(ch) {
            for x in 0..f.w.min(cw) {
                let c = tmp[(y * ui.cmdbtns.w + x) as usize];
                if c != 0 && (0..w).contains(&(l + x)) && (0..h).contains(&(t + y)) {
                    out[((t + y) * w + l + x) as usize] = c;
                }
            }
        }
    }
    let state = if disabled { 1 } else if pressed || lit { 2 } else { 0 };
    let Some(f) = ui.cmdicons.frame(icon as usize) else { return };
    let (dx, dy) = match (replay, icon) {
        (true, 0x182 | 0x183) => (-2, 3),
        (true, 0x184) => (-4, 3),
        (true, 0x185) => (-3, 3),
        _ => (0, 0),
    };
    let x = (cw - f.w) / 2 + l + dx + p;
    let y = (ch - f.h) / 2 + t + dy + p;
    ui.draw_cmd_icon(icon as usize, state, x - f.x, y - f.y, out, w, h);
}

/// Full frame: map view + console + minimap + resource bar, as 0xRRGGBB.
pub fn compose(scene: &Scene, ui: Option<&crate::ui::Ui>, vx: i32, vy: i32, idx: &mut [u8], out: &mut [u32]) {
    compose_with_cursor(scene, ui, vx, vy, None, None, idx, out)
}

/// Minimap geometry: (origin x, origin y, map pixels per minimap pixel). 0x4a4400
/// centers a smaller image in the 128x128 control.
pub fn minimap_geom(scene: &Scene) -> (i32, i32, i32) {
    let (mx, my, ms) = MINIMAP;
    let m = &scene.minimap;
    (mx + (ms - m.w as i32) / 2, my + (ms - m.h as i32) / 2, m.tile_px as i32)
}

fn rect(out: &mut [u8], w: i32, h: i32, (x0, y0, x1, y1): (i32, i32, i32, i32), col: u8) {
    for x in x0.min(x1)..=x0.max(x1) {
        for y in [y0, y1] {
            if (0..w).contains(&x) && (0..h).contains(&y) {
                out[(y * w + x) as usize] = col;
            }
        }
    }
    for y in y0.min(y1)..=y0.max(y1) {
        for x in [x0, x1] {
            if (0..w).contains(&x) && (0..h).contains(&y) {
                out[(y * w + x) as usize] = col;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn compose_with_cursor(
    scene: &Scene,
    ui: Option<&crate::ui::Ui>,
    vx: i32,
    vy: i32,
    cursor: Option<(usize, usize, i32, i32)>,
    menu: Option<&crate::menu::Menu>,
    idx: &mut [u8],
    out: &mut [u32],
) {
    let (w, h) = (SCREEN_W, SCREEN_H);
    scene.draw(vx, vy, w, h, idx);
    // The pause command's grey (0x41e930), then the options' gamma (0x41e870), which
    // every palette shown goes through.
    let pal = if scene.st.palette_grey { crate::palette::grey(scene.palette()) } else { *scene.palette() };
    let pal = crate::palette::gamma(&pal, scene.gamma);
    if let Some(ui) = ui {
        draw_hud(scene, ui, vx, vy, cursor, menu, idx);
    }
    for (o, &i) in out.iter_mut().zip(idx.iter()) {
        let [r, g, b] = pal[i as usize];
        *o = (r as u32) << 16 | (g as u32) << 8 | b as u32;
    }
}

/// The interface over the map view, in the game palette.
fn draw_hud(scene: &Scene, ui: &crate::ui::Ui, vx: i32, vy: i32, cursor: Option<(usize, usize, i32, i32)>, menu: Option<&crate::menu::Menu>, out: &mut [u8]) {
    let (w, h) = (SCREEN_W, SCREEN_H);
    ui.skin.colors.set(scene.minimap.colors);
    if let (Some((_, kind)), Some((_, _, cx, cy))) = (scene.placing, cursor) {
        // Placement boxes are screen layers 3 / 4 (0x48d7f0), drawn into the game screen
        // under the console dialog. 0x48ddc0: the boxes at the tile under the cursor (0x48d660); an addon's parent
        // building (the active unit's type, slot 0) sits the addon offset (0x662538) back.
        if let Some((_, pos)) = Scene::placement_cursor((cx, cy), (vx, vy)) {
            placebox(scene, 1, kind, pos, out, w, h);
            let u = scene.client_selection().active;
            let dat = scene.a.dat();
            if dat.units.flags[kind as usize] & 2 != 0 && u != NIL {
                let v = dat.units.addon_position[kind as usize];
                placebox(scene, 0, scene.st.units[u as usize].unit_type, (pos.0 - v as u16 as i16 as i32, pos.1 - (v >> 16) as u16 as i16 as i32), out, w, h);
            }
        }
    }
    crate::ui::blit(&ui.console, 0, h - ui.console.h as i32, out, w, h);
    let (ox, oy, scale) = minimap_geom(scene);
    let mm = &scene.minimap;
    for y in 0..mm.h as i32 {
        for x in 0..mm.w as i32 {
            out[((oy + y) * w + ox + x) as usize] = mm.pixels[(y * mm.w as i32 + x) as usize];
        }
    }
    let (rx, ry, rw, rh) = (ox + vx / scale, oy + vy / scale, w / scale, (h - 120) / scale);
    rect(out, w, h, (rx, ry, rx + rw, ry + rh), scene.minimap.colors[0]);
    draw_resources(scene, ui, out);
    if let Some((l, t, r, b)) = ui.statport.screen_rect(1) {
        // 0x45e650: the video, then conover.pcx over it (index 0 clear).
        // ponytail: the static shown while a new portrait loads (0x45e570) is not ported.
        scene.portrait.borrow_mut().draw(scene.palette(), l, t, r - l + 1, b - t + 1, out, w, h);
        if let Some(c) = &ui.conover {
            crate::ui::blit(c, l, t, out, w, h);
        }
    }
    if scene.st.replay {
        draw_replay_panel(scene, ui, out, w, h);
    }
    // rez\stat_f10.bin: the MENU button (0x4f5300); rez\minimap.bin's buttons.
    ui.skin.draw(&ui.stat_f10, &mut crate::dlg::Surf { px: out, w, h });
    ui.skin.draw(&ui.minimap, &mut crate::dlg::Surf { px: out, w, h });
    // Building panels of statdata.cpp (training queue, research, upgrade).
    if let Some((_, ctls)) = scene.status_panel(ui) {
        let sd = &ui.statdata;
        for (id, show) in &ctls {
            let Some((l, t, r, b)) = sd.screen_rect(*id) else { continue };
            let (cw, ch) = (r - l + 1, b - t + 1);
            match show {
                crate::statdata::Show::Text(s) => {
                    let Some(c) = sd.get(*id) else { continue };
                    let mut c = c.clone();
                    c.text = s.as_bytes().to_vec();
                    ui.skin.draw_ctl(&c, (l, t, r, b), &mut crate::dlg::Surf { px: out, w, h });
                }
                crate::statdata::Show::Wireframe => {
                    let [u] = scene.st.selected[..] else { continue };
                    // 0x4568f0: the frame is the shown type's (0 past the grp's frames).
                    let wt = crate::ui::wire_type(&scene.st, u) as usize;
                    let frame = if wt < ui.wireframe.frame_count() { wt } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, u, &ui.twire));
                    ui.wireframe.draw(frame, false, l, t, out, w, h, Blend::Colors(&colors));
                }
                crate::statdata::Show::Cargo { bg, unit } => {
                    ui.cmdbtns.draw(*bg as usize, false, l, t, out, w, h, Blend::Normal);
                    let wt = crate::ui::wire_type(&scene.st, *unit) as usize;
                    let frame = if wt < ui.tranwire.frame_count() { wt } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, *unit, &ui.twire));
                    ui.tranwire.draw(frame, false, l, t, out, w, h, Blend::Colors(&colors));
                }
                crate::statdata::Show::GroupWire(u) => {
                    // 0x456f50: the button frame (0x1f hallucination, 0x13 hero, 0x19
                    // parasited, else 0xd; 0x456d30), then the unit's grpwire.grp frame
                    // (its type, 0x425960) centered, in its wireframe colors.
                    let un = &scene.st.units[*u as usize];
                    let flags = scene.a.dat().units.flags[un.unit_type as usize];
                    let gr = if un.status & 0x4000_0000 != 0 {
                        0x1f
                    } else if flags & 0x40 != 0 {
                        0x13
                    } else if un.parasite_flags != 0 {
                        0x19
                    } else {
                        0xd
                    };
                    if let Some(f) = ui.cmdbtns.frame(gr) {
                        let (fw, fh) = (f.w.min(cw), f.h.min(ch));
                        blit_frame(&ui.cmdbtns, gr, l + (cw - fw) / 2, t + (ch - fh) / 2, fw, fh, None, out, w, h);
                    }
                    let frame = un.unit_type as usize;
                    let frame = if frame < ui.grpwire.frame_count() { frame } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, *u, &ui.twire));
                    if let Some(f) = ui.grpwire.frame(frame) {
                        blit_frame(&ui.grpwire, frame, l + (cw - f.w) / 2, t + (ch - f.h) / 2, f.w, f.h, Some(&colors), out, w, h);
                    }
                }
                crate::statdata::Show::Info { icon, level, .. } => {
                    // 0x425600: graphic 0xc, disabled (ticon row 4); drawn by 0x4574e0.
                    stat_button(ui, 0xc, &ui.cmdicons, *icon, 4, true, (l, t, r, b), out);
                    let o = crate::ui::TextOpts { flags: 0x44, clip: (l, t, r, b), ..crate::ui::TextOpts::new() };
                    ui.skin.fonts[0].draw_text(&format!("{level} "), l, t, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out, w, h });
                }
                crate::statdata::Show::Progress(v) => {
                    // 0x456b40: the empty bar, then the full one clipped to the percentage.
                    crate::ui::blit(&ui.pbar_empty, l, t, out, w, h);
                    let _ = ch;
                    crate::ui::blit_w(&ui.pbar_full, l, t, cw * *v as i32 / 100, out, w, h);
                }
                crate::statdata::Show::Slot { bg, icon, cmdicon, num } => {
                    // 0x456c30: ticon row 3 for an enabled slot, 4 for a disabled one (all
                    // five are type 2); 0x4268d0 enables queued slots (0x418e00) and
                    // disables empty ones (0x418640). 0x4268d0 gives a queued unit its number
                    // ("1 " for the first slot, else "\x03N\x01 "), drawn as for 0x4574e0.
                    let g = if *cmdicon { &ui.cmdicons } else { &ui.cmdbtns };
                    stat_button(ui, *bg, g, *icon, if *num != 0 { 3 } else { 4 }, false, (l, t, r, b), out);
                    if *num != 0 {
                        let s = if *num == 1 { "1 ".to_string() } else { format!("\x03{num}\x01 ") };
                        let o = crate::ui::TextOpts { flags: 0x44, clip: (l, t, r, b), ..crate::ui::TextOpts::new() };
                        ui.skin.fonts[0].draw_text(&s, l, t, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out, w, h });
                    }
                }
                crate::statdata::Show::Button { bg, icon, cmdicon } => {
                    // 0x457480: an enabled button (ticon row 3).
                    let g = if *cmdicon { &ui.cmdicons } else { &ui.cmdbtns };
                    stat_button(ui, *bg, g, *icon, 3, false, (l, t, r, b), out);
                }
            }
        }
    }
    // 0x4576a0: an info button's tooltip under the cursor.
    if let (Some((_, ctls)), Some((_, _, cx, cy))) = (scene.status_panel(ui), cursor) {
        for (id, show) in &ctls {
            let crate::statdata::Show::Info { tip, .. } = show else { continue };
            let Some((l, t, r, b)) = ui.statdata.screen_rect(*id) else { continue };
            if (l..=r).contains(&cx) && (t..=b).contains(&cy) {
                tooltip(ui, tip, r, t, out, w, h);
            }
        }
    }
    scene.msgs.draw(&ui.font, crate::ui::TextDst { rows: &ui.text_colors, out, w, h });
    scene.chat.draw(&ui.skin, scene.clock_ms, scene.st.replay, out, w, h);
    if let Some(b) = scene.drag_box {
        rect(out, w, h, b, scene.minimap.colors[0x12]);
    }
    let lit = scene.card_highlight();
    for (i, slot) in scene.card().iter().enumerate() {
        if let Some((b, enabled)) = slot {
            draw_cmd_button(ui, scene.st.replay, b.icon, !*enabled, false, lit == Some(b.icon), card_rect(ui, i as u16 + 1), out, w, h);
        }
    }
    // 0x4593a0: the hovered button's tooltip.
    if let Some((_, _, cx, cy)) = cursor {
        for (i, slot) in scene.card().iter().enumerate() {
            let Some((b, enabled)) = slot else { continue };
            let (bx, by, br, bb) = card_rect(ui, i as u16 + 1);
            if (bx..=br).contains(&cx) && (by..=bb).contains(&cy) {
                draw_tooltip(scene, ui, b, *enabled, bx, by, out, w, h);
            }
        }
    }
    // 0x4a4c40: the terrain button's tooltip (stat_txt 0x32a shown, 0x329 hidden).
    if let (Some((_, _, cx, cy)), Some((l, t, r, b))) = (cursor, ui.minimap.get(2).filter(|c| c.flags & crate::dlg::flag::VISIBLE != 0).and(ui.minimap.screen_rect(2))) {
        if (l..=r).contains(&cx) && (t..=b).contains(&cy) {
            let i = if scene.minimap.hide_terrain { 0x329 } else { 0x32a };
            let s = ui.stat_txt.get(i).map_or("", |s| s.split('\0').next().unwrap_or(""));
            tooltip(ui, s, r, t, out, w, h);
        }
    }
    // 0x4f4f70: the MENU button's tooltip (stat_txt 0x32d) right of it.
    if let (Some((_, _, cx, cy)), Some((l, t, r, b))) = (cursor, ui.stat_f10.screen_rect(1)) {
        if (l..=r).contains(&cx) && (t..=b).contains(&cy) {
            let s = ui.stat_txt.get(0x32d).map_or("", |s| s.split('\0').next().unwrap_or(""));
            tooltip(ui, s, r, t, out, w, h);
        }
    }
    if let Some(m) = menu {
        m.draw(&ui.skin, &mut crate::dlg::Surf { px: out, w, h });
    }
    if let Some((kind, counter, cx, cy)) = cursor {
        ui.draw_cursor(kind, counter, cx, cy, out, w, h);
    }
}

/// 0x4593a0 / 0x459150 / 0x459030: a command button's tooltip: its stat_txt string
/// (after the hotkey and kind bytes) and, for enabled buttons, the cost: minerals, gas
/// and supply for units (kind 1), the current level's cost and the next level for
/// upgrades (2), minerals, gas and energy for techs (3).
#[allow(clippy::too_many_arguments)]
fn draw_tooltip(scene: &Scene, ui: &crate::ui::Ui, b: &bw::buttons::Button, enabled: bool, bx: i32, by: i32, out: &mut [u8], w: i32, h: i32) {
    let stat = |id: u16| (id as usize).checked_sub(1).and_then(|i| ui.stat_txt.get(i)).map(|s| s.split('\0').next().unwrap_or(""));
    let Some((kind, btext)) = crate::dat::tbl_button(&ui.stat_raw, b.enabled_str) else { return };
    let me = scene.local_player();
    let dat = scene.a.dat();
    let v = b.act_var as usize;
    if !enabled {
        // The disabled string whole; an upgrade's is per level (0x4ce7a0).
        let lvl = if kind == 2 && v < dat.upgrades.icon.len() { scene.st.players.upgrade_level(me, v) as u16 } else { 0 };
        if b.disabled_str == 0 {
            return;
        }
        let Some(t) = stat(b.disabled_str + lvl) else { return };
        tooltip_with_costs(ui, t, &[0; 8], scene.local_race() as i32, bx, by, out, w, h);
        return;
    }
    let mut text = btext;
    // (minerals, gas, supply, energy)
    let mut cost = (0i32, 0i32, 0i32, 0i32);
    if enabled {
        match kind {
            1 if v < dat.units.mineral_cost.len() => {
                let two = dat.units.flags[v] & 0x400 != 0;
                let sup = dat.units.supply_required[v] as i32 * if two { 2 } else { 1 };
                cost = (dat.units.mineral_cost[v] as i32, dat.units.gas_cost[v] as i32, sup / 2, 0);
            }
            2 if v < dat.upgrades.icon.len() => {
                let (m, g, _) = with_data(scene.a, |d| scene.st.upgrade_cost(d, me, v as u8));
                cost = (m, g, 0, 0);
                if dat.upgrades.max_level[v] > 1 {
                    let lvl = scene.st.players.upgrade_level(me, v) as i32 + 1;
                    text = format!("{text}\n{} {lvl}", ui.stat_txt.get(0x31a).map_or("", |s| s.split('\0').next().unwrap_or("")));
                }
            }
            3 if v < dat.techdata.icon.len() => {
                cost = (dat.techdata.mineral_cost[v] as i32, dat.techdata.gas_cost[v] as i32, 0, dat.techdata.energy_cost[v] as i32);
            }
            _ => {}
        }
    }
    let race = scene.local_race() as i32;
    let caster = scene.st.selected.first().map_or(4, |&u| with_data(scene.a, |d| bw::State::supply_race_of(d, scene.st.units[u as usize].unit_type)));
    let supply_race = if kind == 1 && v < dat.units.mineral_cost.len() { with_data(scene.a, |d| bw::State::supply_race_of(d, v as u16)) } else { 4 };
    let slots = cost_slots(cost, supply_race, caster);
    tooltip_with_costs(ui, &text, &slots, race, bx, by, out, w, h);
}

/// 0x458800: the eight cost slots (icons 0x515228): minerals, gas, energy by the
/// casting unit's race (0x47b150: zerg 4, terran 3, protoss 2) and supply by the
/// unit's race (zerg 7, terran 6, protoss 5).
fn cost_slots(cost: (i32, i32, i32, i32), supply_race: u16, caster: u16) -> [i32; 8] {
    let mut slots = [0i32; 8];
    slots[0] = cost.0;
    slots[1] = cost.1;
    if let Some(&k) = [7usize, 6, 5].get(supply_race as usize).filter(|_| cost.2 != 0) {
        slots[k] = cost.2;
    }
    if let Some(&k) = [4usize, 3, 2].get(caster as usize).filter(|_| cost.3 != 0) {
        slots[k] = cost.3;
    }
    slots
}

/// 0x459030: tooltip text over interface color 0xf from (2, 0), then the cost line
/// (0x458ef0: each slot's icon at the text's bottom and its number after it, in
/// the color the text left), framed in color 0x10 at the buffer's origin (0x4e1c70)
/// and put above the hovered button at (bx, by) (0x458850).
// ponytail: the numbers use color 2, not the color the text's codes left.
#[allow(clippy::too_many_arguments)]
fn tooltip_with_costs(ui: &crate::ui::Ui, text: &str, slots: &[i32; 8], race: i32, bx: i32, by: i32, out: &mut [u8], w: i32, h: i32) {
    const ICONS: [i32; 8] = [0, 1, 9, 8, 7, 6, 5, 4];
    let colors = ui.skin.colors.get();
    let mut buf = vec![colors[0xf]; (w * h) as usize];
    let o = crate::ui::TextOpts { flags: 0x11, line_gap: 0, clip: (2, 0, w - 1, h - 1), ..crate::ui::TextOpts::new() };
    let font = &ui.skin.fonts[0];
    let mut e = font.draw_text(text, 2, 0, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out: &mut buf, w, h });
    let mut x = 2;
    let mut icon_h = 0;
    let mut any = false;
    for (k, &n) in slots.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let mut f = ICONS[k];
        if f == 1 {
            f = race + 1;
        }
        let f = if (f as usize) < ui.icons.frame_count() { f as usize } else { 0 };
        let (iw, ih) = ui.icons.frame_size(f);
        icon_h = ih;
        ui.draw_icon(f, x, e.b, &mut buf, w, h);
        let t = font.draw_text(&n.to_string(), x + iw + 2, e.b, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out: &mut buf, w, h });
        x = t.r;
        any = true;
    }
    if any {
        e.r = e.r.max(x);
        e.b += icon_h + 1;
    }
    let (bw, bh) = (e.r - e.l + 1, e.b - e.t + 1);
    let mut x0 = bx;
    if x0 + bw - 1 > 0x27f {
        x0 = 0x27f - (bw - 1);
    }
    let y0 = by - 1 - bh + 1;
    for yy in 0..bh {
        for xx in 0..bw {
            let edge = xx == 0 || yy == 0 || xx == bw - 1 || yy == bh - 1;
            let c = if edge { colors[0x10] } else { buf[(yy * w + xx) as usize] };
            if (0..w).contains(&(x0 + xx)) && (0..h).contains(&(y0 + yy)) {
                out[((y0 + yy) * w + x0 + xx) as usize] = c;
            }
        }
    }
}

/// 0x48daf0: build the placement box `slot` for unit type `t`.
/// 0x48d7f0: show it. 0x48d750 sizes the surface: the surface is
/// the larger of the type's first image frame (0x48da30, in the local player's color)
/// and its placement box, rounded up to whole tiles; the box shows with its tile
/// area at screen position `pos` (the box centred on the surface), each tile of its
/// result grid dithered (0x40c4d2) red (0x6ceb34) when not free, else green (0x6ceb33).
fn placebox(scene: &Scene, slot: usize, t: u16, pos: (i32, i32), out: &mut [u8], w: i32, h: i32) {
    let dat = scene.a.dat();
    let v = dat.units.placement_box[t as usize];
    let (pw, ph) = (v as u16 as i16 as i32, (v >> 16) as u16 as i16 as i32);
    let img = dat.sprites.image[dat.flingy.sprite[dat.units.flingy[t as usize] as usize] as usize];
    let (gw, gh, grp) = match scene.a.grp(img) {
        Some(g) => (g.w, g.h, Some(g)),
        None => (0, 0, None),
    };
    let (sw, sh) = ((gw.max(pw) + 31) & !31, (gh.max(ph) + 31) & !31);
    let (cols, rows) = (pw / 32, ph / 32);
    let (ox, oy) = ((sw - cols * 32) / 2, (sh - rows * 32) / 2);
    let mut surf = vec![0u8; (sw * sh) as usize];
    if let Some(g) = grp {
        g.draw(0, false, 0, 0, &mut surf, sw, sh, Blend::Player(&scene.player_colors[scene.local_player() as usize]));
    }
    for r in 0..rows.min(8) {
        for c in 0..cols.min(6) {
            let blocked = scene.st.placement[(r as usize + slot * 8) * 6 + c as usize] != 0;
            let col = scene.minimap.colors[if blocked { 0x14 } else { 0x13 }];
            for yy in 0..32 {
                for xx in (yy & 1..32).step_by(2) {
                    surf[((oy + r * 32 + yy) * sw + ox + c * 32 + xx) as usize] = col;
                }
            }
        }
    }
    let (sx, sy) = (pos.0 - ox, pos.1 - oy);
    for (i, &c) in surf.iter().enumerate() {
        let (px, py) = (sx + i as i32 % sw, sy + i as i32 / sw);
        if c != 0 && (0..w).contains(&px) && (0..h).contains(&py) {
            out[(py * w + px) as usize] = c;
        }
    }
}

fn on_minimap_pt(x: i32, y: i32) -> bool {
    (MINIMAP.0..MINIMAP.0 + MINIMAP.2).contains(&x) && (MINIMAP.1..MINIMAP.1 + MINIMAP.2).contains(&y)
}

fn key_char(k: minifb::Key) -> Option<char> {
    use minifb::Key::*;
    let keys = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    keys.iter().position(|&x| x == k).map(|i| (b'a' + i as u8) as char)
}

/// 0x418220: the hotkey of a button: the first byte of its stat_txt string, compared as
/// is against the lowercased typed character.
fn hotkey(ui: Option<&crate::ui::Ui>, b: &bw::buttons::Button) -> Option<char> {
    let s = ui?.stat_txt.get((b.enabled_str as usize).checked_sub(1)?)?;
    s.chars().next()
}

/// Where a replay viewer's camera looks for one player. 1.16.1 replays don't record
/// the camera, so it follows what the player does: the map point of their latest
/// positional command (right-click, targeted order, build, lift) or, after a
/// selection, the middle of what they selected.
pub struct Follow {
    pub player: u8,
    pub focus: Option<(i32, i32)>,
}

impl Follow {
    /// Update the focus after `player` sent packet `p` (already executed).
    pub fn command(&mut self, st: &bw::State, player: u8, p: &[u8]) {
        if player != self.player {
            return;
        }
        let u16_at = |o: usize| p.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) as i32;
        match p.first() {
            Some(0x14 | 0x15 | 0x2f) => self.focus = Some((u16_at(1), u16_at(3))),
            Some(0x0c) => self.focus = Some((u16_at(2) * 32 + 16, u16_at(4) * 32 + 16)),
            Some(0x09 | 0x0a | 0x0b) => {
                let sel: Vec<u32> = bw::select::selection_units(&st.selections[player as usize]).iter().copied().filter(|&u| u != NIL && st.units[u as usize].sprite != NIL).collect();
                if !sel.is_empty() {
                    let n = sel.len() as i32;
                    let (sx, sy) = sel.iter().fold((0, 0), |(x, y), &u| {
                        let s = &st.sprites[st.units[u as usize].sprite as usize];
                        (x + s.x as i32, y + s.y as i32)
                    });
                    self.focus = Some((sx / n, sy / n));
                }
            }
            _ => {}
        }
    }
}

/// A replay being played in the viewer: its commands and the camera that follows one
/// of its players.
pub struct ReplayPlay {
    pub rep: crate::replay::Replay,
    pub player_of: [u8; 256],
    pub players: Vec<u8>,
    pub next: usize,
    pub frame: u32,
    pub follow: Follow,
}

impl ReplayPlay {
    pub fn new(rep: crate::replay::Replay) -> ReplayPlay {
        let mut player_of = [0u8; 256];
        let mut players = Vec::new();
        for (i, s) in rep.header.slots.iter().enumerate() {
            if s.kind == 1 || s.kind == 2 {
                player_of[s.player_id as usize] = i as u8;
                players.push(i as u8);
            }
        }
        let first = std::env::var("SCBW_FOLLOW").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| players.first().copied().unwrap_or(0));
        ReplayPlay { rep, player_of, players, next: 0, frame: 0, follow: Follow { player: first, focus: None } }
    }

    /// Ease the camera (top-left of a `vw` x `vh` map view) a quarter of the way toward
    /// the followed player's focus; unchanged until they have one.
    pub fn camera_step(&self, cam: (i32, i32), vw: i32, vh: i32) -> (i32, i32) {
        let Some((fx, fy)) = self.follow.focus else { return cam };
        let (tx, ty) = (fx - vw / 2, fy - vh / 2);
        (cam.0 + (tx - cam.0) / 4, cam.1 + (ty - cam.1) / 4)
    }

    /// Run this frame's commands.
    pub fn commands(&mut self, scene: &mut Scene) {
        while self.next < self.rep.commands.len() && self.rep.commands[self.next].frame <= self.frame {
            let c = &self.rep.commands[self.next];
            let p = self.player_of[c.player as usize];
            scene.replay_command(p, &c.data);
            self.follow.command(&scene.st, p, &c.data);
            self.next += 1;
        }
        self.frame += 1;
    }
}

pub fn run(a: &mut Assets, chk: Chk) -> io::Result<()> {
    prepare_map(a, &chk);
    let a: &Assets = a;
    run_with(a, Scene::new(a, chk)?, None)
}

/// Play the game the glue menus set up (0x4e0710).
pub fn run_setup(a: &mut Assets, mut g: GameSetup) -> io::Result<()> {
    prepare_setup(a, &mut g);
    let a: &Assets = a;
    run_with(a, Scene::from_setup(a, &g)?, None)
}

/// Watch a replay; the camera follows a player (Tab: next player, F: follow on/off).
pub fn run_replay(a: &mut Assets, rep: crate::replay::Replay) -> io::Result<()> {
    prepare_replay(a, &rep);
    let a: &Assets = a;
    let scene = Scene::from_replay(a, &rep)?;
    run_with(a, scene, Some(ReplayPlay::new(rep)))
}

/// Show a 0RGB frame. minifb's web canvas takes the buffer's bytes as RGBA instead.
fn present(win: &mut minifb::Window, rgb: &[u32], w: usize, h: usize) -> io::Result<()> {
    #[cfg(target_arch = "wasm32")]
    let rgb = &rgb.iter().map(|&c| 0xff00_0000 | (c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)).collect::<Vec<u32>>();
    win.update_with_buffer(rgb, w, h).map_err(|e| io::Error::other(e.to_string()))
}

fn run_with(a: &Assets, scene: Scene, replay: Option<ReplayPlay>) -> io::Result<()> {
    let mut frame = frame_loop(a, scene, replay)?;
    while frame()? {}
    Ok(())
}

/// The game window's loop body: one call per displayed frame, Ok(false) once the window
/// closes or the player quits. The browser build calls it from requestAnimationFrame.
pub fn frame_loop<'a>(a: &'a Assets, mut scene: Scene<'a>, mut replay: Option<ReplayPlay>) -> io::Result<impl FnMut() -> io::Result<bool> + 'a> {
    use minifb::{Key, Scale, Window, WindowOptions};
    let mut following = replay.is_some();
    let mut replay_acc = 0u32;
    if let Some(rp) = replay.as_ref() {
        scene.local = rp.follow.player;
    }
    let race = ["zerg", "terran", "protoss"][scene.local_race()];
    let ui = crate::ui::Ui::load_with(&a.files, race, scene.st.replay).ok();
    let mut audio = crate::audio::Audio::open(&a.files);
    if let Some(au) = audio.as_mut() {
        au.fade_out(); // 0x4ef100 -> 0x4a5f50: the front-end music ends as the game starts
        au.play_music(&a.files, &format!("music/{race}1.wav"));
    }
    let (vw, vh) = (SCREEN_W, SCREEN_H);
    let opts = WindowOptions { scale: Scale::X2, ..WindowOptions::default() };
    let mut win = Window::new("scbw", vw as usize, vh as usize, opts).map_err(|e| io::Error::other(e.to_string()))?;
    // The window loop runs at the display rate: the software cursor is redrawn on every
    // pass (as 1.16.1's main loop does), the game clock below ticks every 42 ms.
    win.set_target_fps(60);
    // Typed characters for the chat line (WM_CHAR).
    struct Typed(std::rc::Rc<std::cell::RefCell<Vec<u32>>>);
    impl minifb::InputCallback for Typed {
        fn add_char(&mut self, c: u32) {
            self.0.borrow_mut().push(c);
        }
    }
    let typed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    win.set_input_callback(Box::new(Typed(typed.clone())));
    let (mw, mh) = (scene.terrain.0 as i32, scene.terrain.1 as i32);
    // Start with the view on the local player's start location.
    let start = scene.st.start_locations[scene.local_player() as usize];
    let (mut vx, mut vy) = (start.0 as i32 - vw / 2, start.1 as i32 - (vh - 120) / 2);
    let mut idx = vec![0u8; (vw * vh) as usize];
    let mut rgb = vec![0u32; (vw * vh) as usize];
    win.set_cursor_visibility(false);
    let mut tick = 0usize;
    let mut drag_start: Option<(i32, i32)> = None;
    let mut rmb_was_down = false;
    let mut lmb_was_down = false;
    let mut last_click = 0usize;
    // 0x57f270: the saved screen positions (Shift+F2..F4 save, F2..F4 recall).
    let mut camera_slots = [(0i32, 0i32); 3];
    // 0x5999b4: the music volume Ctrl+M turned off; the same for Ctrl+S.
    let (mut saved_music, mut saved_digital) = (0i16, 0i16);
    // A press used for a targeted order is not also a minimap scroll until released.
    let mut press_used = false;
    let mut hud_press: Option<bool> = None;
    // 0x597280 / 0x59727c: the hotkey group last selected and when (0x4c07b0).
    let mut last_group: (u8, usize) = (0xff, 0);
    let mut settings = crate::menu::Settings::default();
    let mut menu: Option<crate::menu::Menu> = None;
    let mut menu_paused = false;
    let mut game_acc = 0u32;
    let mut ctx = crate::menu::Ctx { replay: replay.is_some(), multiplayer: false, replay_speed: 0 };
    let mut menu_btn_down = false;
    let mut terrain_btn_down = false;
    // The next 42 ms tick of input and game logic (the browser build ticks per call).
    #[cfg(not(target_arch = "wasm32"))]
    let mut next_tick = std::time::Instant::now();
    // Input between ticks: the window is redrawn (and its events taken) at the display
    // rate, the game reads input every 42 ms. Key presses and button downs of the frames
    // in between are kept for the next tick (as Windows queues the messages).
    let mut pend_keys: Vec<Key> = Vec::new();
    let mut pend_rep: Vec<Key> = Vec::new();
    let (mut pend_l, mut pend_r) = (false, false);
    Ok(move || {
        if !win.is_open() {
            return Ok(false);
        }
        let (cx, cy) = win.get_mouse_pos(minifb::MouseMode::Clamp).map_or((vw / 2, vh / 2), |(x, y)| (x as i32, y as i32));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (now, dt) = (std::time::Instant::now(), std::time::Duration::from_millis(42));
            if now < next_tick {
                pend_keys.extend(win.get_keys_pressed(minifb::KeyRepeat::No));
                pend_rep.extend(win.get_keys_pressed(minifb::KeyRepeat::Yes));
                pend_l |= win.get_mouse_down(minifb::MouseButton::Left);
                pend_r |= win.get_mouse_down(minifb::MouseButton::Right);
                // Between ticks: only the cursor moves.
                let kind = if menu.is_some() { 0 } else { scene.cursor_kind(cx, cy, vx, vy, drag_start.is_some()) };
                scene.gamma = settings.gamma;
                compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((kind, tick * 42 / 100, cx, cy)), menu.as_ref(), &mut idx, &mut rgb);
                present(&mut win, &rgb, vw as usize, vh as usize)?;
                return Ok(true);
            }
            next_tick = if now > next_tick + dt { now + dt } else { next_tick + dt };
        }
        tick += 1;
        let mut keys_no: Vec<Key> = std::mem::take(&mut pend_keys);
        keys_no.extend(win.get_keys_pressed(minifb::KeyRepeat::No));
        let mut keys_rep: Vec<Key> = std::mem::take(&mut pend_rep);
        keys_rep.extend(win.get_keys_pressed(minifb::KeyRepeat::Yes));
        let lmb = win.get_mouse_down(minifb::MouseButton::Left) || std::mem::take(&mut pend_l);
        let rmb = win.get_mouse_down(minifb::MouseButton::Right) || std::mem::take(&mut pend_r);
        let shift = win.is_key_down(Key::LeftShift) || win.is_key_down(Key::RightShift);
        // The game menu (F10 or the console's MENU button) takes all input while open;
        // single player and replays pause under it (0x4f57a0 -> 0x488790).
        ctx.replay_speed = scene.st.replay_speed as i16;
        let open_menu = |scene: &Scene, kind, depth, settings: &crate::menu::Settings| {
            let u = ui.as_ref()?;
            let mut m = crate::menu::Menu::open(kind, depth, &a.files, &u.skin, ctx, settings)?;
            if kind == crate::menu::Kind::Objectives {
                m.set_text(2, &objectives_text(scene, u));
            }
            Some(m)
        };
        // 0x47e480 when the tips open.
        let open_menu = |scene: &Scene, kind, depth, settings: &mut crate::menu::Settings| {
            let mut m = open_menu(scene, kind, depth, settings)?;
            if let (crate::menu::Kind::Tips, Some(u)) = (kind, ui.as_ref()) {
                m.show_tip(&u.skin, &u.tips, settings, 0);
            }
            Some(m)
        };
        if menu.is_some() {
            typed.borrow_mut().clear();
            let ldown = lmb;
            let mut act = None;
            if let (Some(m), Some(u)) = (menu.as_mut(), ui.as_ref()) {
                act = m.mouse(&u.skin, cx, cy, ldown, &mut settings);
                for k in keys_no.clone() {
                    let ch = match k {
                        Key::Escape => Some(0x1b),
                        Key::Enter => Some(b'\r'),
                        k => key_char(k).map(|c| c as u8),
                    };
                    if let Some(id) = ch.and_then(|c| m.key(c, &mut settings)) {
                        act = Some(id);
                    }
                }
            }
            lmb_was_down = ldown;
            let tips_step = match (menu.as_mut(), ui.as_ref(), act) {
                (Some(m), Some(u), Some(id)) if m.kind == crate::menu::Kind::Tips => m.tips_button(&u.skin, &u.tips, id, &mut settings),
                _ => false,
            };
            if let Some(id) = act.filter(|_| !tips_step) {
                let m = menu.take().unwrap();
                let (next, action) = m.on_activate(id, &mut settings, ctx);
                menu = next.and_then(|(k, depth)| open_menu(&scene, k, depth, &mut settings));
                match action {
                    crate::menu::Action::QuitMission | crate::menu::Action::ExitProgram => return Ok(false),
                    // ponytail: restarting rebuilds nothing yet; the menu just closes.
                    _ => {}
                }
            }
            if let Some(au) = audio.as_mut() {
                au.set_volumes(settings.music, settings.digital);
            }
            scene.voices.sound_flags = settings.sound_flags;
            scene.voices.muted = settings.digital == 0;
            if menu.is_none() {
                scene.unpause_game(true);
            }
            if menu.is_none() && menu_paused {
                menu_paused = false;
                if replay.is_some() {
                    scene.st.replay_paused = false;
                }
            }
            let counter = tick * 42 / 100;
            scene.gamma = settings.gamma;
            compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((0, counter, cx, cy)), menu.as_ref(), &mut idx, &mut rgb);
            present(&mut win, &rgb, vw as usize, vh as usize)?;
            return Ok(true);
        }
        let mdown = lmb;
        let on_menu_btn = ui.as_ref().and_then(|u| u.stat_f10.screen_rect(1)).is_some_and(|(l, t, r, b)| (l..=r).contains(&cx) && (t..=b).contains(&cy));
        let menu_clicked = menu_btn_down && !mdown && on_menu_btn;
        menu_btn_down = mdown && (menu_btn_down || (on_menu_btn && !lmb_was_down));
        if menu_clicked || keys_no.contains(&Key::F10) {
            menu = open_menu(&scene, crate::menu::Kind::Game, 0, &mut settings);
            menu_paused = true;
            scene.pause_game();
            if replay.is_some() && !scene.st.replay_paused {
                scene.st.replay_paused = true;
            } else if replay.is_some() {
                menu_paused = false;
            }
            lmb_was_down = mdown;
            return Ok(true);
        }
        if menu_btn_down {
            lmb_was_down = mdown;
        }
        // rez\minimap.bin button 2 (0x4a54f0): terrain on / off, on release over it.
        let on_terrain_btn = ui.as_ref().and_then(|u| u.minimap.get(2).filter(|c| c.flags & crate::dlg::flag::VISIBLE != 0).and(u.minimap.screen_rect(2))).is_some_and(|(l, t, r, b)| (l..=r).contains(&cx) && (t..=b).contains(&cy));
        if terrain_btn_down && !mdown && on_terrain_btn {
            let Scene { a, st, minimap, .. } = &mut scene;
            with_data(a, |d| minimap.toggle_terrain(d, st));
        }
        terrain_btn_down = mdown && (terrain_btn_down || (on_terrain_btn && !lmb_was_down));
        if terrain_btn_down {
            lmb_was_down = mdown;
        }
        // ponytail: scroll speed 0x6cdfd8 / 0x6cdfdc picks among 7 rates; 8 + 4 per step.
        let sp = 8 + 4 * settings.mouse_scroll as i32;
        if cx <= 1 { vx -= sp }
        if cx >= vw - 2 { vx += sp }
        if cy <= 1 { vy -= sp }
        if cy >= vh - 2 { vy += sp }
        let (ox, oy, scale) = minimap_geom(&scene);
        let map_pt = (cx + vx, cy + vy);
        let down = lmb;
        let clicked = down && !lmb_was_down;
        lmb_was_down = down;
        // Command card: click a button or press its hotkey (enabled buttons only).
        let card = scene.card();
        let mut pressed: Option<bw::buttons::Button> = None;
        if clicked {
            pressed = card.iter().enumerate().find_map(|(i, slot)| {
                let (bx, by, br, bb) = ui.as_ref().map_or((0, 0, -1, -1), |u| card_rect(u, i as u16 + 1));
                ((bx..=br).contains(&cx) && (by..=bb).contains(&cy)).then_some(*slot).flatten()
            }).and_then(|(b, enabled)| enabled.then_some(b));
        }
        let ctrl = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
        let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
        let mut want_menu = None;
        // Chat line: Enter opens it (0x4f32d0), then the edit control (0x416980) takes the
        // keys; Enter submits and Esc cancels (0x4f36fa).
        let typed_chars: Vec<u32> = typed.borrow_mut().drain(..).collect();
        // 0x4d1d70 WM_CHAR -> 0x418510 -> 0x418220: the typed character, lowercased, is
        // matched against the first byte of each visible, enabled hotkey button of the
        // card (letters, digits, symbols; Ctrl makes control codes, Alt sends no WM_CHAR).
        // Not while the chat edit box has the keys.
        let hot_chars: Vec<u8> = if scene.chat.open || ctrl || alt { Vec::new() } else { typed_chars.iter().filter(|&&c| c < 0x80).map(|&c| (c as u8).to_ascii_lowercase()).collect() };
        for c in hot_chars {
            if let Some((b, _)) = card.iter().flatten().find(|(b, en)| *en && hotkey(ui.as_ref(), b) == Some(c as char)) {
                pressed = Some(*b);
            }
        }
        let mut chat_keys = Vec::new();
        if scene.chat.open {
            for c in typed_chars {
                scene.chat.char(c as u8);
            }
            for k in keys_rep.clone() {
                match k {
                    Key::Enter | Key::NumPadEnter => {
                        if let crate::chat::Done::Submit(t) = scene.chat.enter() {
                            // 0x4f32d0: the cheat check (single player), then 0x4c0400
                            // sends command 0x12 with the new options.
                            let mut opt = scene.st.game_options;
                            let on = scene.st.template[0x11] == 1;
                            if replay.is_none() && crate::chat::cheat(&t, &mut opt, true, on) && opt != scene.st.game_options {
                                let me = scene.local;
                                let mut p = vec![0x12];
                                p.extend_from_slice(&opt.to_le_bytes());
                                with_data(a, |d| scene.st.execute_command(d, me, &[], &p));
                            }
                        }
                    }
                    Key::Escape => scene.chat.close(),
                    k => scene.chat.key(k),
                }
            }
        } else if keys_no.contains(&Key::Enter) && menu.is_none() {
            let c = ["z", "t", "p"][scene.local_race()];
            if let Ok(d) = a.files.read(&format!("rez/{c}textbox.bin")) {
                scene.chat.open(crate::dlg::Dialog::parse(&d), scene.clock_ms);
            }
            chat_keys.push(Key::Enter);
        }
        let keys = if scene.chat.open || !chat_keys.is_empty() { Vec::new() } else { keys_no.clone() };
        for k in keys {
            use minifb::Key::*;
            if let Some(g) = [Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9].iter().position(|&n| n == k) {
                // Local.dll accelerators (table 101) and WM_CHAR (0x4846b0): Ctrl sets,
                // Shift adds (command 0x13), Alt centers on the group and selects it
                // (0x4967e0, 0x496b40), the digit alone selects; selecting the same
                // group again within 500 ms centers too (0x4c07b0).
                let g = g as u8;
                if ctrl || shift {
                    scene.hotkey(if ctrl { 0 } else { 2 }, g);
                    last_group.0 = 0xff;
                    continue;
                }
                scene.hotkey(1, g);
                let now = tick * 42;
                let double = last_group.0 == g && now.saturating_sub(last_group.1) < 500;
                if alt || double {
                    if let Some(p) = scene.group_centre() {
                        (vx, vy) = p;
                    }
                    last_group.0 = 0xff;
                } else {
                    last_group = (g, now);
                }
                continue;
            }
            let fkey = [F2, F3, F4].iter().position(|&n| n == k);
            match (k, fkey) {
                // 0x484360 / 0x484500: Shift+F2..F4 saves the screen, F2..F4 returns.
                (_, Some(i)) if shift => {
                    camera_slots[i] = (vx & !7, vy & !7);
                    continue;
                }
                (_, Some(i)) => {
                    (vx, vy) = camera_slots[i];
                    continue;
                }
                // 0x4e6020: Ctrl / Alt+C centers on the status unit.
                (C, _) if ctrl || alt => {
                    if let Some(&u) = scene.st.selected.first() {
                        let (x, y) = scene.st.unit_sprite_pos(u);
                        (vx, vy) = ((((x as i32) >> 5) - 10) << 5, (((y as i32) >> 5) - 6) << 5);
                    }
                    continue;
                }
                _ => {}
            }
            if replay.is_none() {
                let menu_kind = match k {
                    F1 => Some(crate::menu::Kind::Help),
                    H if alt => Some(crate::menu::Kind::Help),
                    O if alt => Some(crate::menu::Kind::Options),
                    Q if ctrl || alt => Some(crate::menu::Kind::QuitMission),
                    X if ctrl || alt => Some(crate::menu::Kind::Quit),
                    _ => None,
                };
                if menu_kind.is_some() {
                    want_menu = menu_kind;
                    continue;
                }
                // 0x4c0480: +/= and -/_ change the game speed outside multiplayer.
                match k {
                    Equal | NumPadPlus if settings.game_speed < 6 => {
                        settings.game_speed += 1;
                        continue;
                    }
                    Minus | NumPadMinus if settings.game_speed > 0 => {
                        settings.game_speed -= 1;
                        continue;
                    }
                    _ => {}
                }
            }
            // Ctrl+M / Ctrl+S: music / sound off and back (0x4bc320, 0x4bc110).
            if ctrl && k == M {
                (settings.music, saved_music) = if settings.music == 0 { (saved_music, 0) } else { (0, settings.music) };
                continue;
            }
            if ctrl && k == S {
                (settings.digital, saved_digital) = if settings.digital == 0 { (saved_digital, 0) } else { (0, settings.digital) };
                continue;
            }
            if let Some(rp) = replay.as_mut() {
                match k {
                    Tab => {
                        let i = rp.players.iter().position(|&p| p == rp.follow.player).map_or(0, |i| (i + 1) % rp.players.len().max(1));
                        rp.follow = Follow { player: rp.players.get(i).copied().unwrap_or(0), focus: None };
                        following = true;
                    }
                    F => following = !following,
                    _ => {}
                }
                continue;
            }
            // Esc leaves targeting / placement, else it is the hotkey of the card's
            // cancel buttons (stat_txt hotkey byte 0x1b).
            if k == Escape && (scene.end_targeting() || scene.placing.take().is_some()) {
                continue;
            }
            let Some(c) = (k == Escape).then_some('\x1b') else { continue };
            if let Some((b, _)) = card.iter().flatten().find(|(b, en)| *en && hotkey(ui.as_ref(), b) == Some(c)) {
                pressed = Some(*b);
            }
        }
        if clicked && pressed.is_none() {
            if let Some(ui) = ui.as_ref() {
                let ctrl = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
                scene.status_click(ui, cx, cy, shift, ctrl);
            }
        }
        let on_card = clicked && cy >= vh - 120 && !on_minimap_pt(cx, cy);
        if let Some(btn) = pressed.filter(|b| replay.is_none() || matches!(b.act, 0x4df260 | 0x4df2c0 | 0x4df300)) {
            scene.press(&btn, shift);
        }
        if scene.placing.is_some() {
            scene.check_placement((cx, cy), (vx, vy));
            if clicked && cy < vh - 120 && pressed.is_none() {
                scene.build();
                lmb_was_down = true;
                drag_start = None;
            }
        }
        let down = down && scene.placing.is_none();
        let mut targeted = false;
        if scene.targeting.is_some() && clicked && pressed.is_none() && !on_card {
            // A targeted order given on the minimap goes to that map point; the camera
            // stays (the click does not also scroll it).
            let pt = if on_minimap_pt(cx, cy) { ((cx - ox) * scale, (cy - oy) * scale) } else { map_pt };
            scene.target_click(pt, shift);
            lmb_was_down = true; // the click is consumed; don't start a drag
            drag_start = None;
            targeted = true;
            press_used = true;
        }
        if !down {
            press_used = false;
        }
        let down = down && !targeted && !press_used && scene.targeting.is_none() && pressed.is_none() && !on_card;
        // 0x46ff70 (left button down on the map screen): 0x4d1140 says whether the press is
        // over the console (an opaque pixel of the console bitmap); only a press elsewhere
        // starts the selection box. The press decides until the button is released.
        if !down {
            hud_press = None;
        } else if hud_press.is_none() {
            hud_press = Some(ui.as_ref().is_some_and(|u| u.hud_hit(cx, cy)));
        }
        if down && on_minimap_pt(cx, cy) && drag_start.is_none() {
            vx = (cx - ox) * scale - vw / 2;
            vy = (cy - oy) * scale - (vh - 120) / 2;
        } else if down && cy < vh - 120 && (drag_start.is_some() || hud_press == Some(false)) {
            let (sx, sy) = *drag_start.get_or_insert((cx, cy));
            scene.drag_box = Some((sx, sy, cx, cy));
        } else if !down {
            if let Some((sx, sy)) = drag_start.take() {
                scene.drag_box = None;
                let (x0, y0, x1, y1) = (sx + vx, sy + vy, cx + vx, cy + vy);
                let hits = if (x1 - x0).abs() < 4 && (y1 - y0).abs() < 4 {
                    let ctrl = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
                    let double = tick.saturating_sub(last_click) < 8;
                    last_click = tick;
                    let screen = [vx as i16, vy as i16, (vx + 640) as i16, (vy + 400) as i16];
                    let cur = scene.st.selected.clone();
                    let st = &mut scene.st;
                    let p = (map_pt.0 as i16, map_pt.1 as i16);
                    with_data(scene.a, |d| st.click_select(d, p, shift, ctrl, double, screen, &cur)).unwrap_or(cur)
                } else {
                    let r = [x0.min(x1) as i16, y0.min(y1) as i16, x0.max(x1) as i16, y0.max(y1) as i16];
                    let cur = scene.st.selected.clone();
                    let st = &mut scene.st;
                    with_data(scene.a, |d| st.box_select(d, r, shift, &cur)).unwrap_or(cur)
                };
                if scene.st.selected != hits {
                    scene.card_menu = None;
                    scene.placing = None;
                }
                let changed = scene.st.selected != hits;
                scene.set_selected(hits);
                if changed || !scene.st.selected.is_empty() {
                    scene.voice_what();
                }
            }
        }
        let rdown = rmb;
        if rdown && !rmb_was_down && (scene.end_targeting() || scene.placing.take().is_some()) {
            // Right-click cancels targeting / placement.
        } else if replay.is_some() {
            // Watching a replay: no commands.
        } else if rdown && !rmb_was_down && cy < vh - 120 {
            scene.right_click(map_pt, shift);
        } else if rdown && !rmb_was_down && on_minimap_pt(cx, cy) {
            scene.right_click(((cx - ox) * scale, (cy - oy) * scale), shift);
        }
        rmb_was_down = rdown;
        if let Some(kind) = want_menu {
            menu = open_menu(&scene, kind, 0, &mut settings);
            menu_paused = menu.is_some();
            if menu_paused {
                scene.pause_game();
            }
        }
        if win.is_key_down(Key::Left) { vx -= sp }
        if win.is_key_down(Key::Right) { vx += sp }
        if win.is_key_down(Key::Up) { vy -= sp }
        if win.is_key_down(Key::Down) { vy += sp }
        vx = vx.clamp(0, (mw - vw).max(0));
        vy = vy.clamp(0, (mh - vh + 120).max(0)); // console covers the bottom
        // 0x57f1d0: the screen position in tiles.
        scene.st.screen_tile = ((vx.max(0) / 32) as u16, (vy.max(0) / 32) as u16);
        // Replay speed: game frames per window tick (42 ms) at the chosen speed and
        // multiplier (0x4ff90c: ms per frame by speed); paused: none.
        let steps = if replay.is_some() {
            if scene.st.replay_paused {
                0
            } else {
                // 0x4deb90 (replay): the frame time is the table entry divided by the multiplier.
                replay_acc += 42;
                let ms = bw::triggers::replay_frame_ms(scene.st.replay_speed as usize, scene.st.replay_mult as u32);
                let n = replay_acc / ms;
                replay_acc %= ms;
                n
            }
        } else {
            // The game speed (0x6cdfd4) picks the ms per frame (0x5124d8).
            game_acc += 42;
            let ms = bw::triggers::FRAME_MS[settings.game_speed.clamp(0, 6) as usize];
            let n = game_acc / ms;
            game_acc %= ms;
            n
        };
        if let Some(rp) = replay.as_mut().filter(|_| steps > 0) {
            rp.commands(&mut scene);
            // The viewer's own selection stays (the replay's select commands only change
            // the players' 0x6284e8 rows), so clicking selects any unit, minerals too.
            scene.local = rp.follow.player;
            if following {
                (vx, vy) = rp.camera_step((vx, vy), vw, vh - 120);
                vx = vx.clamp(0, (mw - vw).max(0));
                vy = vy.clamp(0, (mh - vh + 120).max(0));
            }
        }
        let mut sounds = Vec::new();
        for k in 0..steps {
            if k > 0 {
                if let Some(rp) = replay.as_mut() {
                    rp.commands(&mut scene);
                }
            }
            sounds.extend(scene.step());
        }
        for (id, x, y) in sounds {
            if let Some(au) = &audio {
                if let Some((v, pan)) = position_sound(&scene, a.dat(), settings.digital, id, x, y, vx, vy) {
                    au.play_pan(&a.files, a.dat(), id, v as i32, pan, None);
                }
            }
        }
        // 0x48ed50: speech and building sounds only with their sound option on.
        for (id, bit, u, base) in std::mem::take(&mut scene.st.speech) {
            if let Some(au) = audio.as_ref().filter(|_| bit == 0 || settings.sound_flags & bit != 0) {
                if let Some((v, pan)) = unit_sound(&scene, a.dat(), settings.digital, id, u, base, vx, vy) {
                    au.play_pan(&a.files, a.dat(), id, v as i32, pan, (u != NIL).then_some(u));
                }
            }
        }
        // 0x48ed50 from the sim (vision gate passed in State::unit_sound): distance volume
        // and pan at the unit's sprite, at least the call's minimum and the sfxdata one.
        for (id, u, min, x, y) in std::mem::take(&mut scene.st.unit_sounds) {
            if let (Some(au), true) = (&audio, settings.digital != 0) {
                let m = a.dat().sfxdata.volume.get(id as usize).copied().unwrap_or(0) as i32;
                let (d, pan) = if u == NIL { (99, 0) } else { (sound_distance_volume(x as i32, y as i32, vx, vy), sound_pan(x as i32, vx)) };
                let v = min.max(d).max(m);
                if v > 10 {
                    au.play_pan(&a.files, a.dat(), id, v, pan, (u != NIL).then_some(u));
                }
            }
        }
        // 0x4c77d0 -> 0x4bc490: the trigger's wav from the scenario archive (a replay embeds
        // only the chk, so there is none then).
        for id in std::mem::take(&mut scene.st.trigger_wavs) {
            let name = scene.chk.string_bytes(id).map(|b| String::from_utf8_lossy(b).into_owned());
            if let (Some(au), Some(m), Some(n)) = (&audio, a.map_mpq.as_ref(), name) {
                if let Ok(d) = m.read(&n) {
                    au.play_file(d);
                }
            }
        }
        for id in std::mem::take(&mut scene.st.ui_sounds) {
            if let Some(au) = &audio {
                au.play(&a.files, a.dat(), id, 1.0);
            }
        }
        // 0x4c6de0 -> 0x47eb70: the trigger's screen move, then its wait ends (0x4894a0).
        // ponytail: 1.16.1 scrolls there over a few frames; this jumps.
        if let Some((x, y)) = scene.st.center_view.take() {
            (vx, vy) = (x, y);
            scene.st.trigger_wait_end();
        }
        if let Some(u) = ui.as_ref() {
            scene.portrait_tick(u, audio.as_ref(), 42, settings.portraits);
            scene.text_tick(u, 42, settings.sound_flags & 0x400 != 0);
        }
        let kind = scene.cursor_kind(cx, cy, vx, vy, drag_start.is_some());
        let counter = tick * 42 / 100;
        scene.gamma = settings.gamma;
        compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((kind, counter, cx, cy)), None, &mut idx, &mut rgb);
        present(&mut win, &rgb, vw as usize, vh as usize)?;
        Ok(true)
    })
}

/// 0x48e8d0: sound volume by distance of map point (x, y) from the 640x400 screen at
/// (left, top): 99 on screen, falling 99 per 512 px (Manhattan overshoot), at least 0.
fn sound_distance_volume(x: i32, y: i32, left: i32, top: i32) -> i32 {
    let mut d = 0;
    if x < left {
        d = left - x;
    } else if left + 0x280 < x {
        d = x - left - 0x280;
    }
    if y < top {
        d += top - y;
    } else if top + 400 < y {
        d += y - top - 400;
    }
    (99 - (d * 99 >> 9)).max(0)
}

/// 0x48e850: DirectSound pan for map x: tile distance from the screen's tile x + 10
/// bucketed to hundredths of a dB, negative left of it.
fn sound_pan(x: i32, left: i32) -> i32 {
    let dx = (x >> 5) - (left >> 5) - 10;
    let m = match dx.abs() {
        0..=2 => 0,
        3..=5 => 100,
        6..=10 => 300,
        11..=20 => 600,
        21..=40 => 1000,
        41..=80 => 5000,
        _ => 10000,
    };
    if dx < 0 { -m } else { m }
}

/// 0x48ec10: a sound id at map point (x, y) with the screen at (left, top): the volume
/// (base 0, distance volume, sfxdata.dat minimum; played above 10) and pan, or None
/// when the sound is dropped (id 0, digital volume 0, outside the map or unseen).
// ponytail: the 0x64086c flag (volume / 5) is not ported (its meaning is unknown).
fn position_sound(scene: &Scene, dat: &Dat, digital: i16, id: u16, x: i32, y: i32, left: i32, top: i32) -> Option<(u32, i32)> {
    if id == 0 || digital == 0 || !scene.st.sound_audible(x, y) {
        return None;
    }
    let m = dat.sfxdata.volume.get(id as usize).copied().unwrap_or(0) as i32;
    let v = sound_distance_volume(x, y, left, top).max(m);
    (v > 10).then(|| (v as u32, sound_pan(x, left)))
}

/// 0x48ed50: a sound `id` spoken by unit `u` (NIL: none, full volume, no pan) with base
/// volume `base`, the screen at left `left`: the volume and pan, or None when dropped (id 0,
/// digital volume 0, the unit unseen by the local player, volume <= 10). The caller checks
/// the option flag.
// ponytail: the 0x64086c flag (volume / 5) is not ported (its meaning is unknown).
fn unit_sound(scene: &Scene, dat: &Dat, digital: i16, id: u16, u: u32, base: i32, left: i32, top: i32) -> Option<(u32, i32)> {
    if id == 0 || digital == 0 {
        return None;
    }
    let st = &scene.st;
    let m = dat.sfxdata.volume.get(id as usize).copied().unwrap_or(0) as i32;
    let (mut v, mut pan) = (99, 0);
    if u != NIL {
        let sp = &st.sprites[st.units[u as usize].sprite as usize];
        if (st.replay || !st.is_local_unit(u)) && sp.visibility & st.local_vision_mask == 0 {
            return None;
        }
        v = sound_distance_volume(sp.x as i32, sp.y as i32, left, top);
        pan = sound_pan(sp.x as i32, left);
    }
    v = base.max(v).max(m);
    (v > 10).then(|| (v as u32, pan))
}

/// 0x4c9de0: the mission objectives: network.tbl lines for the built-in game types
/// (template +8; greed by its resource goal, 0x4c9030), else the map string the local
/// player's objectives trigger set (0x58d6c4).
pub fn objectives_text(scene: &Scene, ui: &crate::ui::Ui) -> String {
    let t = &scene.st.template;
    let net = |i: usize| ui.network.get(i).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
    match t[8] {
        4 => net(0x7a),
        3 => match u32::from_le_bytes([t[4], t[5], t[6], t[7]]) {
            0x9c4 => net(0x75),
            5000 => net(0x76),
            0x1d4c => net(0x77),
            10000 => net(0x78),
            _ => String::new(),
        },
        1 | 7 => net(0x74),
        6 => net(0x79),
        5 => net(0x7b),
        _ => {
            let id = scene.st.trig.objectives[scene.local_player() as usize & 7];
            if id == 0 { String::new() } else { scene.chk.string(id as u16).unwrap_or_default() }
        }
    }
}

/// Width of text with tfontgam color codes (bytes below 0x20 draw nothing).
fn coded_width(ui: &crate::ui::Ui, s: &str) -> i32 {
    ui.font.width(&s.chars().filter(|c| *c >= ' ').collect::<String>())
}

/// Left-aligned text with color codes (0x4202b0, default color 2).
fn draw_coded(ui: &crate::ui::Ui, s: &str, x: i32, y: i32, out: &mut [u8], w: i32, h: i32) {
    let o = crate::ui::TextOpts::new();
    ui.font.draw_text(s, x, y, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out, w, h });
}

/// 0x4e5640 (statres.bin's timer) and its controls' draw 0x4e5460: from the right, the
/// supply of each race with supply (icons.grp 4..6, "used/available" halved, used in
/// color 6 when over), gas (1 + race) and minerals (0). The shown amounts step toward
/// the real ones by 1/16 of the difference (0x4e52e0). A replay shows the first
/// selected unit's owner, nothing for none or neutral.
fn draw_resources(scene: &Scene, ui: &crate::ui::Ui, out: &mut [u8]) {
    let (w, h) = (SCREEN_W, SCREEN_H);
    let st = &scene.st;
    let mut jump = false;
    let p = if st.replay {
        let owner = scene.st.selected.first().map(|&u| st.units[u as usize].player).filter(|&p| p != 11);
        let Some(p) = owner else {
            ui.res_shown.set((ui.res_shown.get().0, ui.res_shown.get().1, 0xff));
            return;
        };
        jump = ui.res_shown.get().2 != p;
        p
    } else {
        scene.local_player()
    } as usize;
    let pl = &st.players;
    let mut items: Vec<(u16, String)> = Vec::new();
    for race in 0..3 {
        // 0x488900: available = min(provided, cap).
        let avail = pl.supply_provided[race][p].min(pl.supply_max[race][p]);
        if avail as u16 != 0 {
            let (used, avail) = ((pl.supply_used[race][p] as u16 as u32 + 1) / 2, (avail as u16 as u32 + 1) / 2);
            items.push((4 + race as u16, format!("{}{used}\x01/{avail}", if avail < used { '\x06' } else { '\x07' })));
        }
    }
    // 0x4e52e0: the counters (0x68c22c gas, 0x68c230 minerals).
    // ponytail: one step per drawn frame; the original's dialog timer runs every 20 ms
    // (0x416090), add a real-time step count when it matters.
    let step = |shown: i32, real: i32| {
        let d = real - shown;
        if d == 0 || jump { real } else { shown + ((d + (d >> 31 & 0xf)) >> 4) + if d > 0 { 1 } else { -1 } }
    };
    let (m0, g0, _) = ui.res_shown.get();
    let (m, g) = (step(m0, pl.minerals[p] as i32), step(g0, pl.gas[p] as i32));
    ui.res_shown.set((m, g, p as u8));
    items.push((1 + scene.local_race() as u16, g.to_string()));
    items.push((0, m.to_string()));
    let o = crate::ui::TextOpts { flags: 0x21, ..crate::ui::TextOpts::new() };
    for (id, (icon, text)) in (1..=5i16).zip(items) {
        let Some(r) = ui.statres.screen_rect(id) else { continue };
        let Some(f) = ui.icons.frame(icon as usize) else { continue };
        blit_frame(&ui.icons, icon as usize, r.0, r.1 + (r.3 - r.1 + 1 - f.h) / 2, f.w, f.h, None, out, w, h);
        let o = crate::ui::TextOpts { clip: r, ..o };
        ui.font.draw_text(&text, r.0 + f.w + 2, r.1, 7, &o, crate::ui::TextDst { rows: &ui.text_colors, out, w, h });
    }
}

/// 0x481510: a tooltip: `text` (font8, color 2, no line gap) on the black box of its
/// extent, outlined in interface color 0x10, its top-left at (x, y) kept on screen.
fn tooltip(ui: &crate::ui::Ui, text: &str, x: i32, y: i32, out: &mut [u8], w: i32, h: i32) {
    let colors = ui.skin.colors.get();
    let mut buf = vec![colors[0xf]; (w * h) as usize];
    let o = crate::ui::TextOpts { flags: 0x11, line_gap: 0, clip: (2, 0, w - 1, h - 1), ..crate::ui::TextOpts::new() };
    let e = ui.skin.fonts[0].draw_text(text, 2, 0, 2, &o, crate::ui::TextDst { rows: &ui.text_colors, out: &mut buf, w, h });
    let (bw, bh) = (e.r - e.l + 1, e.b - e.t + 1);
    let x = x.max(0).min(0x27f - (bw - 1));
    let y = y.max(0).min(0x1df - (bh - 1));
    for yy in 0..bh {
        for xx in 0..bw {
            let edge = xx == 0 || yy == 0 || xx == bw - 1 || yy == bh - 1;
            // 0x4e1c70 frames the buffer from its origin, not from the text's extent.
            let c = if edge { colors[0x10] } else { buf[(yy * w + xx) as usize] };
            if (0..w).contains(&(x + xx)) && (0..h).contains(&(y + yy)) {
                out[((y + yy) * w + x + xx) as usize] = c;
            }
        }
    }
}

/// 0x427a80: the replay panel in the command card dialog (statbtnn.bin): "Replay
/// Progress" (-45), progress (10), elapsed time (-46) and speed (-47).
fn draw_replay_panel(scene: &Scene, ui: &crate::ui::Ui, out: &mut [u8], w: i32, h: i32) {
    let st = &scene.st;
    let dlg = &ui.statbtn;
    let glu = |i: usize| ui.glu_all.get(i.wrapping_sub(1)).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
    let stat = |i: usize| ui.stat_txt.get(i.wrapping_sub(1)).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
    let mut s = crate::dlg::Surf { px: out, w, h };
    let label = |id: i16, text: &str, s: &mut crate::dlg::Surf| {
        if let Some(c) = dlg.get(id) {
            let mut c = c.clone();
            c.text = text.as_bytes().to_vec();
            ui.skin.draw_ctl(&c, dlg.abs(&c), s);
        }
    };
    label(-45, &format!("\x03{}", stat(0x609)), &mut s);
    if let Some((l, t, r, _)) = dlg.screen_rect(10) {
        let pct = (st.frame * 100).checked_div(st.replay_frames).unwrap_or(0).min(100) as i32;
        crate::ui::blit(&ui.pbar_empty, l, t, s.px, w, h);
        crate::ui::blit_w(&ui.pbar_full, l, t, (r - l + 1) * pct / 100, s.px, w, h);
    }
    let ms = bw::triggers::FRAME_MS[st.replay_speed.min(6) as usize] as u64;
    let secs = ms * st.frame as u64 / 1000;
    let time = if secs / 3600 == 0 { format!("{:02}:{:02}", secs / 60 % 60, secs % 60) } else { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) };
    label(-46, &glu(0x10).replace("%s", &time), &mut s);
    let mult = if st.replay_mult > 1 { format!(" {} {}", if st.replay_speed > 2 { "x" } else { "/" }, st.replay_mult) } else { String::new() };
    let speed = if st.replay_paused { stat(0x60a) } else { format!("{} {}{}", glu(0x5d), glu(st.replay_speed as usize + 0x55), mult) };
    label(-47, &speed, &mut s);
}
