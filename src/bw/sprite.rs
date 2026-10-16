//! Sprites, images and thingies: faithful port of SC:R's routines.
//!
//! x64 struct offsets are noted on fields; function addresses on functions.

use super::list::{self, Link, List, Node, NIL};
use super::{Data, State};

pub const MAX_IMAGES: usize = 5000;
pub const MAX_SPRITES: usize = 2500;
pub const MAX_THINGIES: usize = 500;

pub mod iflag {
    pub const REDRAW: u16 = 0x1;
    pub const FLIPPED: u16 = 0x2;
    pub const FREEZE_Y: u16 = 0x4;
    pub const HAS_DIRECTIONAL_FRAMES: u16 = 0x8;
    pub const HAS_ISCRIPT_ANIMATIONS: u16 = 0x10;
    pub const CLICKABLE: u16 = 0x20;
    pub const HIDDEN: u16 = 0x40;
    pub const USES_SPECIAL_OFFSET: u16 = 0x80;
}

pub mod sflag {
    pub const HIDDEN: u8 = 0x20;
}

/// iscript execution state (image +0x18).
#[derive(Clone, Copy, Default, Debug)]
pub struct IscriptState {
    pub header: u16,   // +0x18
    pub pc: u16,       // +0x1a
    pub ret: u16,      // +0x1c
    pub anim: u8,      // +0x1e
    pub wait: u8,      // +0x1f
}

#[derive(Clone, Default, Debug)]
pub struct Image {
    pub link: Link,
    pub image_id: u16,    // +0x10
    pub modifier: u8,     // +0x12 draw function
    pub direction: u8,    // +0x13 frame direction 0..=16
    pub flags: u16,       // +0x14
    pub x: i8,            // +0x16
    pub y: i8,            // +0x17
    pub iscript: IscriptState,
    pub frameset: u16,    // +0x20
    pub frame: u16,       // +0x22
    /// +0x38 grp pointer; nonzero while the image is in use.
    pub in_use: bool,
    /// +0x40 modifier data: player color, remap table id, cloak state...
    pub coloring: u64,
    pub sprite: u32,      // +0x50
}

impl Node for Image {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

#[derive(Clone, Default, Debug)]
pub struct Sprite {
    pub link: Link,
    pub sprite_id: u16,       // +0x10
    pub owner: u8,            // +0x12
    pub selection_index: u8,  // +0x13
    pub visibility: u8,       // +0x14
    pub elevation: u8,        // +0x15
    pub flags: u8,            // +0x16
    pub selection_timer: u8,  // +0x17
    pub width: u8,            // +0x1a
    pub height: u8,           // +0x1b
    pub x: i16,               // +0x28 (stored encrypted in SC:R)
    pub y: i16,
    pub main_image: u32,      // +0x30
    pub images: List,         // +0x38 head, +0x40 tail
}

impl Node for Sprite {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

#[derive(Clone, Default, Debug)]
pub struct Thingy {
    pub link: Link,
    pub hp: i32,     // +0x10
    pub sprite: u32, // +0x18
}

impl Node for Thingy {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

/// Pop the head of a free list (images: 1.16.1 0x4d4e30 / SC:R 0x14018dfe0).
pub fn pop<T: Node>(v: &mut [T], l: &mut List) -> Option<u32> {
    let h = l.head;
    if h == NIL {
        return None;
    }
    list::remove(v, h, l);
    Some(h)
}

impl State {
    pub fn sprite_row(&self, s: u32) -> usize {
        // 0x14019ba50: y / 32, clamped to the map's tile rows.
        let y = self.sprites[s as usize].y as i32;
        let row = y.div_euclid(32).max(0) as usize;
        row.min(self.map_tiles.1.saturating_sub(1))
    }

    /// 0x4990f0 / SC:R 0x140197270 create_sprite: off the free list, initialized and put
    /// in its tile row.
    pub fn create_sprite(&mut self, d: &Data, sprite_id: u16, x: i16, y: i16, owner: u8) -> Option<u32> {
        let s = pop(&mut self.sprites, &mut self.free_sprites)?;
        if !self.initialize_sprite(d, s, sprite_id, x, y, owner) {
            list::insert_after_head(&mut self.sprites, s, &mut self.free_sprites);
            return None;
        }
        let row = self.sprite_row(s);
        if row >= 0x100 {
            return None;
        }
        let mut l = self.sprite_rows[row];
        list::insert_after_head(&mut self.sprites, s, &mut l);
        self.sprite_rows[row] = l;
        Some(s)
    }

