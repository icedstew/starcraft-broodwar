//! iscript.bin interpreter, images and sprites.
//!
//! Opcode set and operand sizes verified against the dispatch table in the x64
//! binary (interpreter 0x140190f38, 69-entry jump table at 0x1401922c0).

use crate::dat::Dat;
use std::collections::HashMap;

pub mod anim {
    pub const INIT: usize = 0;
    pub const DEATH: usize = 1;
    pub const GND_ATTK_INIT: usize = 2;
    pub const AIR_ATTK_INIT: usize = 3;
    pub const GND_ATTK_RPT: usize = 5;
    pub const AIR_ATTK_RPT: usize = 6;
    pub const CAST_SPELL: usize = 7;
    pub const GND_ATTK_TO_IDLE: usize = 8;
    pub const AIR_ATTK_TO_IDLE: usize = 9;
    pub const WALKING: usize = 11;
    pub const WALKING_TO_IDLE: usize = 12;
}

/// Number of animation entry points per script header type.
const ANIM_COUNT: [usize; 30] = [2, 2, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14, 14, 16, 16, 0, 0, 0, 0, 22, 22, 0, 24, 26, 0, 28, 28, 0, 28];

pub struct Iscript {
    pub data: Vec<u8>,
    headers: HashMap<u16, Vec<u16>>,
}

impl Iscript {
    pub fn parse(data: Vec<u8>) -> Iscript {
        let mut headers = HashMap::new();
        let rd = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let mut o = rd(0) as usize;
        while o + 4 <= data.len() && rd(o) != 0xFFFF {
            let (id, h) = (rd(o), rd(o + 2) as usize);
            if &data[h..h + 4] == b"SCPE" {
                let n = ANIM_COUNT.get(data[h + 4] as usize).copied().unwrap_or(0);
                headers.insert(id, (0..n).map(|i| rd(h + 8 + i * 2)).collect());
            }
            o += 4;
        }
        Iscript { data, headers }
    }

    pub fn anim(&self, script: u16, anim: usize) -> Option<u16> {
        self.headers.get(&script)?.get(anim).copied().filter(|&p| p != 0)
    }
}

/// BW's synced LCG (0x14039aa10).
#[derive(Clone)]
pub struct Rng {
    pub seed: u32,
}

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x015A_4E35).wrapping_add(1);
        (self.seed >> 16) & 0x7FFF
    }

    pub fn between(&mut self, lo: u32, hi: u32) -> u32 {
        lo + ((self.next() * (hi - lo + 1)) >> 15)
    }
}

pub mod image_flags {
    pub const FLIPPED: u16 = 0x2;
    pub const HIDDEN: u16 = 0x40;
}

#[derive(Clone)]
pub struct Image {
    pub serial: u32,
    pub id: u16,
    pub flags: u16,
    pub x: i8,
    pub y: i8,
    pub frameset: u16,
    pub frame: u16,
    pub draw_fn: u8,
    pub script: u16,
    pub pc: u16,
    pub ret: u16,
    pub anim: u8,
    pub wait: u8,
    pub dead: bool,
}

impl Image {
    pub fn flipped(&self) -> bool {
        self.flags & image_flags::FLIPPED != 0
    }
}

#[derive(Clone)]
pub struct Sprite {
    pub id: u16,
    pub owner: u8,
    pub x: i32,
    pub y: i32,
    pub elevation: u8,
    /// Back-to-front draw order.
    pub images: Vec<Image>,
    pub main: usize,
    next_serial: u32,
    /// Facing, 0..256 (0 = up, clockwise). Lives on the flingy in BW; kept here until units exist.
    pub direction: u8,
    /// Index into Scene::units when this sprite is a unit's main sprite.
    pub unit: Option<usize>,
}

/// Things an image script asks for that reach outside its own image.
pub enum Effect {
    SpawnSprite { sprite: u16, x: i32, y: i32, elevation_delta: i8 },
    Sound { id: u16, x: i32, y: i32 },
}

pub struct World<'a> {
    pub dat: &'a Dat,
    pub iscript: &'a Iscript,
    pub grp_frames: &'a dyn Fn(u16) -> u16,
    pub tileset: u16,
}

