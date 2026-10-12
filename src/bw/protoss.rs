//! Protoss construction: PlaceProtossBuilding (0x4e4d00) and CreateProtossBuilding
//! (0x4e4f20) for the probe, IncompleteWarping (0x4e4f40) for the building, and WarpIn
//! (0x4e4c70).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Warp texture image shown while a Protoss building warps in.
const WARP_TEXTURE: u16 = 0xd3;

impl State {
    /// Probe's distance to start warping (an assimilator sits on a larger geyser).
    fn warp_distance(&self, u: u32) -> u32 {
        let un = &self.units[u as usize];
        if un.build_queue[un.build_queue_slot as usize] == 0x9d { 0x58 } else { 0x46 }
    }

    /// 0x4e4d00: PlaceProtossBuilding.
    pub(super) fn order_place_protoss_building(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ai != NIL {
            self.units[u as usize].status |= 0x80_0000;
        }
        match self.units[u as usize].order_state {
            0 => {
                let p = self.units[u as usize].order_target_pos;
                if self.move_to_facing(d, u, p) {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                let p = self.units[u as usize].order_target_pos;
                if self.within(u, p, self.warp_distance(u)) {
                    self.unit_halt_facing(d, u);
                    self.units[u as usize].order_state = 2;
                    return;
                }
                self.protoss_keep_moving(d, u);
            }
            2 => {
                if self.units[u as usize].fl.flags & 2 != 0 {
                    return;
                }
                let p = self.units[u as usize].order_target_pos;
                if !self.within(u, p, self.warp_distance(u)) {
                    return self.protoss_keep_moving(d, u);
                }
                if !self.pay_for_building(d, u) {
                    return self.order_done_or_next(d, u);
                }
                let un = &self.units[u as usize];
                let slot = un.build_queue_slot as usize;
                let t = un.build_queue[slot];
                let (pos, player) = (un.order_target_pos, un.player);
                let b = if t == 0x9d {
                    todo_port!("0x4678a0", "assimilator: take over the geyser");
                    None
                } else {
                    self.create_unit(d, t, pos.0, pos.1, player)
                };
                self.units[u as usize].build_queue[slot] = super::train::NONE;
                let Some(b) = b else {
                    todo_port!("0x49e530", "show the creation error");
                    return self.order_done_or_next(d, u);
                };
                let s = self.units[b as usize].sprite;
                self.replace_sprite_images(d, s, WARP_TEXTURE, 0);
                if t == 0x9d {
                    todo_port!("0x498d70", "assimilator overlay image 0x158");
                }
                self.units[u as usize].order_target_unit = b;
                if self.player_type(player) == 1 {
                    todo_port!("0x435770", "AI: new building joins its town");
                }
                self.order_now(d, b, 0x30);
                self.units[u as usize].order_state = 3;
                // 0x48ed50: warp-in sound.
            }
            3 => {
                let un = &self.units[u as usize];
                let b = un.order_target_unit;
                if b != NIL && self.units[b as usize].unit_type == 0x9d && un.order_queue_head == NIL {
                    self.order_with_target(d, u, 0x52, b);
                } else {
                    self.order_done_or_next(d, u);
                }
                self.unit_anim(d, u, 0xc);
            }
            _ => {}
        }
    }

    /// The fallback of states 1 and 2: re-aim at the spot once stopped.
    fn protoss_keep_moving(&mut self, d: &Data, u: u32) {
        if self.unit_at_move_target(u) != 0 {
            let p = self.units[u as usize].order_target_pos;
            self.move_to_facing(d, u, p);
        }
    }

    /// 0x4e4f20: CreateProtossBuilding: PlaceProtossBuilding from its warp step.
    pub(super) fn order_create_protoss_building(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.order = 0x1f;
        un.order_state = 2;
        self.order_place_protoss_building(d, u);
    }

    /// Start `anim` on every image with iscript animations (the loops inlined in
    /// 0x4e4f40 / 0x4e4c70).
    fn images_anim(&mut self, d: &Data, u: u32, anim: u8) {
        let s = self.units[u as usize].sprite;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            if self.images[i as usize].flags & super::sprite::iflag::HAS_ISCRIPT_ANIMATIONS != 0 {
                self.image_run_anim(d, i, anim);
            }
            i = next;
        }
    }