    /// 0x499030 / SC:R 0x14019ab60 initialize_sprite.
    fn initialize_sprite(&mut self, d: &Data, s: u32, sprite_id: u16, x: i16, y: i16, owner: u8) -> bool {
        if x as u16 >= self.map_px.0 || y as u16 >= self.map_px.1 {
            return false;
        }
        let sp = &mut self.sprites[s as usize];
        sp.owner = owner;
        sp.sprite_id = sprite_id;
        sp.flags = 0;
        sp.x = x;
        sp.y = y;
        sp.visibility = 0xff;
        sp.elevation = 4;
        sp.selection_timer = 0;
        sp.main_image = NIL;
        sp.images = List::default();
        if d.dat.sprites.visible[sprite_id as usize] == 0 {
            sp.flags |= sflag::HIDDEN;
            sp.visibility = 0;
        }
        let image_id = d.dat.sprites.image[sprite_id as usize];
        let Some(img) = self.create_image(d, s, NIL, image_id, 0, 0, 0) else { return false };
        let (w, h) = d.grp_size(self.images[img as usize].image_id);
        let sp = &mut self.sprites[s as usize];
        sp.width = w.min(255) as u8;
        sp.height = h.min(255) as u8;
        true
    }

    /// 0x498e00 (with `rel` = the main image) / SC:R 0x140197090 create_image: the first
    /// image becomes the main image; later ones go before `rel`.
    pub fn create_image(&mut self, d: &Data, s: u32, rel: u32, image_id: u16, x: i8, y: i8, dir: u8) -> Option<u32> {
        let img = pop(&mut self.images, &mut self.free_images)?;
        let mut l = self.sprites[s as usize].images;
        if l.head == NIL {
            self.sprites[s as usize].main_image = img;
            list::insert_after_head(&mut self.images, img, &mut l);
        } else {
            list::insert_before(&mut self.images, rel, img, &mut l);
        }
        self.sprites[s as usize].images = l;
        self.initialize_image(d, img, image_id, s, x, y);
        self.image_post_init(d, img);
        self.image_set_direction32(d, img, dir);
        Some(img)
    }

    /// 0x4d5a50 / SC:R 0x1401900c0 initialize_image.
    pub(crate) fn initialize_image(&mut self, d: &Data, img: u32, image_id: u16, s: u32, x: i8, y: i8) {
        let id = image_id as usize;
        let im = &d.dat.images;
        let owner = self.sprites[s as usize].owner;
        let i = &mut self.images[img as usize];
        i.image_id = image_id;
        i.in_use = true;
        i.flags = ((im.gfx_turns[id] & 1) as u16) << 3 | ((im.clickable[id] & 1) as u16) << 5;
        i.frameset = 0;
        i.frame = 0;
        i.y = y;
        i.direction = 0;
        i.sprite = s;
        i.x = x;
        i.coloring = 0;
        i.iscript = IscriptState::default();
        if im.draw_function[id] == 0xe {
            i.coloring = owner as u64;
        }
        if im.draw_function[id] == 9 {
            i.coloring = im.remapping[id] as u64;
        }
    }

    /// 0x4d66b0 (draw function 0x4d58b0, header 0x4d6640) / SC:R 0x14018fea0: modifier,
    /// iscript header, Init animation.
    pub(crate) fn image_post_init(&mut self, d: &Data, img: u32) {
        let id = self.images[img as usize].image_id as usize;
        let i = &mut self.images[img as usize];
        i.modifier = d.dat.images.draw_function[id];
        if i.modifier == 0x11 {
            i.coloring = 0x230;
        }
        i.flags |= iflag::REDRAW;
        i.flags = (i.flags & !iflag::HAS_ISCRIPT_ANIMATIONS) | (((d.dat.images.full_iscript[id] & 1) as u16) << 4);
        // 0x14018e5c0: look up the iscript header for images.dat iscript id.
        let script = d.dat.images.iscript[id];
        i.iscript.header = d.iscript_header(script).unwrap_or_else(|| panic!("script {script} does not exist"));
        self.image_run_anim(d, img, 0);
        // 0x14018e3f0 only refreshes the image's screen-space bounds (render cache).
    }

    fn set_frame_from_direction(&mut self, img: u32) {
        let i = &mut self.images[img as usize];
        let f = i.frameset.wrapping_add(i.direction as u16);
        if i.frame != f {
            i.frame = f;
        }
    }

