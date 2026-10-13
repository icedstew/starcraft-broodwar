//! Playable map viewer on the faithful engine (`bw::State`): terrain, fog of war,
//! sprites, selection, the console HUD and player commands.

use crate::bw::{self, list::NIL};
use crate::casc::Casc;
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
    pub casc: Casc,
    /// Engine data (dat tables, iscript, GRP sizes, LO files).
    pub bw: bw::load::Loaded,
    image_names: Vec<String>,
    grps: RefCell<Vec<Option<Option<std::rc::Rc<Grp>>>>>,
}

impl Assets {
    pub fn load(casc: Casc) -> io::Result<Assets> {
        let bw = bw::load::Loaded::load(&casc)?;
        let image_names = dat::parse_tbl(&casc.read("arr/images.tbl")?);
        Ok(Assets { casc, bw, image_names, grps: RefCell::new(vec![None; dat::Images::COUNT]) })
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
            self.casc.read(&format!("unit/{name}")).ok().map(|d| std::rc::Rc::new(Grp::parse(d)))
        })
        .clone()
    }
}

/// The 1.16.1 minimap for a map, with the tileset's graphics from the install.
pub fn load_minimap(casc: &Casc, chk: &Chk) -> std::io::Result<bw::minimap::Minimap> {
    let name = map::TILESETS[chk.tileset()];
    let rd = |p: String| casc.read(&p);
    let wpe = rd(format!("tileset/{name}.wpe"))?;
    let mut pal = [[0u8; 3]; 256];
    for (i, p) in pal.iter_mut().enumerate() {
        p.copy_from_slice(&wpe[i * 4..i * 4 + 3]);
    }
    let dark = pcx(&rd(format!("tileset/{name}/dark.pcx"))?).2;
    let vx4 = rd(format!("tileset/{name}.vx4ex"))?.chunks_exact(4).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
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
    remaps: [Vec<u8>; 7],
}

/// Melee maps: only start locations and neutral units.
pub fn is_melee_map(chk: &Chk) -> bool {
    chk.section(b"UNIT").unwrap_or(&[]).chunks_exact(36).all(|r| r[16] >= 8 || u16::from_le_bytes([r[8], r[9]]) == 214)
}