    /// Take the "iscript signalled" flag (order_signal bit 0).
    fn take_signal(&mut self, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.order_signal & 1 == 0 {
            return false;
        }
        un.order_signal &= !1;
        true
    }

    /// The sprite's own main image (sprites.dat image).
    fn sprite_image(&self, d: &Data, u: u32) -> u16 {
        let s = self.units[u as usize].sprite;
        d.dat.sprites.image[self.sprites[s as usize].sprite_id as usize]
    }

    /// 0x4e4f40: IncompleteWarping.
    pub(super) fn order_incomplete_warping(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                if self.units[u as usize].remaining_build_time == 0 {
                    self.images_anim(d, u, 0xd);
                    // 0x48ed50: warp-in sound.
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                if !self.take_signal(u) {
                    return;
                }
                let img = self.sprite_image(d, u);
                let s = self.units[u as usize].sprite;
                self.replace_sprite_images(d, s, img, 0);
                // 0x4d8500: the main image restarts as the warp flash (draw function 0xc).
                let m = self.sprites[s as usize].main_image;
                self.image_restart_as_warp_flash(d, m);
                self.units[u as usize].order_state = 2;
                return;
            }
            2 => {
                if !self.take_signal(u) {
                    return;
                }
                let img = self.sprite_image(d, u);
                let s = self.units[u as usize].sprite;
                self.replace_sprite_images(d, s, img, 0);
                self.images_anim(d, u, 0x15);
                self.units[u as usize].order_state = 3;
                return;
            }
            3 => {
                if !self.take_signal(u) {
                    return;
                }
                self.finish_unit_pre(d, u);
                self.finish_unit_post(d, u);
                self.update_unit_overlaps(d, u);
                if self.units[u as usize].status & 0x400 != 0 {
                    self.images_anim(d, u, 0x18);
                }
            }
            _ => {}
        }
        // Warp progress: build time, hit points and shields.
        let fast = self.game_options & 2 != 0;
        let un = &mut self.units[u as usize];
        let t = un.remaining_build_time;
        if t != 0 {
            un.remaining_build_time = if fast { (t as i16).wrapping_sub(0x10).max(0) as u16 } else { t - 1 };
        }
        let gain = if fast { un.hp_gain.wrapping_shl(4) } else { un.hp_gain } as i32;
        let hp = un.hp + gain;
        self.set_unit_hp(d, u, hp);
        let un = &mut self.units[u as usize];
        let sgain = if fast { un.shield_gain.wrapping_shl(4) } else { un.shield_gain } as i32;
        let max = (d.dat.units.shields[un.unit_type as usize] as i32) << 8;
        un.shields = if max < sgain + un.shields { max } else { un.shields + sgain };
    }

    /// 0x4d8500: run iscript 0xc1 (the warp flash) from its Init animation on image
    /// `img`, then give the image back its own script header (the pc stays in 0xc1),
    /// switch it to draw function 0xc and continue.
    fn image_restart_as_warp_flash(&mut self, d: &Data, img: u32) {
        let flash = d.iscript_header(0xc1).unwrap_or(0) ;
        self.images[img as usize].iscript.header = flash;
        let pc = d.iscript_u16(flash as usize + 8);
        let st = &mut self.images[img as usize].iscript;
        st.anim = 0;
        st.pc = pc;
        st.wait = 0;
        st.ret = 0;
        super::iscript::run(self, d, img, false, None);
        let id = self.images[img as usize].image_id as usize;
        let own = d.iscript_header(d.dat.images.iscript[id]).unwrap_or(0);
        let i = &mut self.images[img as usize];
        i.iscript.header = own;
        i.modifier = 0xc;
        i.flags |= super::sprite::iflag::REDRAW;
        super::iscript::run(self, d, img, false, None);
    }

    /// 0x4e4c70: WarpIn.
    pub(super) fn order_warp_in(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                if !self.take_signal(u) {
                    return;
                }
                let img = self.sprite_image(d, u);
                let s = self.units[u as usize].sprite;
                let h = self.units[u as usize].fl.heading;
                self.replace_sprite_images(d, s, img, h);
                self.unit_anim(d, u, 0x15);
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if !self.take_signal(u) {
                    return;
                }
                if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x40_0000 != 0 {
                    todo_port!("0x49b5b0", "warped-in unit with flag 0x400000");
                }
                self.order_next_or_idle(d, u);
            }
            _ => {}
        }
    }
}
