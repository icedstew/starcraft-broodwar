//! Map viewer: terrain + map units/doodads animated by iscript.

use crate::casc::Casc;
use crate::dat::{self, Dat};
use crate::grp::{Blend, Grp};
use crate::iscript::{Iscript, Sprite, World};
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
    pub dat: Dat,
    pub iscript: Iscript,
    image_names: Vec<String>,
    grps: RefCell<Vec<Option<Option<std::rc::Rc<Grp>>>>>,
}

impl Assets {
    pub fn load(casc: Casc) -> io::Result<Assets> {
        let dat = Dat::load(&casc)?;
        let iscript = Iscript::parse(casc.read("scripts/iscript.bin")?);
        let image_names = dat::parse_tbl(&casc.read("arr/images.tbl")?);
        Ok(Assets { casc, dat, iscript, image_names, grps: RefCell::new(vec![None; dat::Images::COUNT]) })
    }

    pub fn grp(&self, image: u16) -> Option<std::rc::Rc<Grp>> {
        let mut g = self.grps.borrow_mut();
        let slot = g.get_mut(image as usize)?;
        slot.get_or_insert_with(|| {
            let idx = self.dat.images.grp[image as usize] as usize;
            let name = self.image_names.get(idx.checked_sub(1)?)?;
            self.casc.read(&format!("unit/{name}")).ok().map(|d| std::rc::Rc::new(Grp::parse(d)))
        })
        .clone()
    }
}