impl<'a> Scene<'a> {
    pub fn new(a: &'a Assets, chk: Chk) -> io::Result<Scene<'a>> {
        let ts = Tileset::load(&a.casc, chk.tileset())?;
        let terrain = map::render_terrain(&chk, &ts);
        let (tw, _, tunit) = pcx(&a.casc.read("game/tunit.pcx")?);
        let colr = chk.section(b"COLR").unwrap_or(&[0, 1, 2, 3, 4, 5, 6, 7]).to_vec();
        let mut player_colors = [[0u8; 8]; 12];
        for (p, pc) in player_colors.iter_mut().enumerate() {
            let c = if p < 8 { colr.get(p).copied().unwrap_or(p as u8) as usize } else { p };
            for k in 0..8 {
                pc[k] = tunit.get((c * 8 + k) % tw.max(1)).copied().unwrap_or(0);
            }
        }
        let tsname = map::TILESETS[chk.tileset()];
        let tbl = |n: &str| a.casc.read(&format!("tileset/{tsname}/{n}.pcx")).map(|d| pcx(&d).2).unwrap_or_default();
        // ponytail: shadow row 17 (~50% light) picked by brightness; confirm against the draw-func table in the decomp.
        let dark = tbl("dark");
        let mut shadow = [0u8; 256];
        for (i, s) in shadow.iter_mut().enumerate() {
            *s = dark.get(17 * 256 + i).copied().unwrap_or(i as u8);
        }
        // images.dat remapping ids 1..=7
        let remaps = ["ofire", "gfire", "bfire", "bexpl", "trans50", "red", "green"].map(tbl);
        let mut st = new_game(a, &chk)?;
        let local = st.local_player;
        let mut minimap = load_minimap(&a.casc, &chk)?;
        // 0x581dd6: each player's minimap color is the tminimap.pcx entry of its color.
        let tm = pcx(&a.casc.read("game/tminimap.pcx")?).2;
        for p in 0..12 {
            let c = if p < 8 { colr.get(p).copied().unwrap_or(p as u8) as usize } else { p };
            minimap.player_colors[p] = tm.get(c).copied().unwrap_or(0);
        }
        with_data(a, |d| minimap.refresh(d, &mut st));
        Ok(Scene { st, minimap, selected: Vec::new(), sent_selection: Vec::new(), card_menu: None, placing: None, targeting: None, error: 0, error_frames: 0, place_ok: false, drag_box: None, a, chk, ts, terrain, player_colors, local, shadow, remaps })
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

    /// Advance one frame; returns sounds (sfx id, x, y) requested this frame.
    pub fn step(&mut self) -> Vec<(u16, i32, i32)> {
        let st = &mut self.st;
        with_data(self.a, |d| {
            // 0x4d9670: the frame counter advances before each step.
            st.frame += 1;
            st.game_step(d);
            self.minimap.update(d, st);
        });
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
        sp.flags & bw::sprite::sflag::HIDDEN == 0 && sp.visibility & (1 << self.local) != 0
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
        let (unexplored, fogged) = (1u32 << (self.local + 8), 1u32 << self.local);
        let mw = self.st.terrain.width as i32;
        for y in 0..vh {
            let sy = (vy + y).clamp(0, *th as i32 - 1) as usize;
            let row = &tpx[sy * tw..sy * tw + tw];
            for x in 0..vw {
                let sx = (vx + x).clamp(0, *tw as i32 - 1);
                let f = self.st.terrain.tile_flags.get(((sy as i32 / 32) * mw + sx / 32) as usize).copied().unwrap_or(0);
                let c = row[sx as usize];
                out[(y * vw + x) as usize] = if f & unexplored != 0 { 0 } else if f & fogged != 0 { self.shadow[c as usize] } else { c };
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
        order.sort_by_key(|&s| self.st.sprites[s as usize].elevation);
        let sp_dat = &self.a.dat().sprites;
        for &u in &self.selected {
            let s = self.st.units[u as usize].sprite;
            let sp = &self.st.sprites[s as usize];
            let id = sp.sprite_id as usize;
            // selection circle images start at 561; offset is the vertical shift.
            if let Some(g) = self.a.grp(561 + sp_dat.selection_circle[id] as u16) {
                let x = sp.x as i32 - g.w / 2 - vx;
                let y = sp.y as i32 + sp_dat.selection_offset[id] as i32 - g.h / 2 - vy;
                g.draw(0, false, x, y, out, vw, vh, Blend::Normal);
            }
        }
        for s in order {
            let sp = &self.st.sprites[s as usize];
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
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        with_data(self.a, |d| st.command_right_click(d, me, &sel, (p.0 as i16, p.1 as i16), t, 0xe4, queued));
    }

    /// The status bar's selection (active unit, count).
    pub fn client_selection(&self) -> bw::buttons::Selection {
        with_data(self.a, |d| self.st.client_selection(d, &self.selected))
    }

    /// The button set on the card: an open menu, the cancel sets while targeting
    /// (0xe5) or placing (0xe6), else the active unit's set (+0x94).
    pub fn card_set(&self, s: &bw::buttons::Selection) -> Option<usize> {
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
            Menu(m) => self.card_menu = Some(m),
            CancelTargeting => self.targeting = Option::None,
            CancelPlacement => self.placing = Option::None,
            SelectLarvae(l) => self.selected = l,
            Error => self.show_error(btn.disabled_str),
        }
    }

    /// A map click while targeting (0x46f5b0).
    pub fn target_click(&mut self, p: (i32, i32), queued: bool) {
        let Some(modes) = self.targeting.take() else { return };
        let t = self.unit_at(p.0, p.1).unwrap_or(NIL);
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        let r = with_data(self.a, |d| st.target_click(d, me, &sel, modes, (p.0 as i16, p.1 as i16), t, 0xe4, queued));
        if let Err(e) = r {
            self.show_error(e);
        }
        self.card_menu = Option::None;
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

    pub fn command(&mut self, f: impl FnOnce(&mut bw::State, &bw::Data, u8, &[u32])) {
        let (sel, me) = (self.sim_selection(), self.local);
        let st = &mut self.st;
        with_data(self.a, |d| f(st, d, me, &sel));
    }
}

/// Engine state for a new game on `chk` (map load, players, melee starting units).
fn new_game(a: &Assets, chk: &Chk) -> io::Result<bw::State> {
    let (w, h) = chk.dims();
    let mut st = bw::State::new((w, h), chk.tileset() as u16, 1700);
    let name = map::TILESETS[chk.tileset()];
    let vf4 = a.casc.read(&format!("tileset/{name}.vf4"))?;
    let ts = bw::map::TilesetData { vf4: vf4.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(), cv5: a.casc.read(&format!("tileset/{name}.cv5"))? };
    st.rng.seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32);
    with_data(a, |d| {
        st.search.init(d);
        st.init_tech_defaults(d);
        st.init_acquisition_ranges(d);
        st.init_psi(&a.bw.psi_grp);
        st.init_thingies(d);
        st.load_terrain(&ts, w as u16, h as u16, chk.section(b"MTXM").unwrap_or(&[]));
        if let Some(doodads) = chk.section(b"THG2") {
            st.chk_doodads(d, doodads, Default::default());
        }
        let melee = is_melee_map(chk) && std::env::var("SCBW_NOMELEE").is_err();
        let units = chk.section(b"UNIT").unwrap_or(&[]);
        if melee {
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
        st.local_vision_mask = 1 << st.local_player;
        st.init_alliances(Default::default());
        st.chk_units(d, units, Default::default());
        if melee {
            st.rng.enabled = true;
            st.melee_start_units(d, 2);
            st.rng.enabled = false;
        }
    });
    Ok(st)
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
    cursor: Option<(usize, i32, i32)>,
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
    for (icon, text) in [(0, minerals.as_str()), (1 + race, gas.as_str()), (4 + race, supply.as_str())] {
        ui.draw_icon(icon, x, 2, pal, out, w, h);
        ui.font.draw(text, x + 16, 4, col, pal, out, w, h);
        x += 66;
    }
    // Selected unit panel.
    if let [u] = scene.selected[..] {
        let mu = &scene.st.units[u as usize];
        let kind = mu.unit_type as usize;
        crate::ui::draw_grp_rgb(&ui.wireframe, kind, 170, 390, pal, out, w, h);
        let name = ui.stat_txt.get(kind).map_or("", |s| s.split('\0').next().unwrap_or(""));
        ui.font.draw(name, 260, 392, ui.text_color(2), pal, out, w, h);
        let max = scene.a.dat().units.hit_points[kind] >> 8;
        if max > 0 {
            let hp = format!("{}/{}", (mu.hp.max(0) as u32).div_ceil(256), max);
            ui.font.draw(&hp, 176, 456, ui.text_color(3), pal, out, w, h);
        }
    } else if !scene.selected.is_empty() {
        ui.font.draw(&format!("{} units", scene.selected.len()), 260, 392, ui.text_color(2), pal, out, w, h);
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
    if let (Some((_, kind)), Some((_, cx, cy))) = (scene.placing, cursor) {
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
    if let Some((tick, cx, cy)) = cursor {
        ui.draw_cursor(tick, cx, cy, pal, out, w, h);
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

pub fn run(a: &Assets, chk: Chk) -> io::Result<()> {
    use minifb::{Key, Scale, Window, WindowOptions};
    let mut scene = Scene::new(a, chk)?;
    let race = ["zerg", "terran", "protoss"][scene.local_race()];
    let ui = crate::ui::Ui::load(&a.casc, race).ok();
    let mut audio = crate::audio::Audio::open(&a.casc);
    if let Some(au) = audio.as_mut() {
        au.play_music(&a.casc, &format!("music/{race}1.ogg"));
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
            let Some(c) = key_char(k) else { continue };
            if let Some((b, _)) = card.iter().flatten().find(|(b, en)| *en && hotkey(ui.as_ref(), b) == Some(c)) {
                pressed = Some(*b);
            }
        }
        let on_card = clicked && cy >= vh - 120 && !on_minimap_pt(cx, cy);
        if let Some(btn) = pressed {
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
                scene.selected = hits;
            }
        }
        let rdown = win.get_mouse_down(minifb::MouseButton::Right);
        if rdown && !rmb_was_down && (scene.targeting.take().is_some() || scene.placing.take().is_some()) {
            // Right-click cancels targeting / placement.
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
        for (id, x, y) in scene.step() {
            // ponytail: on-screen only, no distance falloff or per-frame voice limits yet.
            if let Some(au) = &audio {
                if (vx..vx + vw).contains(&x) && (vy..vy + vh).contains(&y) {
                    au.play(&a.casc, a.dat(), id, 0.6);
                }
            }
        }
        compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((tick, cx, cy)), &mut idx, &mut rgb);
        win.update_with_buffer(&rgb, vw as usize, vh as usize).map_err(|e| io::Error::other(e.to_string()))?;
    }
    Ok(())
}
