//! Playable map viewer on the faithful engine (`bw::State`): terrain, fog of war,
//! sprites, selection, the console HUD and player commands.

use crate::bw::{self, list::NIL};
use crate::storage::Storage;
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
    image_names: Vec<String>,
    grps: RefCell<Vec<Option<Option<std::rc::Rc<Grp>>>>>,
}

impl Assets {
    pub fn load(files: Storage) -> io::Result<Assets> {
        let bw = bw::load::Loaded::load(&files)?;
        let image_names = dat::parse_tbl(&files.read("arr/images.tbl")?);
        Ok(Assets { files, bw, image_names, grps: RefCell::new(vec![None; dat::Images::COUNT]) })
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
    Ok(bw::minimap::Minimap::new(w as u16, h as u16, chk.tileset() as u16, &pal, &dark, vx4, vr4, colors))
}

/// Run `f` with the engine's `Data` view over `a`.
fn with_data<R>(a: &Assets, f: impl FnOnce(&bw::Data) -> R) -> R {
    let g = |i: u16| a.bw.grp_size(i);
    let d = bw::load::data(&a.bw, &g);
    f(&d)
}

pub struct Scene<'a> {
    pub st: bw::State,
    pub selected: Vec<u32>,
    /// Unit voices (selection and acknowledgement lines).
    pub voices: crate::voices::Voices,
    /// 0x59724c: the selection last sent to the simulation as select commands.
    sent_selection: Vec<u32>,
    /// Command card submenu (button set index) opened by a Menu button, e.g. SCV "Build".
    pub card_menu: Option<u16>,
    /// Building type being placed, and whether the spot under the cursor is valid.
    pub placing: Option<(u8, u16)>,
    /// Targeting mode: orders for a ground / unit / fog click (0x641691..3).
    pub targeting: Option<(u8, u8, u8)>,
    /// Last refused button or click (stat_txt message id, 0 for none).
    pub error: u16,
    /// Frames the error line stays up (0x48ccb0: 7000 ms).
    pub error_frames: u32,
    pub place_ok: bool,
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
    pub fn new(a: &'a Assets, chk: Chk) -> io::Result<Scene<'a>> {
        Scene::start(a, chk, None)
    }

    /// The game a replay recorded, set up as the replay's header says.
    pub fn from_replay(a: &'a Assets, r: &crate::replay::Replay) -> io::Result<Scene<'a>> {
        let mut s = Scene::start(a, Chk::parse(&r.chk), Some(&r.header))?;
        s.st.replay_frames = r.header.frames;
        Ok(s)
    }

    fn start(a: &'a Assets, chk: Chk, rep: Option<&crate::replay::Header>) -> io::Result<Scene<'a>> {
        let ts = Tileset::load(&a.files, chk.tileset())?;
        let terrain = map::render_terrain(&chk, &ts);
        let (tw, _, tunit) = pcx(&a.files.read("game/tunit.pcx")?);
        let colr = match rep {
            // A replay's header keeps each player's color as the game assigned it.
            Some(h) => h.slots.iter().take(8).map(|s| s.color as u8).collect(),
            None => chk.section(b"COLR").unwrap_or(&[0, 1, 2, 3, 4, 5, 6, 7]).to_vec(),
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
        let mut st = new_game(a, &chk, rep)?;
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
            st.game_step(d);
            st.game_step(d);
            minimap.refresh(d, &mut st)
        });
        Ok(Scene { st, minimap, selected: Vec::new(), voices: crate::voices::Voices::new(), sent_selection: Vec::new(), card_menu: None, placing: None, targeting: None, error: 0, error_frames: 0, place_ok: false, drag_box: None, a, chk, ts, terrain, player_colors, local, shadow, dark, fog: crate::fog::Fog::new(), tselect, thpbar, remaps })
    }

    /// Send the local selection to the simulation (0x4c0860) and return the local
    /// player's selection as the simulation sees it (0x6284e8).
    fn sim_selection(&mut self) -> Vec<u32> {
        let me = self.local;
        let mut sent = std::mem::take(&mut self.sent_selection);
        let pkts = self.st.selection_packets(&mut sent, &self.selected);
        self.sent_selection = sent;
        let st = &mut self.st;
        with_data(self.a, |d| {
            for p in &pkts {
                st.execute_command(d, me, &[], p);
            }
        });
        bw::select::selection_units(&self.st.selections[me as usize]).to_vec()
    }

    /// Hotkey group command 0x13 (`action` 0 set, 1 select, 2 add); the local selection
    /// follows the simulation's.
    pub fn hotkey(&mut self, action: u8, g: u8) {
        self.sim_selection();
        let me = self.local;
        let st = &mut self.st;
        with_data(self.a, |d| st.execute_command(d, me, &[], &[0x13, action, g]));
        let sel = bw::select::selection_units(&self.st.selections[me as usize]).to_vec();
        if sel != self.selected {
            self.card_menu = None;
            self.placing = None;
        }
        self.selected = sel.clone();
        self.sent_selection = sel;
    }

    /// 0x48ccb0: put stat_txt line `id` (1-based) up for 7 seconds.
    pub fn show_error(&mut self, id: u16) {
        self.error = id;
        self.error_frames = 7000 / 42;
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
            self.minimap.update(d, st);
        });
        self.st.alert_tick();
        self.voices.tick();
        // 0x4a4e40 runs on a 200 ms timer: about every fifth frame at 42 ms.
        if self.st.frame % 5 == 0 {
            self.minimap.blink_phase = !self.minimap.blink_phase;
        }
        if self.st.shown_error != 0 {
            let e = std::mem::take(&mut self.st.shown_error);
            self.show_error(e);
        }
        self.error_frames = self.error_frames.saturating_sub(1);
        self.selected.retain(|&u| self.st.unit_alive(u));
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

    /// Local player's race (0 zerg, 1 terran, 2 protoss).
    pub fn local_race(&self) -> usize {
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
        let (tw, th, tpx) = &self.terrain;
        for y in 0..vh {
            let sy = (vy + y).clamp(0, *th as i32 - 1) as usize;
            let row = &tpx[sy * tw..sy * tw + tw];
            for x in 0..vw {
                let sx = (vx + x).clamp(0, *tw as i32 - 1);
                out[(y * vw + x) as usize] = row[sx as usize];
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
        // below the sprite, colored from tselect.pcx by 0x581d6a (own 0, ally 1, enemy 2).
        let sp_dat = &self.a.dat().sprites;
        let circled: std::collections::HashMap<u32, (u8, u32)> = self
            .selected
            .iter()
            .map(|&u| {
                let un = &self.st.units[u as usize];
                let p = un.player as usize;
                let c = if p == self.local as usize { 0 } else if self.st.alliances[self.local as usize][p] == 0 { 2 } else { 1 };
                (un.sprite, (c, u))
            })
            .collect();
        let total = order.len();
        for (k, s) in order.into_iter().enumerate() {
            if k == fogged {
                self.fog.shade(&self.st, &self.dark, vx, vy, out, vw as usize, vh as usize);
            }
            let sp = &self.st.sprites[s as usize];
            if let Some(&(c, _)) = circled.get(&s) {
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
                let blend = match img.modifier {
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
        self.voice_yes();
    }

    /// 0x4563a0 / 0x48f4d0: the status unit (first selected) acknowledges an order.
    pub fn voice_yes(&mut self) {
        let Some(&u) = self.selected.first() else { return };
        let (st, v) = (&self.st, &mut self.voices);
        if let Some(id) = with_data(self.a, |d| v.yes(st, d, u)) {
            self.st.ui_sounds.push(id);
        }
    }

    /// 0x49aef0 / 0x48f910: the highest-ranked selected unit (units.dat rank + its
    /// rank increase, 0x49a350) answers a new selection.
    pub fn voice_what(&mut self) {
        let st = &self.st;
        let best = with_data(self.a, |d| {
            self.selected.iter().copied().max_by_key(|&u| {
                let un = &st.units[u as usize];
                d.dat.units.rank[un.unit_type as usize] as u32 + un.u8_at(0x8e) as u32
            })
        });
        let Some(u) = best else { return };
        let (st, v) = (&self.st, &mut self.voices);
        if let Some(id) = with_data(self.a, |d| v.what(st, d, u)) {
            self.st.ui_sounds.push(id);
        }
    }

    /// The status bar's selection (active unit, count).
    pub fn client_selection(&self) -> bw::buttons::Selection {
        with_data(self.a, |d| self.st.client_selection(d, &self.selected))
    }

    /// The button set on the card: an open menu, the cancel sets while targeting
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
        (s.active != NIL).then(|| self.st.units[s.active as usize].button_set as usize)
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
            CancelTargeting => self.targeting = Option::None,
            CancelPlacement => self.placing = Option::None,
            SelectLarvae(l) => self.selected = l,
            Error => self.show_error(btn.disabled_str),
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

    /// Placement tile for building `t` with the cursor at map point `p`.
    pub fn build_tile(&self, t: u16, p: (i32, i32)) -> (i16, i16) {
        let v = self.a.dat().units.placement_box[t as usize];
        let (w, h) = (v as i16 as i32, (v >> 16) as i16 as i32);
        (((p.0 - w / 2 + 16) / 32) as i16, ((p.1 - h / 2 + 16) / 32) as i16)
    }

    /// Update `place_ok` for the building being placed at map point `p` (placement
    /// grid slot 1, as the placement cursor does).
    pub fn check_placement(&mut self, p: (i32, i32)) {
        let Some((_, t)) = self.placing else { return };
        let (tx, ty) = self.build_tile(t, p);
        let u = self.client_selection().active;
        let (me, st) = (self.local, &mut self.st);
        self.place_ok = with_data(self.a, |d| st.placement_check(d, u, me, tx, ty, t, 1, false, false, false) == 0);
    }

    /// Place the building at map point `p`: the build command (0x0c).
    pub fn build(&mut self, p: (i32, i32)) {
        let Some((order, t)) = self.placing.take() else { return };
        let (tx, ty) = self.build_tile(t, p);
        let mut pkt = vec![0x0c, order];
        pkt.extend_from_slice(&(tx as u16).to_le_bytes());
        pkt.extend_from_slice(&(ty as u16).to_le_bytes());
        pkt.extend_from_slice(&t.to_le_bytes());
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        with_data(self.a, |d| st.execute_command(d, me, &sel, &pkt));
        self.card_menu = Option::None;
    }

    /// The selected building's statdata panel, if it shows one.
    pub fn status_panel(&self, ui: &crate::ui::Ui) -> Option<(crate::statdata::Panel, Vec<(i16, crate::statdata::Show)>)> {
        if self.selected.len() > 1 {
            return Some((crate::statdata::Panel::Group, crate::statdata::group_panel(&self.selected)));
        }
        let [u] = self.selected[..] else { return None };
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

    /// A click at screen (x, y) on a status panel button (0x4573a0); true if handled.
    pub fn status_click(&mut self, ui: &crate::ui::Ui, x: i32, y: i32) -> bool {
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
                    // A unit of a multiple selection: select just it.
                    crate::statdata::Show::GroupWire(unit) => {
                        self.selected = vec![unit];
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

/// Engine state for a new game on `chk` (map load, players, melee starting units), or
/// for the game replay header `rep` recorded.
fn new_game(a: &Assets, chk: &Chk, rep: Option<&crate::replay::Header>) -> io::Result<bw::State> {
    let (w, h) = chk.dims();
    let mut st = bw::State::new((w, h), chk.tileset() as u16, 1700);
    let name = map::TILESETS[chk.tileset()];
    let vf4 = a.files.read(&format!("tileset/{name}.vf4"))?;
    let ts = bw::map::TilesetData { vf4: vf4.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(), cv5: a.files.read(&format!("tileset/{name}.cv5"))? };
    st.rng.seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32);
    with_data(a, |d| {
        st.search.init(d);
        st.init_tech_defaults(d);
        st.init_acquisition_ranges(d);
        st.init_psi(&a.bw.psi_grp);
        st.init_thingies(d);
        st.load_terrain(&ts, w as u16, h as u16, &chk.mtxm());
        let melee = rep.is_none() && is_melee_map(chk) && std::env::var("SCBW_NOMELEE").is_err();
        // 0x596865: the game's template (a replay keeps its game type and sub type; a
        // map played here is Melee, or Use Map Settings when it is not a melee map).
        let (gt, sub) = match rep {
            Some(h) => (h.game_type, h.sub_type),
            None if melee => (2, 1),
            None => (10, 1),
        };
        st.template = load_template(a, gt, sub);
        let modes = bw::chk_load::ScenarioModes::of_template(&st.template);
        let units = chk.section(b"UNIT").unwrap_or(&[]); // start locations: the last UNIT handler call sets them
        if let Some(h) = rep {
            // 0x4ef100: a replay's game uses the seed in its header (0x6d0f38).
            st.rng.seed = h.start_time;
            // The header's player table is 0x57eee0 as it was when the game started.
            for (p, s) in h.slots.iter().enumerate().take(8) {
                st.player_types[p] = s.kind;
                st.player_races[p] = s.race;
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
        // 0x4eee00 loads the scenario right after 0x4ef100 seeds the RNG, with it on.
        // Handler order (table 0x5001e8): MTXM, THG2 (its unit doodads draw from the
        // RNG too), UNIT.
        st.rng.enabled = true;
        for doodads in chk.sections(b"THG2") {
            st.chk_doodads(d, doodads, Default::default());
        }
        for units in chk.sections(b"UNIT") {
            st.chk_units(d, units, modes);
        }
        st.rng.enabled = false;
        // 0x4bf520: 0x49da40 starting units (team melee with shared bases 0x49d8e0 is
        // not ported).
        st.rng.enabled = true;
        st.melee_start_units(d, st.template[0xc]);
        st.rng.enabled = false;
    });
    Ok(st)
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

/// The draw order 0x498d40 gets from the sprite heap: entries pushed in row-list order
/// (0x42d4c0 / 0x42d400, max-heap), popped largest first (0x42d460 / 0x42d370) into
/// 0x6c2318 and drawn from the last popped back to the first.
fn heap_draw_order(entries: Vec<(u32, u32)>) -> Vec<u32> {
    // 1-based heap as at 0x6c4a30.
    let mut h: Vec<(u32, u32)> = vec![(0, 0)];
    for e in entries {
        h.push(e);
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
/// Command card: top-left of button 1 and the slot pitch (3x3, positions 1..9).
const CARD: (i32, i32, i32, i32) = (505, 358, 46, 40);

/// Screen rect origin of command card position `pos` (1..=9).
pub fn card_slot(pos: u16) -> (i32, i32) {
    let i = pos as i32 - 1;
    (CARD.0 + (i % 3) * CARD.2, CARD.1 + (i / 3) * CARD.3)
}

/// Full frame: map view + console + minimap + resource bar, as 0xRRGGBB.
pub fn compose(scene: &Scene, ui: Option<&crate::ui::Ui>, vx: i32, vy: i32, idx: &mut [u8], out: &mut [u32]) {
    compose_with_cursor(scene, ui, vx, vy, None, idx, out)
}

/// Minimap geometry: (origin x, origin y, map pixels per minimap pixel). 0x4a4400
/// centers a smaller image in the 128x128 control.
pub fn minimap_geom(scene: &Scene) -> (i32, i32, i32) {
    let (mx, my, ms) = MINIMAP;
    let m = &scene.minimap;
    (mx + (ms - m.w as i32) / 2, my + (ms - m.h as i32) / 2, m.tile_px as i32)
}

fn rect(out: &mut [u32], w: i32, h: i32, (x0, y0, x1, y1): (i32, i32, i32, i32), col: u32) {
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

pub fn compose_with_cursor(
    scene: &Scene,
    ui: Option<&crate::ui::Ui>,
    vx: i32,
    vy: i32,
    cursor: Option<(usize, usize, i32, i32)>,
    idx: &mut [u8],
    out: &mut [u32],
) {
    let (w, h) = (SCREEN_W, SCREEN_H);
    scene.draw(vx, vy, w, h, idx);
    let pal = scene.palette();
    for (o, &i) in out.iter_mut().zip(idx.iter()) {
        let [r, g, b] = pal[i as usize];
        *o = (r as u32) << 16 | (g as u32) << 8 | b as u32;
    }
    let Some(ui) = ui else { return };
    crate::ui::blit_rgba(&ui.console, 0, h - ui.console.h as i32, out, w, h);
    let (ox, oy, scale) = minimap_geom(scene);
    let rgb = |i: u8| {
        let [r, g, b] = pal[i as usize];
        (r as u32) << 16 | (g as u32) << 8 | b as u32
    };
    let mm = &scene.minimap;
    for y in 0..mm.h as i32 {
        for x in 0..mm.w as i32 {
            out[((oy + y) * w + ox + x) as usize] = rgb(mm.pixels[(y * mm.w as i32 + x) as usize]);
        }
    }
    let (rx, ry, rw, rh) = (ox + vx / scale, oy + vy / scale, w / scale, (h - 120) / scale);
    rect(out, w, h, (rx, ry, rx + rw, ry + rh), 0xFFFFFF);
    // Resource bar. icons.grp frames: 0 minerals, 1..=3 gas (z/t/p), 4..=6 supply (z/t/p).
    let col = ui.text_color(2);
    let race = scene.local_race();
    let me = scene.local_player() as usize;
    let (used, max) = scene.supply(me as u8, race);
    let (supply, minerals, gas) = (format!("{}/{}", used, max), scene.st.players.minerals[me].to_string(), scene.st.players.gas[me].to_string());
    let mut x = 440;
    // Watching a replay shows no resources (statres is not shown).
    if !scene.st.replay {
        for (icon, text) in [(0, minerals.as_str()), (1 + race, gas.as_str()), (4 + race, supply.as_str())] {
            ui.draw_icon(icon, x, 2, pal, out, w, h);
            ui.font.draw(text, x + 16, 4, col, pal, out, w, h);
            x += 66;
        }
    }
    if scene.st.replay {
        draw_replay_panel(scene, ui, pal, out, w, h);
    }
    // Building panels of statdata.cpp (training queue, research, upgrade).
    if let Some((_, ctls)) = scene.status_panel(ui) {
        let sd = &ui.statdata;
        for (id, show) in &ctls {
            let Some((l, t, r, b)) = sd.screen_rect(*id) else { continue };
            let (cw, ch) = (r - l + 1, b - t + 1);
            match show {
                crate::statdata::Show::Text(s) => {
                    let tw = coded_width(ui, s);
                    draw_coded(ui, s, l + (cw - tw) / 2, t + (ch - ui.font.height) / 2, pal, out, w, h);
                }
                crate::statdata::Show::Wireframe => {
                    let [u] = scene.selected[..] else { continue };
                    // 0x4568f0: the frame is the shown type's (0 past the grp's frames).
                    let wt = crate::ui::wire_type(&scene.st, u) as usize;
                    let frame = if wt < ui.wireframe.frame_count() { wt } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, u, &ui.twire));
                    crate::ui::draw_grp_rgb_blend(&ui.wireframe, frame, l, t, pal, out, w, h, crate::grp::Blend::Colors(&colors));
                }
                crate::statdata::Show::Cargo { bg, unit } => {
                    crate::ui::draw_grp_rgb(&ui.cmdbtns, *bg as usize, l, t, pal, out, w, h);
                    let wt = crate::ui::wire_type(&scene.st, *unit) as usize;
                    let frame = if wt < ui.tranwire.frame_count() { wt } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, *unit, &ui.twire));
                    crate::ui::draw_grp_rgb_blend(&ui.tranwire, frame, l, t, pal, out, w, h, crate::grp::Blend::Colors(&colors));
                }
                crate::statdata::Show::GroupWire(u) => {
                    let wt = crate::ui::wire_type(&scene.st, *u) as usize;
                    let frame = if wt < ui.grpwire.frame_count() { wt } else { 0 };
                    let colors = with_data(scene.a, |d| crate::ui::wireframe_colors(&scene.st, d, *u, &ui.twire));
                    crate::ui::draw_grp_rgb_blend(&ui.grpwire, frame, l, t, pal, out, w, h, crate::grp::Blend::Colors(&colors));
                }
                crate::statdata::Show::Info { icon, level, .. } => {
                    crate::ui::draw_grp_rgb(&ui.cmdbtns, 2, l, t, pal, out, w, h);
                    ui.draw_cmd_icon(*icon as usize, 0, l + 2, t + 1, pal, out, w, h);
                    let n = format!("{level} ");
                    let tw = ui.font.width(&n);
                    ui.font.draw(&n, r - tw, b - ui.font.height, ui.text_color(2), pal, out, w, h);
                }
                crate::statdata::Show::Progress(v) => {
                    // 0x456b40: the empty bar, then the full one clipped to the percentage.
                    crate::ui::blit_rgba(&ui.pbar_empty, l, t, out, w, h);
                    let fw = cw * *v as i32 / 100;
                    let full = &ui.pbar_full;
                    for y in 0..full.h.min(ch as usize) as i32 {
                        for x in 0..fw.min(full.w as i32) {
                            let [r, g, b, a] = full.px[y as usize * full.w + x as usize];
                            if a != 0 {
                                out[((t + y) * w + l + x) as usize] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
                            }
                        }
                    }
                }
                crate::statdata::Show::Button { bg, icon, cmdicon } => {
                    crate::ui::draw_grp_rgb(&ui.cmdbtns, *bg as usize, l, t, pal, out, w, h);
                    if *cmdicon {
                        ui.draw_cmd_icon(*icon as usize, 0, l + 2, t + 1, pal, out, w, h);
                    } else if let Some(f) = ui.cmdbtns.frame(*icon as usize) {
                        crate::ui::draw_grp_rgb(&ui.cmdbtns, *icon as usize, l + (cw - f.w as i32) / 2 - f.x as i32, t + (ch - f.h as i32) / 2 - f.y as i32, pal, out, w, h);
                    }
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
                draw_text_box(ui, tip, r, t, pal, out, w, h);
            }
        }
    }
    if scene.error != 0 && scene.error_frames > 0 {
        let msg = ui.stat_txt.get(scene.error as usize - 1).map_or("", |s| s.split('\0').next().unwrap_or(""));
        let msg: String = msg.chars().filter(|c| *c >= ' ').collect();
        let tw = msg.chars().count() as i32 * 6;
        // Color code 4 (0x641680); tfontgam rows start at code 2.
        ui.font.draw(&msg, (w - tw) / 2, h - 160, ui.text_color(4 - 2), pal, out, w, h);
    }
    if let Some(b) = scene.drag_box {
        rect(out, w, h, b, 0x10FC18);
    }
    if let (Some((_, kind)), Some((_, _, cx, cy))) = (scene.placing, cursor) {
        // placebox.cpp (0x48daf0, 0x48d7f0): the building's first frame in the local
        // player's color, each tile dithered (0x40c4d2) green when free, red when not.
        let (tx, ty) = scene.build_tile(kind, (cx + vx, cy + vy));
        let dat = scene.a.dat();
        let v = dat.units.placement_box[kind as usize];
        let (pw, ph) = (v as u16 as i32, (v >> 16) as u16 as i32);
        let img = dat.sprites.image[dat.flingy.sprite[dat.units.flingy[kind as usize] as usize] as usize];
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
                let blocked = scene.st.placement[(r as usize + 8) * 6 + c as usize] != 0;
                let col = scene.minimap.colors[if blocked { 0x14 } else { 0x13 }];
                for yy in 0..32 {
                    for xx in (yy & 1..32).step_by(2) {
                        surf[((oy + r * 32 + yy) * sw + ox + c * 32 + xx) as usize] = col;
                    }
                }
            }
        }
        let (sx, sy) = (tx as i32 * 32 - vx - ox, ty as i32 * 32 - vy - oy);
        for (i, &c) in surf.iter().enumerate() {
            let (px, py) = (sx + i as i32 % sw, sy + i as i32 / sw);
            if c != 0 && (0..w).contains(&px) && (0..h - 120).contains(&py) {
                let [r, g, b] = pal[c as usize];
                out[(py * w + px) as usize] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
            }
        }
    }
    for (i, slot) in scene.card().iter().enumerate() {
        if let Some((b, enabled)) = slot {
            let (bx, by) = card_slot(i as u16 + 1);
            ui.draw_cmd_icon(b.icon as usize, if *enabled { 0 } else { 1 }, bx + 2, by + 1, pal, out, w, h);
        }
    }
    // 0x4593a0: the hovered button's tooltip.
    if let Some((_, _, cx, cy)) = cursor {
        for (i, slot) in scene.card().iter().enumerate() {
            let Some((b, enabled)) = slot else { continue };
            let (bx, by) = card_slot(i as u16 + 1);
            if (bx..bx + 40).contains(&cx) && (by..by + 36).contains(&cy) {
                draw_tooltip(scene, ui, b, *enabled, bx, by, pal, out, w, h);
            }
        }
    }
    if let Some((kind, counter, cx, cy)) = cursor {
        ui.draw_cursor(kind, counter, cx, cy, pal, out, w, h);
    }
}

/// 0x4593a0 / 0x459150 / 0x459030: a command button's tooltip: its stat_txt string
/// (after the hotkey and kind bytes) and, for enabled buttons, the cost: minerals, gas
/// and supply for units (kind 1), the current level's cost and the next level for
/// upgrades (2), minerals, gas and energy for techs (3).
#[allow(clippy::too_many_arguments)]
fn draw_tooltip(scene: &Scene, ui: &crate::ui::Ui, b: &bw::buttons::Button, enabled: bool, bx: i32, by: i32, pal: &[[u8; 3]; 256], out: &mut [u32], w: i32, h: i32) {
    let id = if enabled { b.enabled_str } else { b.disabled_str };
    let Some(raw) = (id as usize).checked_sub(1).and_then(|i| ui.stat_txt.get(i)) else { return };
    let raw = raw.split('\0').next().unwrap_or("");
    let kind = raw.chars().nth(1).map_or(0, |c| c as u32);
    let mut text: String = raw.chars().skip(2).collect();
    let me = scene.local_player();
    let dat = scene.a.dat();
    let v = b.act_var as usize;
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
    let lines: Vec<&str> = text.split('\n').collect();
    let lh = ui.font.height + 1;
    let tw = lines.iter().map(|l| coded_width(ui, l)).max().unwrap_or(0);
    let has_cost = cost != (0, 0, 0, 0);
    let bw_ = (tw + 8).max(if has_cost { 150 } else { 0 });
    let bh = lines.len() as i32 * lh + if has_cost { 14 } else { 0 } + 4;
    let x0 = (bx + 40 - bw_).clamp(0, w - bw_);
    let y0 = by - bh - 4;
    for y in y0.max(0)..(y0 + bh).min(h) {
        for x in x0..(x0 + bw_).min(w) {
            out[(y * w + x) as usize] = 0;
        }
    }
    for (k, l) in lines.iter().enumerate() {
        draw_coded(ui, l, x0 + 4, y0 + 2 + k as i32 * lh, pal, out, w, h);
    }
    if has_cost {
        let race = scene.local_race();
        let y = y0 + 2 + lines.len() as i32 * lh;
        let mut x = x0 + 4;
        for (icon, n, show) in [(0, cost.0, true), (1 + race, cost.1, true), (4 + race, cost.2, cost.2 != 0), (7, cost.3, cost.3 != 0)] {
            if !show {
                continue;
            }
            ui.draw_icon(icon, x, y, pal, out, w, h);
            ui.font.draw(&n.to_string(), x + 16, y + 2, ui.text_color(2), pal, out, w, h);
            x += 36;
        }
    }
}

fn on_minimap_pt(x: i32, y: i32) -> bool {
    (MINIMAP.0..MINIMAP.0 + MINIMAP.2).contains(&x) && (MINIMAP.1..MINIMAP.1 + MINIMAP.2).contains(&y)
}

/// The hotkey of a button: the first character of its stat_txt string.
fn hotkey(ui: Option<&crate::ui::Ui>, b: &bw::buttons::Button) -> Option<char> {
    let s = ui?.stat_txt.get((b.enabled_str as usize).checked_sub(1)?)?;
    s.chars().next().map(|c| c.to_ascii_lowercase())
}

fn key_char(k: minifb::Key) -> Option<char> {
    use minifb::Key::*;
    let keys = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    keys.iter().position(|&x| x == k).map(|i| (b'a' + i as u8) as char)
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
                let sel = bw::select::selection_units(&st.selections[player as usize]);
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

pub fn run(a: &Assets, chk: Chk) -> io::Result<()> {
    run_with(a, Scene::new(a, chk)?, None)
}

/// Watch a replay; the camera follows a player (Tab: next player, F: follow on/off).
pub fn run_replay(a: &Assets, rep: crate::replay::Replay) -> io::Result<()> {
    let scene = Scene::from_replay(a, &rep)?;
    run_with(a, scene, Some(ReplayPlay::new(rep)))
}

fn run_with(a: &Assets, mut scene: Scene, mut replay: Option<ReplayPlay>) -> io::Result<()> {
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
        au.play_music(&a.files, &format!("music/{race}1.wav"));
    }
    let (vw, vh) = (SCREEN_W, SCREEN_H);
    let opts = WindowOptions { scale: Scale::X2, ..WindowOptions::default() };
    let mut win = Window::new("scbw", vw as usize, vh as usize, opts).map_err(|e| io::Error::other(e.to_string()))?;
    win.set_target_fps(24); // BW "fastest" = 42 ms/frame
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
    while win.is_open() && !win.is_key_down(Key::Escape) {
        tick += 1;
        let sp = 16;
        let shift = win.is_key_down(Key::LeftShift) || win.is_key_down(Key::RightShift);
        let (cx, cy) = win.get_mouse_pos(minifb::MouseMode::Clamp).map_or((vw / 2, vh / 2), |(x, y)| (x as i32, y as i32));
        if cx <= 1 { vx -= sp }
        if cx >= vw - 2 { vx += sp }
        if cy <= 1 { vy -= sp }
        if cy >= vh - 2 { vy += sp }
        let (ox, oy, scale) = minimap_geom(&scene);
        let map_pt = (cx + vx, cy + vy);
        let down = win.get_mouse_down(minifb::MouseButton::Left);
        let clicked = down && !lmb_was_down;
        lmb_was_down = down;
        // Command card: click a button or press its hotkey (enabled buttons only).
        let card = scene.card();
        let mut pressed: Option<bw::buttons::Button> = None;
        if clicked {
            pressed = card.iter().enumerate().find_map(|(i, slot)| {
                let (bx, by) = card_slot(i as u16 + 1);
                ((bx..bx + 40).contains(&cx) && (by..by + 36).contains(&cy)).then_some(*slot).flatten()
            }).and_then(|(b, enabled)| enabled.then_some(b));
        }
        for k in win.get_keys_pressed(minifb::KeyRepeat::No) {
            use minifb::Key::*;
            if let Some(g) = [Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9].iter().position(|&n| n == k) {
                // Hotkey groups (command 0x13): Ctrl sets, Shift adds, the key alone selects.
                let ctrl = win.is_key_down(LeftCtrl) || win.is_key_down(RightCtrl);
                scene.hotkey(if ctrl { 0 } else if shift { 2 } else { 1 }, g as u8);
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
            let Some(c) = key_char(k) else { continue };
            if let Some((b, _)) = card.iter().flatten().find(|(b, en)| *en && hotkey(ui.as_ref(), b) == Some(c)) {
                pressed = Some(*b);
            }
        }
        if clicked && pressed.is_none() {
            if let Some(ui) = ui.as_ref() {
                scene.status_click(ui, cx, cy);
            }
        }
        let on_card = clicked && cy >= vh - 120 && !on_minimap_pt(cx, cy);
        if let Some(btn) = pressed.filter(|b| replay.is_none() || matches!(b.act, 0x4df260 | 0x4df2c0 | 0x4df300)) {
            scene.press(&btn, shift);
        }
        if scene.placing.is_some() {
            scene.check_placement(map_pt);
            if clicked && cy < vh - 120 && pressed.is_none() {
                if scene.place_ok {
                    scene.build(map_pt);
                }
                lmb_was_down = true;
                drag_start = None;
            }
        }
        let down = down && scene.placing.is_none();
        if scene.targeting.is_some() && clicked && pressed.is_none() && !on_card {
            let pt = if on_minimap_pt(cx, cy) { ((cx - ox) * scale, (cy - oy) * scale) } else { map_pt };
            scene.target_click(pt, shift);
            lmb_was_down = true; // the click is consumed; don't start a drag
            drag_start = None;
        }
        let down = down && scene.targeting.is_none() && pressed.is_none() && !on_card;
        if down && on_minimap_pt(cx, cy) && drag_start.is_none() {
            vx = (cx - ox) * scale - vw / 2;
            vy = (cy - oy) * scale - (vh - 120) / 2;
        } else if down && cy < vh - 120 {
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
                    let cur = scene.selected.clone();
                    let st = &mut scene.st;
                    let p = (map_pt.0 as i16, map_pt.1 as i16);
                    with_data(scene.a, |d| st.click_select(d, p, shift, ctrl, double, screen, &cur)).unwrap_or(cur)
                } else {
                    let r = [x0.min(x1) as i16, y0.min(y1) as i16, x0.max(x1) as i16, y0.max(y1) as i16];
                    let cur = scene.selected.clone();
                    let st = &mut scene.st;
                    with_data(scene.a, |d| st.box_select(d, r, shift, &cur)).unwrap_or(cur)
                };
                if scene.selected != hits {
                    scene.card_menu = None;
                    scene.placing = None;
                }
                let changed = scene.selected != hits;
                scene.selected = hits;
                if changed || !scene.selected.is_empty() {
                    scene.voice_what();
                }
            }
        }
        let rdown = win.get_mouse_down(minifb::MouseButton::Right);
        if rdown && !rmb_was_down && (scene.targeting.take().is_some() || scene.placing.take().is_some()) {
            // Right-click cancels targeting / placement.
        } else if replay.is_some() {
            // Watching a replay: no commands.
        } else if rdown && !rmb_was_down && cy < vh - 120 {
            scene.right_click(map_pt, shift);
        } else if rdown && !rmb_was_down && on_minimap_pt(cx, cy) {
            scene.right_click(((cx - ox) * scale, (cy - oy) * scale), shift);
        }
        rmb_was_down = rdown;
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
                replay_acc += 42 * scene.st.replay_mult as u32;
                let ms = [167u32, 111, 83, 67, 56, 48, 42][scene.st.replay_speed.min(6) as usize];
                let n = replay_acc / ms;
                replay_acc %= ms;
                n
            }
        } else {
            1
        };
        if let Some(rp) = replay.as_mut().filter(|_| steps > 0) {
            rp.commands(&mut scene);
            // The followed player's view: their selection is shown.
            scene.selected = bw::select::selection_units(&scene.st.selections[rp.follow.player as usize]).to_vec();
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
            // ponytail: on-screen only, no distance falloff or per-frame voice limits yet.
            if let Some(au) = &audio {
                if (vx..vx + vw).contains(&x) && (vy..vy + vh).contains(&y) {
                    au.play(&a.files, a.dat(), id, 1.0);
                }
            }
        }
        for id in std::mem::take(&mut scene.st.ui_sounds) {
            if let Some(au) = &audio {
                au.play(&a.files, a.dat(), id, 1.0);
            }
        }
        let kind = scene.cursor_kind(cx, cy, vx, vy, drag_start.is_some());
        let counter = tick * 42 / 100;
        compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((kind, counter, cx, cy)), &mut idx, &mut rgb);
        win.update_with_buffer(&rgb, vw as usize, vh as usize).map_err(|e| io::Error::other(e.to_string()))?;
    }
    Ok(())
}

/// Width of text with tfontgam color codes (bytes below 0x20 draw nothing).
fn coded_width(ui: &crate::ui::Ui, s: &str) -> i32 {
    ui.font.width(&s.chars().filter(|c| *c >= ' ').collect::<String>())
}

/// Text with color codes: code `c` switches to tfontgam row `c - 2` (default 2).
#[allow(clippy::too_many_arguments)]
fn draw_coded(ui: &crate::ui::Ui, s: &str, mut x: i32, y: i32, pal: &[[u8; 3]; 256], out: &mut [u32], w: i32, h: i32) {
    let mut color = 2usize;
    let mut run = String::new();
    for ch in s.chars() {
        if (ch as u32) < 0x20 {
            if !run.is_empty() {
                ui.font.draw(&run, x, y, ui.text_color(color - 2), pal, out, w, h);
                x += ui.font.width(&run);
                run.clear();
            }
            color = (ch as usize).max(2);
        } else {
            run.push(ch);
        }
    }
    if !run.is_empty() {
        ui.font.draw(&run, x, y, ui.text_color(color - 2), pal, out, w, h);
    }
}

/// A tooltip box (0x481510): `text` lines on black, bottom-right at (`right`, `bottom`).
#[allow(clippy::too_many_arguments)]
fn draw_text_box(ui: &crate::ui::Ui, text: &str, right: i32, bottom: i32, pal: &[[u8; 3]; 256], out: &mut [u32], w: i32, h: i32) {
    let lines: Vec<&str> = text.split('\n').collect();
    let lh = ui.font.height + 1;
    let bw_ = lines.iter().map(|l| coded_width(ui, l)).max().unwrap_or(0) + 8;
    let bh = lines.len() as i32 * lh + 4;
    let x0 = (right - bw_).clamp(0, (w - bw_).max(0));
    let y0 = (bottom - bh - 2).max(0);
    for y in y0..(y0 + bh).min(h) {
        for x in x0..(x0 + bw_).min(w) {
            out[(y * w + x) as usize] = 0;
        }
    }
    for (k, l) in lines.iter().enumerate() {
        draw_coded(ui, l, x0 + 4, y0 + 2 + k as i32 * lh, pal, out, w, h);
    }
}

/// 0x427a80: the replay panel in the command card dialog (statbtnn.bin): "Replay
/// Progress" (-45), progress (10), elapsed time (-46) and speed (-47).
fn draw_replay_panel(scene: &Scene, ui: &crate::ui::Ui, pal: &[[u8; 3]; 256], out: &mut [u32], w: i32, h: i32) {
    let st = &scene.st;
    let Some(dlg) = ui.statbtn_replay.as_ref() else { return };
    let glu = |i: usize| ui.glu_all.get(i.wrapping_sub(1)).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
    let stat = |i: usize| ui.stat_txt.get(i.wrapping_sub(1)).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string());
    let centered = |id: i16, text: &str, out: &mut [u32]| {
        if let Some((l, t, r, b)) = dlg.screen_rect(id) {
            let tw = coded_width(ui, text);
            draw_coded(ui, text, l + (r - l + 1 - tw) / 2, t + (b - t + 1 - ui.font.height) / 2, pal, out, w, h);
        }
    };
    centered(-45, &format!("\x03{}", stat(0x609)), out);
    if let Some((l, t, r, b)) = dlg.screen_rect(10) {
        let pct = (st.frame * 100).checked_div(st.replay_frames).unwrap_or(0).min(100) as i32;
        crate::ui::blit_rgba(&ui.pbar_empty, l, t, out, w, h);
        let fw = (r - l + 1) * pct / 100;
        let full = &ui.pbar_full;
        for y in 0..full.h.min((b - t + 1) as usize) as i32 {
            for x in 0..fw.min(full.w as i32) {
                let [cr, cg, cb, a] = full.px[y as usize * full.w + x as usize];
                if a != 0 {
                    out[((t + y) * w + l + x) as usize] = (cr as u32) << 16 | (cg as u32) << 8 | cb as u32;
                }
            }
        }
    }
    let ms = [167u64, 111, 83, 67, 56, 48, 42][st.replay_speed.min(6) as usize];
    let secs = ms * st.frame as u64 / 1000;
    let time = if secs / 3600 == 0 { format!("{:02}:{:02}", secs / 60 % 60, secs % 60) } else { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) };
    centered(-46, &glu(0x10).replace("%s", &time), out);
    let mult = if st.replay_mult > 1 { format!(" {} {}", if st.replay_speed > 2 { "x" } else { "/" }, st.replay_mult) } else { String::new() };
    let speed = if st.replay_paused { stat(0x60a) } else { format!("{} {}{}", glu(0x5d), glu(st.replay_speed as usize + 0x56), mult) };
    centered(-47, &speed, out);
}