    /// 0x4d5ea0 / SC:R 0x14018ea00: set direction from a 0..=31 direction index.
    pub fn image_set_direction32(&mut self, d: &Data, img: u32, dir: u8) {
        let i = &self.images[img as usize];
        if i.flags & iflag::HAS_DIRECTIONAL_FRAMES == 0 {
            return;
        }
        let fdir = if dir < 0x11 { dir } else { 0x20u8.wrapping_sub(dir) };
        let flip = dir > 0x10;
        if (i.flags & iflag::FLIPPED != 0) == flip && i.direction == fdir {
            return;
        }
        let i = &mut self.images[img as usize];
        i.direction = fdir;
        if i.modifier == 0x11 {
            i.coloring = 0x230;
        }
        i.flags = (i.flags & !iflag::FLIPPED) | if flip { iflag::FLIPPED } else { 0 } | iflag::REDRAW;
        self.set_frame_from_direction(img);
        if self.images[img as usize].flags & iflag::USES_SPECIAL_OFFSET != 0 {
            self.image_update_special_offset(d, img);
        }
    }

    /// 0x4d5f80 / SC:R 0x1401927f0: set direction from a 0..=255 heading.
    pub fn image_set_direction(&mut self, d: &Data, img: u32, heading: u8) {
        if self.images[img as usize].flags & iflag::USES_SPECIAL_OFFSET != 0 {
            self.image_update_special_offset(d, img);
        }
        let i = &self.images[img as usize];
        if i.flags & iflag::HAS_DIRECTIONAL_FRAMES == 0 {
            return;
        }
        let d32 = ((heading as u32 + 4) >> 3) as u8;
        let fdir = if d32 < 0x11 { d32 } else { 0x20u8.wrapping_sub(d32) };
        let flip = d32 > 0x10;
        if i.direction == fdir && (i.flags & iflag::FLIPPED != 0) == flip {
            return;
        }
        let i = &mut self.images[img as usize];
        i.direction = fdir;
        if i.modifier == 0x11 {
            i.coloring = 0x230;
        }
        i.flags = (i.flags & !iflag::FLIPPED) | if flip { iflag::FLIPPED } else { 0 } | iflag::REDRAW;
        self.set_frame_from_direction(img);
    }

    /// 0x401df0: LO offset of kind `kind` (entry `idx`) of sprite `s`'s main image, x
    /// negated when the image is flipped.
    pub fn image_lo_offset(&self, d: &Data, s: u32, kind: usize, idx: i8) -> (i8, i8) {
        let im = &self.images[self.sprites[s as usize].main_image as usize];
        let (ox, oy) = d.lo_offset(kind, im.image_id, im.frame, idx).unwrap_or((0, 0));
        (if im.flags & iflag::FLIPPED != 0 { ox.wrapping_neg() } else { ox }, oy)
    }

    /// 0x401e40: set the offset of sprite `s`'s main image; redraw only if it changed.
    pub fn sprite_set_main_offset(&mut self, s: u32, x: i8, y: i8) {
        let mi = self.sprites[s as usize].main_image;
        let si = &mut self.images[mi as usize];
        if si.x != x || si.y != y {
            si.flags |= iflag::REDRAW;
            si.x = x;
            si.y = y;
        }
    }

