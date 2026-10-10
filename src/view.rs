//! Map viewer: terrain + map units/doodads animated by iscript.

use crate::casc::Casc;
use crate::dat::{self, Dat};
use crate::grp::{Blend, Grp};
use crate::iscript::{Effect, Iscript, Rng, Sprite, World};
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

/// A map-placed unit (game logic proper comes later; this backs selection/UI).
pub struct MapUnit {
    pub id: usize,
    pub owner: u8,
    /// Hit points, 24.8 fixed point like BW.
    pub hp: u32,
}

pub struct Scene<'a> {
    pub units: Vec<MapUnit>,
    pub selected: Vec<usize>,
    /// Screen-space drag rectangle while box-selecting.
    pub drag_box: Option<(i32, i32, i32, i32)>,
    pub a: &'a Assets,
    pub chk: Chk,
    pub ts: Tileset,
    pub terrain: (usize, usize, Vec<u8>),
    pub sprites: Vec<Sprite>,
    pub rng: Rng,
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
        let mut s = Scene { units: Vec::new(), selected: Vec::new(), drag_box: None, a, chk, ts, terrain, sprites: Vec::new(), rng: Rng { seed: 0 }, player_colors, shadow, remaps };
        s.place_map_objects();
        Ok(s)
    }

    fn world(&self, frames: &'a dyn Fn(u16) -> u16) -> World<'a> {
        World { dat: &self.a.dat, iscript: &self.a.iscript, grp_frames: frames, tileset: self.chk.tileset() as u16 }
    }

    fn place_map_objects(&mut self) {
        let a = self.a;
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = self.world(&frames);
        let u = &a.dat.units;
        let mut new = Vec::new();
        if let Some(units) = self.chk.section(b"UNIT") {
            for r in units.chunks_exact(36) {
                let r16 = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let (x, y, id, owner) = (r16(4) as i32, r16(6) as i32, r16(8) as usize, r[16]);
                if id >= dat::Units::COUNT || id == 214 {
                    continue; // 214 = start location, never drawn
                }
                let sprite = a.dat.flingy.sprite[u.flingy[id] as usize];
                let mut s = Sprite::new(&w, &mut self.rng, sprite, owner, x, y);
                s.elevation = u.elevation[id];
                // UNIT record: hp% at offset 0x11 (percentage of max).
                let pct = if r[17] == 0 { 100 } else { r[17] as u32 };
                s.unit = Some(self.units.len());
                self.units.push(MapUnit { id, owner, hp: u.hit_points[id] / 100 * pct });
                new.push(s);
                if u.subunit1[id] as usize != 228 {
                    let sub = u.subunit1[id] as usize;
                    let sp = a.dat.flingy.sprite[u.flingy[sub] as usize];
                    let mut t = Sprite::new(&w, &mut self.rng, sp, owner, x, y);
                    t.elevation = u.elevation[id] + 1;
                    new.push(t);
                }
            }
        }
        if let Some(thg2) = self.chk.section(b"THG2") {
            for r in thg2.chunks_exact(10) {
                let r16 = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let (id, x, y, owner, flags) = (r16(0), r16(2) as i32, r16(4) as i32, r[6], r16(8));
                // Flag 0x1000: pure sprite; otherwise id is a unit type (doodad unit).
                let sprite = if flags & 0x1000 != 0 {
                    id
                } else if (id as usize) < dat::Units::COUNT {
                    a.dat.flingy.sprite[u.flingy[id as usize] as usize]
                } else {
                    continue;
                };
                new.push(Sprite::new(&w, &mut self.rng, sprite, owner, x, y));
            }
        }
        self.sprites = new;
    }

    /// Advance one frame; returns sounds (sfx id, x, y) requested this frame.
    pub fn step(&mut self) -> Vec<(u16, i32, i32)> {
        let a = self.a;
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = self.world(&frames);
        let mut fx = Vec::new();
        let rng = &mut self.rng;
        self.sprites.retain_mut(|s| s.step(&w, rng, &mut fx));
        let mut sounds = Vec::new();
        for e in fx {
            match e {
                Effect::SpawnSprite { sprite, x, y, elevation_delta } => {
                    let mut s = Sprite::new(&w, &mut self.rng, sprite, 11, x, y);
                    s.elevation = s.elevation.saturating_add_signed(elevation_delta);
                    self.sprites.push(s);
                }
                Effect::Sound { id, x, y } => sounds.push((id, x, y)),
            }
        }
        sounds
    }

    /// (x, y, unit index) for every unit, from its sprite.
    pub fn unit_positions(&self) -> Vec<(i32, i32, usize)> {
        self.sprites.iter().filter_map(|s| s.unit.map(|u| (s.x, s.y, u))).collect()
    }

    /// Units whose units.dat bounds intersect the map-space rectangle.
    pub fn units_in_rect(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<usize> {
        let d = &self.a.dat.units.dimensions;
        self.unit_positions()
            .into_iter()
            .filter(|&(x, y, u)| {
                let dm = d[self.units[u].id];
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
        let mut order: Vec<&Sprite> = self.sprites.iter().filter(|s| (s.x - vx).abs() < vw + 256 && (s.y - vy).abs() < vh + 256).collect();
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
    for s in &scene.sprites {
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
    let mut x = 440;
    for (icon, text) in [(0, "0"), (2, "0"), (5, "0/0")] {
        ui.draw_icon(icon, x, 2, pal, out, w, h);
        ui.font.draw(text, x + 16, 4, col, pal, out, w, h);
        x += 66;
    }
    // Selected unit panel.
    if let [u] = scene.selected[..] {
        let mu = &scene.units[u];
        crate::ui::draw_grp_rgb(&ui.wireframe, mu.id, 170, 390, pal, out, w, h);
        let name = ui.stat_txt.get(mu.id).map_or("", |s| s.split('\0').next().unwrap_or(""));
        ui.font.draw(name, 260, 392, ui.text_color(2), pal, out, w, h);
        let max = scene.a.dat.units.hit_points[mu.id] >> 8;
        if max > 0 {
            let hp = format!("{}/{}", mu.hp.div_ceil(256), max);
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
    if let Some((tick, cx, cy)) = cursor {
        ui.draw_cursor(tick, cx, cy, pal, out, w, h);
    }
}

pub fn run(a: &Assets, chk: Chk) -> io::Result<()> {
    use minifb::{Key, Scale, Window, WindowOptions};
    let mut scene = Scene::new(a, chk)?;
    let ui = crate::ui::Ui::load(&a.casc, "terran").ok();
    let mut audio = crate::audio::Audio::open(&a.casc);
    if let Some(au) = audio.as_mut() {
        au.play_music(&a.casc, "music/terran1.ogg");
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
                    let mut hits: Vec<usize> = scene.units_in_rect(x0, y0, x1, y1).into_iter().filter(|&u| scene.units[u].owner == 0).collect();
                    hits.truncate(12);
                    hits
                };
                scene.selected = hits;
            }
        }
        if win.is_key_down(Key::Left) || win.is_key_down(Key::A) { vx -= sp }
        if win.is_key_down(Key::Right) || win.is_key_down(Key::D) { vx += sp }
        if win.is_key_down(Key::Up) || win.is_key_down(Key::W) { vy -= sp }
        if win.is_key_down(Key::Down) || win.is_key_down(Key::S) { vy += sp }
        vx = vx.clamp(0, (mw - vw).max(0));
        vy = vy.clamp(0, (mh - vh + 120).max(0)); // console covers the bottom
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