impl Sprite {
    pub fn new(w: &World, rng: &mut Rng, id: u16, owner: u8, x: i32, y: i32) -> Sprite {
        let mut s = Sprite { id, owner, x, y, elevation: 4, images: Vec::new(), main: 0, next_serial: 0, direction: 0, unit: None };
        s.add_image(w, rng, w.dat.sprites.image[id as usize], 0, 0, None);
        s
    }

    /// Insert image `id`; `above = None` means "as the main image".
    pub fn add_image(&mut self, w: &World, rng: &mut Rng, id: u16, x: i8, y: i8, rel: Option<(usize, bool)>) -> usize {
        let im = &w.dat.images;
        self.next_serial += 1;
        let mut img = Image {
            serial: self.next_serial,
            id,
            flags: 0,
            x,
            y,
            frameset: 0,
            frame: 0,
            draw_fn: im.draw_function[id as usize],
            script: im.iscript[id as usize] as u16,
            pc: 0,
            ret: 0,
            anim: 0,
            wait: 0,
            dead: false,
        };
        img.pc = w.iscript.anim(img.script, anim::INIT).unwrap_or(0);
        let idx = match rel {
            None => {
                self.images.push(img);
                self.main = self.images.len() - 1;
                self.main
            }
            Some((at, above)) => {
                let i = if above { at + 1 } else { at };
                if self.main >= i {
                    self.main += 1;
                }
                self.images.insert(i, img);
                i
            }
        };
        self.update_frame(w, idx);
        let mut fx = Vec::new();
        self.run_image(w, rng, idx, &mut fx);
        idx
    }

    fn update_frame(&mut self, w: &World, i: usize) {
        let img = &mut self.images[i];
        if w.dat.images.gfx_turns[img.id as usize] != 0 {
            let d = (self.direction as u16 + 4) / 8 % 32;
            if d > 16 {
                img.frame = img.frameset + 32 - d;
                img.flags |= image_flags::FLIPPED;
            } else {
                img.frame = img.frameset + d;
                img.flags &= !image_flags::FLIPPED;
            }
        } else {
            img.frame = img.frameset;
        }
    }

    pub fn set_anim(&mut self, w: &World, rng: &mut Rng, anim: usize, fx: &mut Vec<Effect>) {
        for i in 0..self.images.len() {
            if let Some(pc) = w.iscript.anim(self.images[i].script, anim) {
                let img = &mut self.images[i];
                img.pc = pc;
                img.anim = anim as u8;
                img.wait = 0;
                self.run_image(w, rng, i, fx);
            }
        }
    }

    /// Advance every image one frame. Returns false when the sprite has no images left.
    pub fn step(&mut self, w: &World, rng: &mut Rng, fx: &mut Vec<Effect>) -> bool {
        // Images created this frame already ran their first step in add_image.
        let serials: Vec<u32> = self.images.iter().map(|m| m.serial).collect();
        for s in serials {
            if let Some(i) = self.images.iter().position(|m| m.serial == s) {
                self.run_image(w, rng, i, fx);
            }
        }
        let main = self.images.get(self.main).map(|m| m.serial);
        self.images.retain(|m| !m.dead);
        self.main = main.and_then(|s| self.images.iter().position(|m| m.serial == s)).unwrap_or(0);
        !self.images.is_empty()
    }