    /// 0x4d5a00 / SC:R 0x140190e00: follow the main image's special-overlay (LO) offset.
    pub fn image_update_special_offset(&mut self, d: &Data, img: u32) {
        let s = self.images[img as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        let (mid, mframe, mflip) = {
            let m = &self.images[main as usize];
            (m.image_id, m.frame, m.flags & iflag::FLIPPED != 0)
        };
        let Some((mut x, y)) = d.lo_offset(2, mid, mframe, 0) else { return };
        if mflip {
            x = -x;
        }
        let i = &mut self.images[img as usize];
        if i.x != x || i.y != y {
            i.flags |= iflag::REDRAW;
            i.x = x;
            i.y = y;
        }
    }

    /// 0x4d8470 / SC:R 0x1401928e0: start animation `anim` (with BW's attack-repeat fallbacks and no-restart rules).
    pub fn image_run_anim(&mut self, d: &Data, img: u32, anim: u8) {
        let cur = self.images[img as usize].iscript.anim;
        let flags = self.images[img as usize].flags;
        if anim == 1 && cur == 1 {
            return; // already dying
        }
        if flags & iflag::HAS_ISCRIPT_ANIMATIONS == 0 && anim >= 2 {
            return;
        }
        if anim == cur && (anim == 11 || anim == 19) {
            return; // Walking / IsWorking don't restart
        }
        let anim = match anim {
            5 if cur != 5 && cur != 2 => 2,
            6 if cur != 6 && cur != 3 => 3,
            a => a,
        };
        let header = self.images[img as usize].iscript.header as usize;
        let pc = d.iscript_u16(header + 8 + anim as usize * 2);
        let st = &mut self.images[img as usize].iscript;
        st.anim = anim;
        st.pc = pc;
        st.wait = 0;
        st.ret = 0;
        super::iscript::run(self, d, img, false, None);
    }

    /// 0x499d00 / SC:R 0x14019c7d0: run `anim` on every image of a sprite (unless the
    /// sprite opts out).
    pub fn sprite_run_anim(&mut self, d: &Data, s: u32, anim: u8, force: bool) {
        let mut cur = self.sprites[s as usize].images.head;
        while cur != NIL {
            let next = self.images[cur as usize].link.next;
            if force || (self.sprites[s as usize].flags as i8) >= 0 {
                self.image_run_anim(d, cur, anim);
            }
            cur = next;
        }
    }

    /// 0x498c30 / SC:R 0x14019c6f0: set every image's direction from a heading.
    pub fn sprite_set_direction(&mut self, d: &Data, s: u32, heading: u8) {
        let mut cur = self.sprites[s as usize].images.head;
        while cur != NIL {
            let next = self.images[cur as usize].link.next;
            self.image_set_direction(d, cur, heading);
            cur = next;
        }
    }

    /// 0x140190b40: overlay image above (in front of) or below `img` in its sprite.
    pub fn create_overlay(&mut self, d: &Data, img: u32, image_id: u16, x: i8, y: i8, above: bool) -> Option<u32> {
        let new = pop(&mut self.images, &mut self.free_images)?;
        let s = self.images[img as usize].sprite;
        let mut l = self.sprites[s as usize].images;
        if above {
            list::insert_before(&mut self.images, img, new, &mut l);
        } else {
            list::insert_after(&mut self.images, img, new, &mut l);
        }
        self.sprites[s as usize].images = l;
        self.initialize_image(d, new, image_id, s, x, y);
        self.image_post_init(d, new);
        if let Some(u) = self.iscript_unit {
            // Hallucinations are tinted only for their owner (or when everything is revealed).
            let owner = self.host_unit_owner(u);
            if self.images[new as usize].modifier == 0
                && self.host_is_hallucination(u)
                && ((!self.replay && owner == self.local_player) || self.reveal_all)
            {
                let i = &mut self.images[new as usize];
                i.flags |= iflag::REDRAW;
                i.modifier = 0x10;
                i.coloring = 0;
            }
        }
        let flags = self.images[new as usize].flags;
        if flags & iflag::HAS_DIRECTIONAL_FRAMES != 0 {
            let p = &self.images[img as usize];
            let pd = if p.flags & iflag::FLIPPED != 0 { 0x20u8.wrapping_sub(p.direction) } else { p.direction };
            self.image_set_direction32(d, new, pd);
        }
        self.set_frame_from_direction(new);
        if let Some(u) = self.iscript_unit {
            if self.host_is_cloaked(u) {
                let main = self.sprites[s as usize].main_image;
                if d.dat.images.draw_if_cloaked[image_id as usize] == 0 {
                    self.images[new as usize].flags |= iflag::HIDDEN;
                } else if self.images[new as usize].modifier == 0 {
                    let (mm, mc) = (self.images[main as usize].modifier, self.images[main as usize].coloring);
                    if (2..8).contains(&mm) {
                        let i = &mut self.images[new as usize];
                        i.modifier = mm;
                        if mm == 0x11 {
                            i.coloring = 0x230;
                        }
                        i.flags |= iflag::REDRAW;
                        i.coloring = (i.coloring & !0xffff) | (mc & 0xffff);
                    }
                }
            }
        }
        Some(new)
    }

    /// 0x498d70: image `image_id` right below sprite `s`'s main image (outside iscript,
    /// so no hallucination / cloak inheritance).
    pub fn add_image_below_main(&mut self, d: &Data, s: u32, image_id: u16) -> Option<u32> {
        let main = self.sprites[s as usize].main_image;
        let saved = self.iscript_unit.take();
        let r = self.create_overlay(d, main, image_id, 0, 0, false);
        self.iscript_unit = saved;
        r
    }

    /// 0x498f40: image `image_id` on top of sprite `s` (first in its list; the main image
    /// when the sprite has none), without starting its iscript.
    fn add_image_on_top(&mut self, d: &Data, s: u32, image_id: u16, x: i8, y: i8, dir: u8) -> Option<u32> {
        let img = pop(&mut self.images, &mut self.free_images)?;
        let mut l = self.sprites[s as usize].images;
        if l.head == NIL {
            self.sprites[s as usize].main_image = img;
            list::insert_after_head(&mut self.images, img, &mut l);
        } else {
            list::insert_before(&mut self.images, l.head, img, &mut l);
        }
        self.sprites[s as usize].images = l;
        self.initialize_image(d, img, image_id, s, x, y);
        // 0x4d58b0 / 0x4d6640: draw function and iscript header; no Init animation.
        let id = image_id as usize;
        let i = &mut self.images[img as usize];
        i.modifier = d.dat.images.draw_function[id];
        if i.modifier == 0x11 {
            i.coloring = 0x230;
        }
        i.flags |= iflag::REDRAW;
        i.flags = (i.flags & !iflag::HAS_ISCRIPT_ANIMATIONS) | (((d.dat.images.full_iscript[id] & 1) as u16) << 4);
        let script = d.dat.images.iscript[id];
        i.iscript.header = d.iscript_header(script).unwrap_or_else(|| panic!("script {script} does not exist"));
        self.image_set_direction32(d, img, dir);
        Some(img)
    }

    /// 0x499210: a still copy of sprite `src` (same frames and flips; health bar and
    /// selection circle images left out). None, with nothing left behind, if the pools run out.
    pub(super) fn copy_sprite(&mut self, d: &Data, src: u32) -> Option<u32> {
        let sp = &self.sprites[src as usize];
        let s = self.create_sprite(d, sp.sprite_id, sp.x, sp.y, sp.owner)?;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.destroy_image(i);
            i = next;
        }
        self.sprites[s as usize].main_image = NIL;
        let mut i = self.sprites[src as usize].images.tail;
        while i != NIL {
            let o = self.images[i as usize].clone();
            if o.modifier != 0xb && o.modifier != 0xd {
                let Some(n) = self.add_image_on_top(d, s, o.image_id, o.x, o.y, o.direction) else {
                    self.free_sprite_images(s);
                    return None;
                };
                let im = &mut self.images[n as usize];
                im.frameset = o.frameset;
                im.frame = o.frame;
                let flip = o.flags & iflag::FLIPPED;
                if im.flags & iflag::FLIPPED != flip {
                    // The draw routines (0x512514 / 0x5125a4) follow the flip (render only).
                    im.flags = (im.flags & !iflag::FLIPPED) | flip | iflag::REDRAW;
                    if im.flags & iflag::USES_SPECIAL_OFFSET != 0 {
                        self.image_update_special_offset(d, n);
                    }
                }
                // 0x4d57b0: screen position (render only).
                if i == self.sprites[src as usize].main_image {
                    self.sprites[s as usize].main_image = n;
                }
            }
            i = self.images[i as usize].link.prev;
        }
        Some(s)
    }