pub struct Scene<'a> {
    pub game: crate::game::Game,
    pub selected: Vec<usize>,
    /// Screen-space drag rectangle while box-selecting.
    pub drag_box: Option<(i32, i32, i32, i32)>,
    pub a: &'a Assets,
    pub chk: Chk,
    pub ts: Tileset,
    pub terrain: (usize, usize, Vec<u8>),
    pub player_colors: [[u8; 8]; 12],
    shadow: [u8; 256],
    remaps: [Vec<u8>; 7],
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
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = World { dat: &a.dat, iscript: &a.iscript, grp_frames: &frames, tileset: chk.tileset() as u16 };
        let (gw, gh, walk) = map::walk_grid(&chk, &ts);
        let game = crate::game::Game::new(&w, &chk, crate::game::path::WalkGrid { w: gw, h: gh, walk });
        Ok(Scene { game, selected: Vec::new(), drag_box: None, a, chk, ts, terrain, player_colors, shadow, remaps })
    }

    fn world(&self, frames: &'a dyn Fn(u16) -> u16) -> World<'a> {
        World { dat: &self.a.dat, iscript: &self.a.iscript, grp_frames: frames, tileset: self.chk.tileset() as u16 }
    }

    /// Advance one frame; returns sounds (sfx id, x, y) requested this frame.
    pub fn step(&mut self) -> Vec<(u16, i32, i32)> {
        let a = self.a;
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = self.world(&frames);
        self.game.step(&w)
    }

    pub fn spawn(&mut self, kind: usize, owner: u8, x: i32, y: i32) -> usize {
        let a = self.a;
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = self.world(&frames);
        self.game.create_unit(&w, kind, owner, x, y, 100)
    }

    /// Local player: first human (OWNR 6) slot.
    pub fn local_player(&self) -> u8 {
        self.chk.owners().iter().position(|&o| o == 6).unwrap_or(0) as u8
    }

    /// Local player's race (SIDE: 0 zerg, 1 terran, 2 protoss); random/other -> terran.
    pub fn local_race(&self) -> usize {
        match self.chk.races()[self.local_player() as usize] {
            r @ 0..=2 => r as usize,
            _ => 1,
        }
    }

    /// (used, max) supply for `player` and race, from units.dat values (stored doubled).
    pub fn supply(&self, player: u8, race: usize) -> (u32, u32) {
        let u = &self.a.dat.units;
        let race_flag = [1u8, 2, 4][race];
        let (mut used, mut max) = (0u32, 0u32);
        for mu in self.game.units.iter().filter(|m| !m.dead && m.owner == player && u.staredit_group_flags[m.kind] & race_flag != 0) {
            used += u.supply_required[mu.kind] as u32;
            max += u.supply_provided[mu.kind] as u32;
        }
        (used.div_ceil(2), (max / 2).min(200))
    }

    /// (x, y, unit index) for every unit, from its sprite.
    pub fn unit_positions(&self) -> Vec<(i32, i32, usize)> {
        // Subunits (turrets, units.dat flag 0x10) are not selectable on their own.
        let flags = &self.a.dat.units.flags;
        (0..self.game.units.len())
            .filter(|&i| !self.game.units[i].dead && flags[self.game.units[i].kind] & 0x10 == 0)
            .map(|i| {
                let (x, y) = self.game.unit_pos(i);
                (x, y, i)
            })
            .collect()
    }

    /// Units whose units.dat bounds intersect the map-space rectangle.
    pub fn units_in_rect(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<usize> {
        let d = &self.a.dat.units.dimensions;
        self.unit_positions()
            .into_iter()
            .filter(|&(x, y, u)| {
                let dm = d[self.game.units[u].kind];
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
                out[(y * vw + x) as usize] = row[(vx + x).clamp(0, *tw as i32 - 1) as usize];
            }
        }
        let mut order: Vec<&Sprite> = self.game.sprites.iter().flatten().filter(|s| (s.x - vx).abs() < vw + 256 && (s.y - vy).abs() < vh + 256).collect();
        order.sort_by_key(|s| (s.elevation, s.y));
        for s in &order {
            let Some(u) = s.unit.filter(|u| self.selected.contains(u)) else { continue };
            let sp = &self.a.dat.sprites;
            let id = s.id as usize;
            // selection circle images start at 561; offset is the vertical shift.
            let img = 561 + sp.selection_circle[id] as u16;
            if let Some(g) = self.a.grp(img) {
                let x = s.x - g.w / 2 - vx;
                let y = s.y + sp.selection_offset[id] as i32 - g.h / 2 - vy;
                let _ = u;
                g.draw(0, false, x, y, out, vw, vh, Blend::Normal);
            }
        }
        for s in order {
            for img in &s.images {
                if img.flags & crate::iscript::image_flags::HIDDEN != 0 {
                    continue;
                }
                let Some(g) = self.a.grp(img.id) else { continue };
                let x = s.x + img.x as i32 - g.w / 2 - vx;
                let y = s.y + img.y as i32 - g.h / 2 - vy;
                let blend = match img.draw_fn {
                    10 => Blend::Remap(&self.shadow),
                    9 => match self.a.dat.images.remapping[img.id as usize] {
                        r @ 1..=7 => Blend::Table(&self.remaps[r as usize - 1]),
                        _ => Blend::Normal,
                    },
                    _ => Blend::Player(&self.player_colors[(s.owner as usize).min(11)]),
                };
                g.draw(img.frame as usize, img.flipped(), x, y, out, vw, vh, blend);
            }
        }
    }

    pub fn palette(&self) -> &[[u8; 3]; 256] {
        &self.ts.palette
    }
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

/// Buttons for the current selection (the first selected unit's type).
pub fn card_buttons(scene: &Scene) -> &'static [crate::game::buttons::Button] {
    match scene.selected.first() {
        Some(&u) if scene.game.units[u].owner == scene.local_player() => {
            crate::game::buttons::SETS.get(scene.game.units[u].kind).copied().unwrap_or(&[])
        }
        _ => &[],
    }
}

/// Full frame: map view + console + minimap + resource bar, as 0xRRGGBB.
pub fn compose(scene: &Scene, ui: Option<&crate::ui::Ui>, vx: i32, vy: i32, idx: &mut [u8], out: &mut [u32]) {
    compose_with_cursor(scene, ui, vx, vy, None, idx, out)
}

/// Minimap geometry: (origin x, origin y, map pixels per minimap pixel).
pub fn minimap_geom(scene: &Scene) -> (i32, i32, i32) {
    let (mx, my, ms) = MINIMAP;
    let (tw, th) = (scene.terrain.0 as i32, scene.terrain.1 as i32);
    let scale = (tw.max(th) / ms).max(1);
    (mx + (ms - tw / scale) / 2, my + (ms - th / scale) / 2, scale)
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
    // Minimap: sample the terrain image, then mark units and the current view.
    let (tw, th, tpx) = &scene.terrain;
    let (ox, oy, scale) = minimap_geom(scene);
    let (mw, mh) = (*tw as i32 / scale, *th as i32 / scale);
    let rgb = |i: u8| {
        let [r, g, b] = pal[i as usize];
        (r as u32) << 16 | (g as u32) << 8 | b as u32
    };
    for y in 0..mh {
        for x in 0..mw {
            let t = tpx[(y * scale) as usize * tw + (x * scale) as usize];
            out[((oy + y) * w + ox + x) as usize] = rgb(t);
        }
    }
    for s in scene.game.sprites.iter().flatten() {
        if s.owner < 8 {
            let (px, py) = (ox + s.x / scale, oy + s.y / scale);
            if (0..w).contains(&px) && (0..h).contains(&py) {
                out[(py * w + px) as usize] = rgb(scene.player_colors[s.owner as usize][0]);
            }
        }
    }
    let (rx, ry, rw, rh) = (ox + vx / scale, oy + vy / scale, w / scale, (h - 120) / scale);
    for i in 0..=rw {
        for py in [ry, ry + rh] {
            if (0..h).contains(&py) && (0..w).contains(&(rx + i)) {
                out[(py * w + rx + i) as usize] = 0xFFFFFF;
            }
        }
    }
    for i in 0..=rh {
        for px in [rx, rx + rw] {
            if (0..h).contains(&(ry + i)) && (0..w).contains(&px) {
                out[((ry + i) * w + px) as usize] = 0xFFFFFF;
            }
        }
    }
    // Resource bar (values come from game state once players exist).
    // icons.grp frames: 0 minerals, 1..=3 gas (z/t/p), 4..=6 supply (z/t/p).
    let col = ui.text_color(2);
    let race = scene.local_race();
    let (used, max) = scene.supply(scene.local_player(), race);
    // icons.grp race order is z/t/p, same as SIDE race ids.
    let supply = format!("{}/{}", used, max);
    let mut x = 440;
    for (icon, text) in [(0, "50"), (1 + race, "0"), (4 + race, supply.as_str())] {
        ui.draw_icon(icon, x, 2, pal, out, w, h);
        ui.font.draw(text, x + 16, 4, col, pal, out, w, h);
        x += 66;
    }
    // Selected unit panel.
    if let [u] = scene.selected[..] {
        let mu = &scene.game.units[u];
        crate::ui::draw_grp_rgb(&ui.wireframe, mu.kind, 170, 390, pal, out, w, h);
        let name = ui.stat_txt.get(mu.kind).map_or("", |s| s.split('\0').next().unwrap_or(""));
        ui.font.draw(name, 260, 392, ui.text_color(2), pal, out, w, h);
        let max = scene.a.dat.units.hit_points[mu.kind] >> 8;
        if max > 0 {
            let hp = format!("{}/{}", (mu.hp.max(0) as u32).div_ceil(256), max);
            ui.font.draw(&hp, 176, 456, ui.text_color(3), pal, out, w, h);
        }
    } else if !scene.selected.is_empty() {
        ui.font.draw(&format!("{} units", scene.selected.len()), 260, 392, ui.text_color(2), pal, out, w, h);
    }
    if let Some((x0, y0, x1, y1)) = scene.drag_box {
        for x in x0.min(x1)..=x0.max(x1) {
            for y in [y0, y1] {
                if (0..w).contains(&x) && (0..h).contains(&y) { out[(y * w + x) as usize] = 0x10FC18; }
            }
        }
        for y in y0.min(y1)..=y0.max(y1) {
            for x in [x0, x1] {
                if (0..w).contains(&x) && (0..h).contains(&y) { out[(y * w + x) as usize] = 0x10FC18; }
            }
        }
    }
    for b in card_buttons(scene) {
        let (bx, by) = card_slot(b.pos);
        ui.draw_cmd_icon(b.icon as usize, 0, bx + 2, by + 1, pal, out, w, h);
    }
    if let Some((tick, cx, cy)) = cursor {
        ui.draw_cursor(tick, cx, cy, pal, out, w, h);
    }
}

use crate::game::buttons::Act;

fn on_minimap_pt(x: i32, y: i32) -> bool {
    (MINIMAP.0..MINIMAP.0 + MINIMAP.2).contains(&x) && (MINIMAP.1..MINIMAP.1 + MINIMAP.2).contains(&y)
}

/// Give `order` to every selected unit the local player owns.
fn issue_to_selection(scene: &mut Scene, order: u8, target: Option<crate::game::Target>) {
    let me = scene.local_player();
    for &u in &scene.selected.clone() {
        if scene.game.units[u].owner == me {
            let t = target.unwrap_or(crate::game::Target { pos: scene.game.units[u].flingy.pos, unit: None });
            scene.game.issue_order(u, order, t, false);
        }
    }
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
    let (mut vx, mut vy) = (0, 0);
    let mut idx = vec![0u8; (vw * vh) as usize];
    let mut rgb = vec![0u32; (vw * vh) as usize];
    win.set_cursor_visibility(false);
    let mut tick = 0usize;
    let mut drag_start: Option<(i32, i32)> = None;
    let mut rmb_was_down = false;
    let mut lmb_was_down = false;
    // Command-card targeting mode (Move/Attack/Patrol waiting for a map click).
    let mut targeting: Option<Act> = None;
    while win.is_open() && !win.is_key_down(Key::Escape) {
        tick += 1;
        let sp = 16;
        let (cx, cy) = win.get_mouse_pos(minifb::MouseMode::Clamp).map_or((vw / 2, vh / 2), |(x, y)| (x as i32, y as i32));
        if cx <= 1 { vx -= sp }
        if cx >= vw - 2 { vx += sp }
        if cy <= 1 { vy -= sp }
        if cy >= vh - 2 { vy += sp }
        let (ox, oy, scale) = minimap_geom(&scene);
        let down = win.get_mouse_down(minifb::MouseButton::Left);
        let clicked = down && !lmb_was_down;
        lmb_was_down = down;
        // Command card: click a button or press its hotkey.
        let mut pressed: Option<Act> = None;
        if clicked {
            pressed = card_buttons(&scene).iter().find(|b| {
                let (bx, by) = card_slot(b.pos);
                (bx..bx + 40).contains(&cx) && (by..by + 36).contains(&cy)
            }).map(|b| b.act);
        }
        for k in win.get_keys_pressed(minifb::KeyRepeat::No) {
            let want = match k {
                Key::M => Some(Act::Move),
                Key::S => Some(Act::Stop),
                Key::A => Some(Act::Attack),
                Key::P => Some(Act::Patrol),
                Key::H => Some(Act::HoldPosition),
                _ => None,
            };
            if let Some(a) = want.filter(|a| card_buttons(&scene).iter().any(|b| b.act == *a)) {
                pressed = Some(a);
            }
        }
        if let Some(act) = pressed {
            match act {
                Act::Stop => issue_to_selection(&mut scene, crate::game::orders::id::STOP, None),
                Act::HoldPosition => issue_to_selection(&mut scene, crate::game::orders::id::HOLD_POSITION, None),
                Act::Move | Act::Attack | Act::Patrol => targeting = Some(act),
                _ => {}
            }
        }
        if let (Some(act), true) = (targeting, clicked && cy < vh - 120 && pressed.is_none()) {
            let (tx, ty) = ((cx + vx) as i16, (cy + vy) as i16);
            let hit = scene.units_in_rect(tx as i32, ty as i32, tx as i32, ty as i32).into_iter().next();
            let me = scene.local_player();
            let order = match act {
                Act::Attack => match hit {
                    Some(h) if scene.game.units[h].owner != me => crate::game::orders::id::ATTACK_UNIT,
                    _ => crate::game::orders::id::ATTACK_MOVE,
                },
                Act::Patrol => crate::game::orders::id::PATROL,
                _ => crate::game::orders::id::MOVE,
            };
            issue_to_selection(&mut scene, order, Some(crate::game::Target { pos: (tx, ty), unit: hit }));
            targeting = None;
            lmb_was_down = true; // the click is consumed; don't start a drag
            drag_start = None;
        }
        let down = down && targeting.is_none() && pressed.is_none() && !(clicked && cy >= vh - 120 && !on_minimap_pt(cx, cy));
        let on_minimap = (MINIMAP.0..MINIMAP.0 + MINIMAP.2).contains(&cx) && (MINIMAP.1..MINIMAP.1 + MINIMAP.2).contains(&cy);
        if down && on_minimap && drag_start.is_none() {
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
                    // Click: the single unit nearest the point.
                    let pt = (cx + vx, cy + vy);
                    let mut hits = scene.units_in_rect(pt.0, pt.1, pt.0, pt.1);
                    hits.truncate(1);
                    hits
                } else {
                    // Box: BW only box-selects the player's own units (max 12).
                    let mut hits: Vec<usize> = scene.units_in_rect(x0, y0, x1, y1).into_iter().filter(|&u| scene.game.units[u].owner == scene.local_player()).collect();
                    hits.truncate(12);
                    hits
                };
                scene.selected = hits;
            }
        }
        let rdown = win.get_mouse_down(minifb::MouseButton::Right);
        if rdown && !rmb_was_down && targeting.take().is_some() {
            // Right-click cancels targeting.
        } else if rdown && !rmb_was_down && cy < vh - 120 {
            let (tx, ty) = ((cx + vx) as i16, (cy + vy) as i16);
            let me = scene.local_player();
            // Right-click on a unit: attack if hostile, else move to it.
            let hit = scene.units_in_rect(tx as i32, ty as i32, tx as i32, ty as i32).into_iter().next();
            for &u in &scene.selected.clone() {
                if scene.game.units[u].owner == me {
                    let t = crate::game::Target { pos: (tx, ty), unit: hit };
                    let order = match hit {
                        Some(h) if scene.game.units[h].owner != me && scene.game.units[h].owner < 8 => crate::game::orders::id::ATTACK_UNIT,
                        _ => crate::game::orders::id::MOVE,
                    };
                    scene.game.issue_order(u, order, t, false);
                }
            }
        }
        rmb_was_down = rdown;
        if win.is_key_down(Key::Left) { vx -= sp }
        if win.is_key_down(Key::Right) { vx += sp }
        if win.is_key_down(Key::Up) { vy -= sp }
        if win.is_key_down(Key::Down) { vy += sp }
        vx = vx.clamp(0, (mw - vw).max(0));
        vy = vy.clamp(0, (mh - vh + 120).max(0)); // console covers the bottom
        scene.selected.retain(|&u| !scene.game.units[u].dead);
        for (id, x, y) in scene.step() {
            // ponytail: on-screen only, no distance falloff or per-frame voice limits yet.
            if let Some(au) = &audio {
                if (vx..vx + vw).contains(&x) && (vy..vy + vh).contains(&y) {
                    au.play(&a.casc, &a.dat, id, 0.6);
                }
            }
        }
        compose_with_cursor(&scene, ui.as_ref(), vx, vy, Some((tick, cx, cy)), &mut idx, &mut rgb);
        win.update_with_buffer(&rgb, vw as usize, vh as usize).map_err(|e| io::Error::other(e.to_string()))?;
    }
    Ok(())
}
