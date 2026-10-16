//! iscript interpreter: port of 0x140190f00 (69-opcode dispatch, table at 0x1401922c0).
//!
//! `no_side_effects` is BW's dry-run mode (used to predict movement); in it most
//! opcodes only advance the pc, but randcondjmp still draws from the RNG and
//! wait/waitrand/end don't stop execution, exactly like the original.

use super::list::NIL;
use super::sprite::iflag;
use super::{Data, State};

impl State {
    /// 0x494bd0 / SC:R 0x140188220: unsigned angular distance between two headings.
    pub fn angle_diff(a: u8, b: u8) -> u32 {
        let d = (b as u32).wrapping_sub(a as u32) & 0xff;
        if d < 0x81 { d } else { 0x100 - d }
    }

    /// 0x140188b20 set_unit_heading.
    pub fn set_unit_heading(&mut self, d: &Data, u: u32, h: u8) {
        let un = &mut self.units[u as usize];
        un.fl.velocity_dir_prev = h;
        un.fl.heading = h;
        un.fl.velocity_dir = h;
        let (dx, dy) = crate::game::flingy::direction_table()[h as usize];
        un.fl.velocity = ((dx * un.fl.speed) >> 8, (dy * un.fl.speed) >> 8);
        if un.fl.next_target_waypoint != un.fl.pos {
            un.fl.next_target_waypoint = un.fl.pos;
        }
        let s = un.sprite;
        self.sprite_set_direction(d, s, h);
    }

    /// 0x140188bb0 set next/current speed (iscript-driven movement).
    pub fn set_unit_next_speed(&mut self, u: u32, v: i32) {
        let fl = &mut self.units[u as usize].fl;
        fl.next_speed = v;
        if fl.speed != v {
            fl.speed = v;
            let (dx, dy) = crate::game::flingy::direction_table()[fl.velocity_dir as usize];
            fl.velocity = ((dx * v) >> 8, (dy * v) >> 8);
        }
    }

    /// 0x1401c8120: speed after stim / speed upgrade / ensnare.
    pub fn unit_modified_speed(&self, u: u32, speed: u32) -> u32 {
        let un = &self.units[u as usize];
        let mut k = (un.stim_timer != 0) as i32;
        if un.status & 0x1000_0000 != 0 {
            k += 1;
        }
        if un.ensnare_timer != 0 {
            k -= 1;
        }
        if k > 0 {
            // Scout, Mojo, Artanis get a fixed upgraded speed.
            if matches!(un.unit_type, 0x46 | 0x50 | 0x58) {
                return 0x6ab;
            }
            let s = speed + (speed >> 1);
            return if s < 0x355 { 0x355 } else { s };
        }
        if k < 0 { speed >> 1 } else { speed }
    }

    /// 0x140189e00 setspawnframe: aim the target waypoint 16px along `dir`.
    fn set_spawn_frame(&mut self, u: u32, dir: u8) {
        let fl = &mut self.units[u as usize].fl;
        let (dx, dy) = crate::game::flingy::direction_table()[dir as usize];
        let x = (((dx << 12) >> 8) + fl.exact.0) >> 8;
        let y = (((dy << 12) >> 8) + fl.exact.1) >> 8;
        if (x as i16, y as i16) != fl.next_target_waypoint {
            fl.next_target_waypoint = (x as i16, y as i16);
        }
    }

    /// 0x14018ecd0: play a sound at the image's sprite position.
    fn play_image_sound(&mut self, img: u32, id: u16) {
        let s = self.images[img as usize].sprite;
        let sp = &self.sprites[s as usize];
        self.sounds.push((id, sp.x, sp.y));
    }

    fn frame_update(&mut self, img: u32) {
        let i = &mut self.images[img as usize];
        let f = i.frameset.wrapping_add(i.direction as u16);
        if i.frame != f {
            i.flags |= iflag::REDRAW;
            i.frame = f;
        }
    }

    /// Copy direction/flip from the sprite's main image (followmaingraphic/engframe/engset).
    fn adopt_main_orientation(&mut self, img: u32, main: u32) {
        let (mdir, mflags) = (self.images[main as usize].direction, self.images[main as usize].flags);
        let i = &mut self.images[img as usize];
        i.direction = mdir;
        if i.modifier == 0x11 {
            i.coloring = 0x230;
        }
        i.flags = ((i.flags ^ mflags) & iflag::FLIPPED ^ i.flags) | iflag::REDRAW;
    }

