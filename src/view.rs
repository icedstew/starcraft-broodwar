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

pub struct Scene<'a> {
    pub a: &'a Assets,
    pub chk: Chk,
    pub ts: Tileset,
    pub terrain: (usize, usize, Vec<u8>),
    pub sprites: Vec<Sprite>,
    pub rng: Rng,
    player_colors: [[u8; 8]; 12],
    shadow: [u8; 256],
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
        // ponytail: shadow = nearest palette color at ~half brightness; BW uses a tileset light table.
        let mut shadow = [0u8; 256];
        for (i, s) in shadow.iter_mut().enumerate() {
            let [r, g, b] = ts.palette[i].map(|c| c as i32 / 2);
            *s = (1..256)
                .min_by_key(|&j| {
                    let [r2, g2, b2] = ts.palette[j].map(|c| c as i32);
                    (r - r2).pow(2) + (g - g2).pow(2) + (b - b2).pow(2)
                })
                .unwrap_or(0) as u8;
        }
        let mut s = Scene { a, chk, ts, terrain, sprites: Vec::new(), rng: Rng { seed: 0 }, player_colors, shadow };
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

    pub fn step(&mut self) {
        let a = self.a;
        let frames = |id: u16| a.grp(id).map_or(0, |g| g.frame_count() as u16);
        let w = self.world(&frames);
        let mut fx = Vec::new();
        let rng = &mut self.rng;
        self.sprites.retain_mut(|s| s.step(&w, rng, &mut fx));
        for e in fx {
            if let Effect::SpawnSprite { sprite, x, y, elevation_delta } = e {
                let mut s = Sprite::new(&w, &mut self.rng, sprite, 11, x, y);
                s.elevation = s.elevation.saturating_add_signed(elevation_delta);
                self.sprites.push(s);
            }
        }
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

pub fn run(a: &Assets, chk: Chk) -> io::Result<()> {
    use minifb::{Key, Window, WindowOptions};
    let mut scene = Scene::new(a, chk)?;
    let (vw, vh) = (960i32, 640i32);
    let mut win = Window::new("scbw", vw as usize, vh as usize, WindowOptions::default()).map_err(|e| io::Error::other(e.to_string()))?;
    win.set_target_fps(24); // BW "fastest" = 42 ms/frame
    let (mw, mh) = (scene.terrain.0 as i32, scene.terrain.1 as i32);
    let (mut vx, mut vy) = (0, 0);
    let mut idx = vec![0u8; (vw * vh) as usize];
    let mut rgb = vec![0u32; (vw * vh) as usize];
    while win.is_open() && !win.is_key_down(Key::Escape) {
        let sp = 24;
        if win.is_key_down(Key::Left) || win.is_key_down(Key::A) { vx -= sp }
        if win.is_key_down(Key::Right) || win.is_key_down(Key::D) { vx += sp }
        if win.is_key_down(Key::Up) || win.is_key_down(Key::W) { vy -= sp }
        if win.is_key_down(Key::Down) || win.is_key_down(Key::S) { vy += sp }
        vx = vx.clamp(0, (mw - vw).max(0));
        vy = vy.clamp(0, (mh - vh).max(0));
        scene.step();
        scene.draw(vx, vy, vw, vh, &mut idx);
        let pal = scene.palette();
        for (o, &i) in rgb.iter_mut().zip(&idx) {
            let [r, g, b] = pal[i as usize];
            *o = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
        win.update_with_buffer(&rgb, vw as usize, vh as usize).map_err(|e| io::Error::other(e.to_string()))?;
    }
    Ok(())
}