    /// 0x14019fb00 create_thingy: a free-standing sprite (dust, effects).
    pub fn create_thingy(&mut self, d: &Data, sprite_id: u16, x: i16, y: i16, owner: u8) -> Option<u32> {
        let t = pop(&mut self.thingies, &mut self.free_thingies)?;
        self.thingies[t as usize].hp = 1;
        match self.create_sprite(d, sprite_id, x, y, owner) {
            Some(s) => {
                self.thingies[t as usize].sprite = s;
                list::insert_after_head(&mut self.thingies, t, &mut self.active_thingies);
                Some(t)
            }
            None => {
                self.thingies[t as usize].sprite = NIL;
                list::insert_after_head(&mut self.thingies, t, &mut self.free_thingies);
                None
            }
        }
    }

    /// 0x140190d70: spawn a thingy sprite at an image's position (sprol/lowsprul/...).
    pub fn create_sprite_at_image(&mut self, d: &Data, img: u32, sprite_id: u16, dx: i32, dy: i32, elevation: u8) -> Option<u32> {
        let i = &self.images[img as usize];
        let s = &self.sprites[i.sprite as usize];
        let x = s.x as i32 + i.x as i32 + dx;
        let y = s.y as i32 + i.y as i32 + dy;
        let t = self.create_thingy(d, sprite_id, x as i16, y as i16, 0)?;
        let ts = self.thingies[t as usize].sprite;
        self.sprites[ts as usize].elevation = elevation;
        // 0x14019ffb0 refreshes fog visibility for the new sprite (render/vision only).
        Some(ts)
    }

    /// 0x4d4ce0 (iscript `end`, 0x16): unlink the image and return it to the free list
    /// (its screen area is redrawn, 0x4970a0).
    pub fn destroy_image(&mut self, img: u32) {
        let s = self.images[img as usize].sprite;
        let mut l = self.sprites[s as usize].images;
        list::remove(&mut self.images, img, &mut l);
        self.sprites[s as usize].images = l;
        self.images[img as usize].in_use = false;
        list::insert_after_head(&mut self.images, img, &mut self.free_images);
    }
}