    /// Facing for sprites spawned by spruluselo/sprul/sproluselo: the image's 0..31 direction.
    fn image_dir32(&self, img: u32) -> u8 {
        let i = &self.images[img as usize];
        if i.flags & iflag::FLIPPED != 0 { 0x20u8.wrapping_sub(i.direction) } else { i.direction }
    }
}

/// Run `img`'s iscript until it waits/ends. `distance` receives `move` amounts (BW's movement probe).
pub fn run(st: &mut State, d: &Data, img: u32, nse: bool, mut distance: Option<&mut u32>) {
    {
        let s = &mut st.images[img as usize].iscript;
        if s.wait != 0 {
            s.wait -= 1;
            return;
        }
    }
    let mut p = st.images[img as usize].iscript.pc as usize;
    let u8_ = |at: usize| d.iscript_u8(at);
    let i8_ = |at: usize| d.iscript_u8(at) as i8;
    let u16_ = |at: usize| d.iscript_u16(at);
    loop {
        // Unknown opcode bytes are skipped (the dispatcher loops while op > 0x44).
        let mut op = u8_(p);
        p += 1;
        while op > 0x44 {
            op = u8_(p);
            p += 1;
        }
        let a = p; // operand start
        let unit = st.iscript_unit;
        match op {
            0x00 => {
                p = a + 2;
                let v = u16_(a);
                if !nse && st.images[img as usize].frameset != v {
                    st.images[img as usize].frameset = v;
                    st.frame_update(img);
                }
            }
            0x01 => {
                p = a + 2;
                if !nse {
                    let v = st.tileset.wrapping_add(u16_(a));
                    let frames = d.grp_frames(st.images[img as usize].image_id) & 0x7fff;
                    if v < frames && st.images[img as usize].frameset != v {
                        st.images[img as usize].frameset = v;
                        st.frame_update(img);
                    }
                }
            }
            0x02 => {
                p = a + 1;
                let v = i8_(a);
                if !nse && st.images[img as usize].x != v {
                    let i = &mut st.images[img as usize];
                    i.flags |= iflag::REDRAW;
                    i.x = v;
                }
            }
            0x03 => {
                p = a + 1;
                let v = i8_(a);
                if !nse && unit.is_none_or(|u| !st.host_is_cloaked(u)) && st.images[img as usize].y != v {
                    let i = &mut st.images[img as usize];
                    i.flags |= iflag::REDRAW;
                    i.y = v;
                }
            }
            0x04 => {
                p = a + 2;
                let (x, y) = (i8_(a), i8_(a + 1));
                let i = &mut st.images[img as usize];
                if !nse && (i.x != x || i.y != y) {
                    i.flags |= iflag::REDRAW;
                    i.x = x;
                    i.y = y;
                }
            }
            0x05 => {
                st.images[img as usize].iscript.wait = u8_(a).wrapping_sub(1);
                p = a + 1;
                break;
            }
            0x06 => {
                p = a + 2;
                if !nse {
                    let (lo, hi) = (u8_(a) as i32, u8_(a + 1) as i32);
                    if st.rng.log {
                        eprintln!("  waitrand unit {:?} img {img}", st.iscript_unit);
                    }
                    let r = (st.rng.rand(3) & 0xff) as i32;
                    let rem = r % (hi - lo + 1);
                    st.images[img as usize].iscript.wait = (rem as u8).wrapping_sub(1).wrapping_add(lo as u8);
                    break;
                }
            }
            0x07 => p = u16_(a) as usize,
            0x08 | 0x09 => {
                p = a + 4;
                if !nse {
                    let i = &st.images[img as usize];
                    let x = i.x.wrapping_add(u8_(a + 2) as i8);
                    let y = i.y.wrapping_add(u8_(a + 3) as i8);
                    st.create_overlay(d, img, u16_(a), x, y, op == 0x08);
                }
            }
            0x0a | 0x0b => {
                p = a + 2;
                if !nse {
                    if let Some(n) = st.create_overlay(d, img, u16_(a), 0, 0, op == 0x0a) {
                        if st.images[n as usize].flags & iflag::USES_SPECIAL_OFFSET == 0 {
                            st.images[n as usize].flags |= iflag::USES_SPECIAL_OFFSET;
                            st.image_update_special_offset(d, n);
                        }
                    }
                }
            }
            0x0d | 0x0e => {
                p = a + 4;
                if !nse {
                    let i = &st.images[img as usize];
                    let (kind, idx) = (u8_(a + 2) as usize, u8_(a + 3) as i8);
                    if let Some((mut lx, ly)) = d.lo_offset(kind, i.image_id, i.frame, idx) {
                        if i.flags & iflag::FLIPPED != 0 {
                            lx = lx.wrapping_neg();
                        }
                        let x = i.x.wrapping_add(lx);
                        let y = i.y.wrapping_add(ly);
                        st.create_overlay(d, img, u16_(a), x, y, op == 0x0d);
                    }
                }
            }
            0x0f => {
                p = a + 4;
                if !nse {
                    let mut id = u16_(a);
                    if st.iscript_bullet_is_upgraded_goliath(d) {
                        id = 0x1f9;
                    }
                    let s = st.images[img as usize].sprite;
                    let elev = st.sprites[s as usize].elevation.wrapping_add(1);
                    st.create_sprite_at_image(d, img, id, i8_(a + 2) as i32, i8_(a + 3) as i32, elev);
                }
            }
            0x10 | 0x11 => {
                p = a + 4;
                if !nse {
                    let s = st.images[img as usize].sprite;
                    let elev = if op == 0x10 { st.sprites[s as usize].elevation.wrapping_sub(1) } else { 1 };
                    st.create_sprite_at_image(d, img, u16_(a), i8_(a + 2) as i32, i8_(a + 3) as i32, elev);
                }
            }
            0x12 => {
                p = a + 2;
                if !nse {
                    let im = &st.images[img as usize];
                    let sp = &st.sprites[im.sprite as usize];
                    let (x, y) = (sp.x.wrapping_add(im.x as i16), sp.y.wrapping_add(im.y as i16));
                    st.spawn_unstable_flingy(d, u16_(a), x, y);
                }
            }
            0x13 | 0x14 => {
                p = a + 4;
                if !nse {
                    let id = u16_(a);
                    let show = unit.is_none_or(|u| !st.host_is_cloaked(u))
                        || d.dat.images.draw_if_cloaked[d.dat.sprites.image[id as usize] as usize] != 0;
                    if show {
                        let s = st.images[img as usize].sprite;
                        let mut elev = st.sprites[s as usize].elevation;
                        if op == 0x13 {
                            if st.images[img as usize].direction > 7 {
                                elev = elev.saturating_sub(1);
                            }
                        } else {
                            elev = elev.wrapping_sub(1);
                        }
                        if let Some(ns) = st.create_sprite_at_image(d, img, id, i8_(a + 2) as i32, i8_(a + 3) as i32, elev) {
                            let dir = st.image_dir32(img);
                            st.sprite_set_direction32(d, ns, dir);
                        }
                    }
                }
            }
            0x15 => {
                p = a + 3;
                if !nse {
                    let i = &st.images[img as usize];
                    if let Some((mut lx, ly)) = d.lo_offset(u8_(a + 2) as usize, i.image_id, i.frame, 0) {
                        if i.flags & iflag::FLIPPED != 0 {
                            lx = lx.wrapping_neg();
                        }
                        let s = i.sprite;
                        let elev = st.sprites[s as usize].elevation.wrapping_add(1);
                        if let Some(ns) = st.create_sprite_at_image(d, img, u16_(a), lx as i32, ly as i32, elev) {
                            let dir = st.image_dir32(img);
                            st.sprite_set_direction32(d, ns, dir);
                        }
                    }
                }
            }
            0x16 => {
                if !nse {
                    st.destroy_image(img);
                    st.images[img as usize].iscript.pc = p as u16;
                    return;
                }
            }
            0x17 => {
                p = a + 1;
                let b = u8_(a);
                let i = &mut st.images[img as usize];
                if !nse && ((i.flags >> 1) & 1) as u8 != b {
                    if i.modifier == 0x11 {
                        i.coloring = 0x230;
                    }
                    i.flags = (i.flags & !iflag::FLIPPED) | ((b as u16 & 1) << 1) | iflag::REDRAW;
                    if i.flags & iflag::USES_SPECIAL_OFFSET != 0 {
                        st.image_update_special_offset(d, img);
                    }
                }
            }
            0x18 => {
                p = a + 2;
                if !nse {
                    st.play_image_sound(img, u16_(a));
                }
            }
            0x19 | 0x1c => {
                if op == 0x1c && !nse {
                    if let Some(u) = unit {
                        st.attack_melee(d, u);
                    }
                }
                let n = u8_(a) as usize;
                if !nse && n > 0 {
                    let r = st.rng.rand(4) as usize % n;
                    st.play_image_sound(img, u16_(a + 1 + r * 2));
                }
                p = a + 1 + n * 2;
            }
            0x1a => {
                p = a + 4;
                if !nse {
                    let (lo, hi) = (u16_(a) as u32, u16_(a + 2) as u32);
                    let r = st.rng.rand(5);
                    st.play_image_sound(img, (r % (hi.wrapping_sub(lo).wrapping_add(1)) + lo) as u16);
                }
            }
            0x1b | 0x44 => {
                if !nse {
                    if let Some(b) = st.iscript_bullet {
                        st.bullet_damage(d, b);
                    }
                }
            }
            0x1d => {
                if !nse {
                    let s = st.images[img as usize].sprite;
                    let main = st.sprites[s as usize].main_image;
                    if main != NIL {
                        let (mi, ii) = (&st.images[main as usize], &st.images[img as usize]);
                        if ii.frame != mi.frame || (mi.flags ^ ii.flags) & iflag::FLIPPED != 0 {
                            let fs = mi.frameset;
                            st.images[img as usize].frameset = fs;
                            st.adopt_main_orientation(img, main);
                            let i = &mut st.images[img as usize];
                            let f = fs.wrapping_add(i.direction as u16);
                            if i.frame != f {
                                i.frame = f;
                            }
                        }
                    }
                }
            }
            0x1e => {
                // Draws from the RNG even in dry-run mode.
                let r = (st.rng.rand(7) & 0xff) as u8;
                p = a + 3;
                if r <= u8_(a) {
                    p = u16_(a + 1) as usize;
                }
            }
            0x1f | 0x20 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        let h = st.units[u as usize].fl.heading;
                        let n = u8_(a).wrapping_mul(8);
                        let nh = if op == 0x1f { h.wrapping_sub(n) } else { h.wrapping_add(n) };
                        st.set_unit_heading(d, u, nh);
                    }
                }
            }
            0x21 => {
                if !nse {
                    if let Some(u) = unit.filter(|&u| st.units[u as usize].order_target_unit == NIL) {
                        let h = st.units[u as usize].fl.heading.wrapping_add(8);
                        st.set_unit_heading(d, u, h);
                    }
                }
            }
            0x22 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        let n = u8_(a).wrapping_mul(8);
                        let h = st.units[u as usize].fl.heading;
                        let nh = if st.rng.rand(6) & 3 == 1 { h.wrapping_sub(n) } else { h.wrapping_add(n) };
                        st.set_unit_heading(d, u, nh);
                    }
                }
            }
            0x23 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        st.set_spawn_frame(u, u8_(a));
                    }
                }
            }
            0x24 | 0x41 => {
                p = a + 1;
                if !nse {
                    // 0x6d11f4 is the current order unit, or the bullet being updated.
                    let s = if let Some(u) = st.iscript_order_unit {
                        Some(&mut st.units[u as usize].order_signal)
                    } else {
                        st.iscript_bullet.map(|b| &mut st.bullets.pool[b as usize].order_signal)
                    };
                    if let Some(s) = s {
                        if op == 0x24 { *s |= u8_(a) } else { *s &= !u8_(a) }
                    }
                }
            }
            0x25 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        st.attack_with(d, u, u8_(a));
                    }
                }
            }
            0x26 => {
                if !nse {
                    if let Some(u) = unit {
                        let t = st.units[u as usize].order_target_unit;
                        if t == NIL || st.units[t as usize].status & super::unit::status::IN_AIR == 0 {
                            st.attack_with(d, u, 1);
                        } else {
                            let w = d.dat.units.air_weapon[st.units[u as usize].unit_type as usize];
                            st.fire_projectile(d, u, w);
                            if d.dat.weapons.damage_factor[w as usize] == 2 {
                                st.fire_projectile(d, u, w);
                            }
                        }
                    }
                }
            }
            0x27 => {
                if !nse {
                    if let Some(u) = unit {
                        st.iscript_cast_spell(d, u);
                    }
                }
            }
            0x28 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        st.use_weapon(d, u, u8_(a));
                    }
                }
            }
            0x29 => {
                let n = u8_(a) as u32;
                p = a + 1;
                if let Some(u) = unit {
                    if let Some(dist) = distance.as_deref_mut() {
                        *dist = st.unit_modified_speed(u, n << 8);
                    }
                    if !nse {
                        let v = st.unit_modified_speed(u, n << 8);
                        st.set_unit_next_speed(u, v as i32);
                    }
                }
            }
            0x2a => {
                if !nse {
                    if let Some(u) = unit {
                        st.units[u as usize].fl.flags &= !0x08;
                    }
                }
            }
            0x2b => {
                p = a + 1;
                if !nse {
                    let n = u8_(a) as u16;
                    let s = st.images[img as usize].sprite;
                    let main = st.sprites[s as usize].main_image;
                    st.images[img as usize].frameset = n;
                    st.adopt_main_orientation(img, main);
                    let i = &mut st.images[img as usize];
                    let f = n.wrapping_add(i.direction as u16);
                    if i.frame != f {
                        i.frame = f;
                    }
                }
            }
            0x2c => {
                p = a + 1;
                if !nse {
                    let n = u8_(a) as u16;
                    let s = st.images[img as usize].sprite;
                    let main = st.sprites[s as usize].main_image;
                    let (mid, mfs) = (st.images[main as usize].image_id, st.images[main as usize].frameset);
                    let fs = (d.grp_frames(mid) & 0x7fff).wrapping_mul(n).wrapping_add(mfs);
                    st.images[img as usize].frameset = fs;
                    st.adopt_main_orientation(img, main);
                    let i = &mut st.images[img as usize];
                    let f = fs.wrapping_add(i.direction as u16);
                    if i.frame != f {
                        i.frame = f;
                    }
                }
            }
            0x2d => {
                // 0x4d74c0 op 0x2d: the right-click marker is done (0x652920 = 0).
                if !nse {
                    st.marker_shown = false;
                }
            }
            0x2e => {
                if !nse {
                    if let Some(u) = unit {
                        st.units[u as usize].status |= super::unit::status::NO_BRK_CODE_START;
                        let s = st.units[u as usize].sprite;
                        st.sprites[s as usize].flags |= 0x80;
                    }
                }
            }
            0x2f => {
                if !nse {
                    if let Some(u) = unit {
                        st.nobrk_end(d, u);
                    }
                }
            }
            0x30 => {
                if !nse {
                    if let Some(u) = unit {
                        if st.units[u as usize].order_target_unit != NIL {
                            let i = &mut st.images[img as usize].iscript;
                            i.wait = 10;
                            i.pc = (p - 1) as u16;
                            return;
                        }
                        st.order_start(d, u);
                    }
                }
            }
            0x31 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        let w = st.ground_weapon(d, u);
                        st.set_forward_offset(d, w, u8_(a));
                        st.attack_with(d, u, 1);
                    }
                }
            }
            0x32 => {
                if !nse {
                    st.image_hide(img);
                }
            }
            0x33 => {
                if !nse {
                    st.image_unhide(img);
                }
            }
            0x34 => {
                p = a + 1;
                if !nse {
                    if let Some(u) = unit {
                        st.set_unit_heading(d, u, u8_(a).wrapping_mul(8));
                    }
                }
            }
            0x35 => {
                st.images[img as usize].iscript.ret = (a + 2) as u16;
                p = u16_(a) as usize;
            }
            0x36 => p = st.images[img as usize].iscript.ret as usize,
            0x37 => {
                p = a + 2;
                if !nse {
                    if let Some(u) = unit {
                        st.units[u as usize].fl.top_speed = u16_(a) as i32;
                    }
                }
            }
            0x38 => {
                p = a + 1;
                if !nse {
                    let n = u8_(a) as i8;
                    let i = &st.images[img as usize];
                    if let Some((mut lx, ly)) = d.lo_offset(2, i.image_id, i.frame, n) {
                        if i.flags & iflag::FLIPPED != 0 {
                            lx = lx.wrapping_neg();
                        }
                        let depleted = unit.is_some_and(|u| st.units[u as usize].resource_amount() == 0);
                        let base: u16 = if depleted { 0x1b3 } else { 0x1ae };
                        let (x, y) = (i.x.wrapping_add(lx), i.y.wrapping_add(ly));
                        let s = i.sprite;
                        if let Some(new) = super::sprite::pop(&mut st.images, &mut st.free_images) {
                            let mut l = st.sprites[s as usize].images;
                            super::list::insert_before(&mut st.images, img, new, &mut l);
                            st.sprites[s as usize].images = l;
                            st.image_init(d, new, base.wrapping_add(n as u16), s, x, y);
                        }
                    }
                }
            }
            0x39 => {
                p = a + 2;
                let s = st.images[img as usize].sprite;
                if s != NIL && st.sprites[s as usize].main_image != img {
                    p = u16_(a) as usize;
                }
            }
            0x3a => {
                p = a + 4;
                if !nse {
                    // trgtrangecondjmp: jump when the target's box (shrunk by a quarter
                    // per side) is within the distance (0x4762c0, 0x401240).
                    if let Some(u) = unit {
                        let t = st.units[u as usize].order_target_unit;
                        if t != NIL {
                            let q = st.target_box_point(d, u, t);
                            if st.near_point(u, u16_(a) as u32, q) {
                                p = u16_(a + 2) as usize;
                            }
                        }
                    }
                }
            }
            0x3b => {
                p = a + 6;
                if !nse {
                    if let Some(u) = unit {
                        let t = st.units[u as usize].order_target_unit;
                        if t != NIL {
                            let (ux, uy) = st.units[u as usize].fl.pos;
                            let (tx, ty) = st.units[t as usize].fl.pos;
                            let dir = crate::game::flingy::direction_to(ux as i32, uy as i32, tx as i32, ty as i32);
                            if State::angle_diff(dir, u16_(a) as u8) < u16_(a + 2) as u32 {
                                p = u16_(a + 4) as usize;
                            }
                        }
                    }
                }
            }
            0x3c => {
                p = a + 6;
                if !nse {
                    if let Some(u) = unit {
                        if State::angle_diff(st.units[u as usize].fl.heading, u16_(a) as u8) < u16_(a + 2) as u32 {
                            p = u16_(a + 4) as usize;
                        }
                    }
                }
            }
            0x3d => {
                p = a + 2;
                if !nse {
                    let i = &st.images[img as usize];
                    let (x, y) = (i.x.wrapping_add(u8_(a) as i8), i.y.wrapping_add(u8_(a + 1) as i8));
                    let id = i.image_id.wrapping_add(1);
                    st.create_overlay(d, img, id, x, y, false);
                }
            }
            0x3f => {
                p = a + 2;
                if !nse && unit.is_some_and(|u| st.units[u as usize].status & super::unit::status::IN_AIR != 0) {
                    p = u16_(a) as usize;
                }
            }
            0x40 => {
                p = a + 2;
                if !nse {
                    let i = &mut st.images[img as usize];
                    i.flags |= iflag::REDRAW;
                    i.coloring = u16_(a) as u64;
                }
            }
            0x42 => {
                p = a + 4;
                if !nse {
                    // Only where a marine (unit 0) could stand (0x42d810).
                    let (dx, dy) = (i8_(a + 2), i8_(a + 3));
                    let im = &st.images[img as usize];
                    let sp = &st.sprites[im.sprite as usize];
                    let x = sp.x.wrapping_add(im.x as i16).wrapping_add(dx as i16);
                    let y = sp.y.wrapping_add(im.y as i16).wrapping_add(dy as i16);
                    let elev = sp.elevation.wrapping_add(1);
                    if st.terrain_fits(d, 0, x, y) {
                        st.create_sprite_at_image(d, img, u16_(a), dx as i32, dy as i32, elev);
                    }
                }
            }
            _ => {} // 0x0c, 0x3e, 0x43: no-ops
        }
    }
    st.images[img as usize].iscript.pc = p as u16;
}

impl State {
    /// 0x4d74c0 op 0x0f: a bullet from a goliath (0x4011f0: types 3 / 0x11) with
    /// Charon Boosters (upgrade 0x36), or a hero one in Brood War, uses sprite 0x1f9.
    fn iscript_bullet_is_upgraded_goliath(&self, d: &Data) -> bool {
        let Some(b) = self.iscript_bullet else { return false };
        let src = self.bullets.pool[b as usize].source;
        if src == NIL {
            return false;
        }
        let un = &self.units[src as usize];
        if un.unit_type != 3 && un.unit_type != 0x11 {
            return false;
        }
        self.players.upgrade_bw[un.player as usize][0x36 - 0x2e] != 0
            || (d.dat.units.flags[un.unit_type as usize] & 0x40 != 0 && self.is_expansion)
    }


}