    fn run_image(&mut self, w: &World, rng: &mut Rng, i: usize, fx: &mut Vec<Effect>) {
        if self.images[i].dead {
            return;
        }
        if self.images[i].wait > 0 {
            self.images[i].wait -= 1;
            return;
        }
        let d = &w.iscript.data;
        let u8_at = |p: usize| d.get(p).copied().unwrap_or(0);
        let i8_at = |p: usize| u8_at(p) as i8;
        let u16_at = |p: usize| u16::from_le_bytes([u8_at(p), u8_at(p + 1)]);
        let mut i = i;
        for _ in 0..1000 {
            // ponytail: step cap guards against malformed infinite loops; BW has none.
            let mut pc = self.images[i].pc as usize;
            let op = u8_at(pc);
            pc += 1;
            macro_rules! img {
                () => {
                    self.images[i]
                };
            }
            match op {
                0x00 => {
                    img!().frameset = u16_at(pc);
                    pc += 2;
                    img!().pc = pc as u16;
                    self.update_frame(w, i);
                    continue;
                }
                0x01 => {
                    let f = u16_at(pc) + w.tileset;
                    pc += 2;
                    if f < (w.grp_frames)(img!().id) {
                        img!().frameset = f;
                        img!().pc = pc as u16;
                        self.update_frame(w, i);
                        continue;
                    }
                }
                0x02 => {
                    img!().x = i8_at(pc);
                    pc += 1;
                }
                0x03 => {
                    img!().y = i8_at(pc);
                    pc += 1;
                }
                0x04 => {
                    img!().x = i8_at(pc);
                    img!().y = i8_at(pc + 1);
                    pc += 2;
                }
                0x05 => {
                    img!().wait = u8_at(pc).saturating_sub(1);
                    img!().pc = (pc + 1) as u16;
                    return;
                }
                0x06 => {
                    let (a, b) = (u8_at(pc) as u32, u8_at(pc + 1) as u32);
                    img!().wait = (rng.between(a, b.max(a)) as u8).saturating_sub(1);
                    img!().pc = (pc + 2) as u16;
                    return;
                }
                0x07 => pc = u16_at(pc) as usize,
                // imgol, imgul, imgolorig, imgoluselo, imguluselo, imgulnextid
                0x08 | 0x09 | 0x0a | 0x0d | 0x0e | 0x3d => {
                    let (id, x, y, len) = match op {
                        0x0a => (u16_at(pc), 0, 0, 2),
                        0x0d | 0x0e => (u16_at(pc), 0, 0, 4), // ponytail: LO overlay offsets not loaded yet
                        0x3d => (img!().id + 1, i8_at(pc), i8_at(pc + 1), 2),
                        _ => (u16_at(pc), i8_at(pc + 2), i8_at(pc + 3), 4),
                    };
                    pc += len;
                    img!().pc = pc as u16;
                    let above = matches!(op, 0x08 | 0x0a | 0x0d);
                    let (px, py) = (img!().x, img!().y);
                    let n = self.add_image(w, rng, id, px.wrapping_add(x), py.wrapping_add(y), Some((i, above)));
                    if n <= i {
                        i += 1;
                    }
                    continue;
                }
                0x0b => {
                    // switchul: replace the underlay directly below with another image id.
                    let id = u16_at(pc);
                    pc += 2;
                    if i > 0 {
                        self.images[i - 1].id = id;
                    }
                }
                0x0c | 0x2d | 0x3e | 0x43 => {}
                // sprol, highsprol, lowsprul, spruluselo, sprul, grdsprol
                0x0f | 0x10 | 0x11 | 0x13 | 0x14 | 0x42 => {
                    let (s, x, y) = (u16_at(pc), i8_at(pc + 2) as i32, i8_at(pc + 3) as i32);
                    pc += 4;
                    let ed = match op {
                        0x10 => 1,
                        0x11 | 0x14 => -1,
                        _ => 0,
                    };
                    let (ix, iy) = (img!().x as i32, img!().y as i32);
                    fx.push(Effect::SpawnSprite { sprite: s, x: self.x + ix + x, y: self.y + iy + y, elevation_delta: ed });
                }
                0x12 => pc += 2, // uflunstable
                0x15 => {
                    // sproluselo
                    let s = u16_at(pc);
                    pc += 3;
                    fx.push(Effect::SpawnSprite { sprite: s, x: self.x, y: self.y, elevation_delta: 0 });
                }
                0x16 => {
                    img!().dead = true;
                    return;
                }
                0x17 => {
                    let f = u8_at(pc) != 0;
                    pc += 1;
                    if f {
                        img!().flags |= image_flags::FLIPPED
                    } else {
                        img!().flags &= !image_flags::FLIPPED
                    }
                }
                0x18 => {
                    fx.push(Effect::Sound { id: u16_at(pc), x: self.x, y: self.y });
                    pc += 2;
                }
                0x19 | 0x1c => {
                    let n = u8_at(pc) as usize;
                    if n > 0 {
                        let k = rng.next() as usize % n;
                        fx.push(Effect::Sound { id: u16_at(pc + 1 + k * 2), x: self.x, y: self.y });
                    }
                    pc += 1 + n * 2;
                }
                0x1a => {
                    let (a, b) = (u16_at(pc) as u32, u16_at(pc + 2) as u32);
                    fx.push(Effect::Sound { id: rng.between(a, b.max(a)) as u16, x: self.x, y: self.y });
                    pc += 4;
                }
                0x1b | 0x26 | 0x27 | 0x2a | 0x2e | 0x2f | 0x44 => {} // unit-side ops (no units yet)
                0x1d => {
                    // followmaingraphic
                    if let Some(m) = self.images.get(self.main).cloned() {
                        img!().frame = m.frame;
                        img!().frameset = m.frameset;
                        img!().flags = (img!().flags & !image_flags::FLIPPED) | (m.flags & image_flags::FLIPPED);
                    }
                }
                0x1e => {
                    let (chance, target) = (u8_at(pc) as u32, u16_at(pc + 1) as usize);
                    pc += 3;
                    if rng.next() & 0xFF <= chance {
                        pc = target;
                    }
                }
                0x1f => {
                    self.direction = self.direction.wrapping_sub(u8_at(pc).wrapping_mul(8));
                    pc += 1;
                    img!().pc = pc as u16;
                    self.update_all_frames(w);
                    continue;
                }
                0x20 => {
                    self.direction = self.direction.wrapping_add(u8_at(pc).wrapping_mul(8));
                    pc += 1;
                    img!().pc = pc as u16;
                    self.update_all_frames(w);
                    continue;
                }
                0x21 => {
                    self.direction = self.direction.wrapping_add(8);
                    img!().pc = pc as u16;
                    self.update_all_frames(w);
                    continue;
                }
                0x22 => {
                    let n = u8_at(pc).wrapping_mul(8);
                    pc += 1;
                    self.direction = if rng.next() % 4 == 1 { self.direction.wrapping_sub(n) } else { self.direction.wrapping_add(n) };
                    img!().pc = pc as u16;
                    self.update_all_frames(w);
                    continue;
                }
                0x23 | 0x24 | 0x25 | 0x28 | 0x29 | 0x2b | 0x31 | 0x38 | 0x41 => pc += 1, // unit-side, 1 byte arg
                0x2c => {
                    // engset: frame block n of the main image
                    let n = u8_at(pc) as u16;
                    pc += 1;
                    if let Some(m) = self.images.get(self.main) {
                        let per = (w.grp_frames)(m.id);
                        img!().frameset = m.frame + per * n;
                        img!().frame = img!().frameset;
                    }
                }
                0x30 => {
                    // ignorerest: park on this opcode until the animation changes.
                    img!().pc = (pc - 1) as u16;
                    return;
                }
                0x32 => img!().flags |= image_flags::HIDDEN,
                0x33 => img!().flags &= !image_flags::HIDDEN,
                0x34 => {
                    self.direction = u8_at(pc).wrapping_mul(8);
                    pc += 1;
                    img!().pc = pc as u16;
                    self.update_all_frames(w);
                    continue;
                }
                0x35 => {
                    img!().ret = (pc + 2) as u16;
                    pc = u16_at(pc) as usize;
                }
                0x36 => pc = img!().ret as usize,
                0x37 | 0x39 | 0x3f | 0x40 => pc += 2, // setflspeed, pwrupcondjmp, liftoffcondjmp, warpoverlay
                0x3a => pc += 4,          // trgtrangecondjmp
                0x3b | 0x3c => pc += 6,   // trgtarccondjmp, curdirectcondjmp
                _ => {
                    img!().dead = true; // unknown opcode: BW would crash; drop the image instead
                    return;
                }
            }
            img!().pc = pc as u16;
        }
    }

    pub fn set_direction(&mut self, w: &World, d: u8) {
        if self.direction != d {
            self.direction = d;
            self.update_all_frames(w);
        }
    }

    fn update_all_frames(&mut self, w: &World) {
        for i in 0..self.images.len() {
            self.update_frame(w, i);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rng_matches_bw_lcg() {
        let mut r = super::Rng { seed: 0 };
        // seed1 = 1 -> 0; seed2 = 0x15A4E36 -> 0x15A
        assert_eq!(r.next(), 0);
        assert_eq!(r.next(), 0x015A);
    }
}
