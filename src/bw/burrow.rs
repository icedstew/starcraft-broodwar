//! Burrowing zerg units: the Burrow / Unburrow commands and the Burrowing (0x74),
//! Burrowed (0x75) and Unburrowing (0x76) orders (inline in 0x4ec4d0).

use super::list::NIL;
use super::sprite::iflag;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x4c1fa0: Burrow command: units that can burrow (units.dat flag 0x100000) and may
    /// use Burrowing (tech 0xb).
    pub fn command_burrow(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            let un = &self.units[u as usize];
            if d.dat.units.flags[un.unit_type as usize] & 0x10_0000 == 0 || self.can_use_tech(d, u, 0xb, player) != Ok(1) {
                continue;
            }
            let un = &self.units[u as usize];
            if un.status & 0x10 == 0 && un.order != 0x74 {
                self.command_order(d, u, 1, queued, 0x74, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// 0x4c1ac0: Unburrow command.
    pub fn command_unburrow(&mut self, d: &Data, player: u8, sel: &[u32]) {
        for u in self.live_selection(sel) {
            let t = self.units[u as usize].unit_type as usize;
            if d.dat.units.flags[t] & 0x10_0000 != 0 && self.can_use_tech(d, u, 0xb, player) == Ok(1) {
                self.unburrow(d, u);
            }
        }
    }

    /// 0x4e97c0: a burrowing or burrowed unit (or any lurker) is told to come up.
    pub(super) fn unburrow(&mut self, d: &Data, u: u32) {
        let o = self.units[u as usize].order;
        let lurker = self.units[u as usize].unit_type == 0x67;
        if o != 0x74 && o != 0x75 && !lurker {
            return;
        }
        if lurker {
            self.units[u as usize].status |= 0x1000;
        }
        self.units[u as usize].order_flags |= 1;
        if o != 0 {
            loop {
                let h = self.units[u as usize].order_queue_head;
                if h == NIL {
                    break;
                }
                let ho = self.orders[h as usize].order;
                if d.dat.orders.interruptible[ho as usize] == 0 && ho != 0x76 {
                    break;
                }
                self.order_remove(d, u, h);
            }
            self.order_queue_append(d, u, 0x76, 0xe4, (0, 0), NIL);
        }
        self.prepare_next_order(d, u);
        self.order_next_or_idle(d, u);
    }

    /// 0x4011a0 with direction `dir`: aim `u` 16 pixels ahead in that direction.
    fn face_direction(&mut self, u: u32, dir: u8) {
        let (vx, vy) = crate::game::flingy::direction_table()[(dir as usize * 8) & 0xff];
        let f = &mut self.units[u as usize].fl;
        let x = (((vx << 12) >> 8) + f.exact.0) >> 8;
        let y = (((vy << 12) >> 8) + f.exact.1) >> 8;
        let p = (x as i16, y as i16);
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// Burrowing (0x74, 0x4e9e60): stop, turn, play Burrow, then hide underground.
    pub(super) fn order_burrowing(&mut self, d: &Data, u: u32) {
        let mut st = self.units[u as usize].order_state;
        if st == 0 {
            self.unit_halt(d, u);
            self.units[u as usize].order_state = 1;
            st = 1;
        }
        if st == 1 {
            if self.units[u as usize].fl.flags & 2 != 0 {
                if self.units[u as usize].fl.next_speed == 0 {
                    self.unit_stop_moving(d, u);
                }
                return;
            }
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xc);
            let dir = match self.units[u as usize].unit_type {
                0x2e | 0x34 => 0xb,
                0x29 => 0xd,
                _ => 0xc,
            };
            self.face_direction(u, dir);
            self.units[u as usize].order_state = 2;
            st = 2;
        }
        if st == 2 {
            if !self.facing_target(u) {
                return;
            }
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0x19);
            // 0x48ed50: the burrow sound (UI).
            self.units[u as usize].order_state = 3;
        }
        if self.units[u as usize].order_signal & 4 == 0 {
            return;
        }
        self.units[u as usize].order_signal &= !4;
        let s = self.units[u as usize].sprite;
        self.hide_uncloakable_images(d, s);
        self.status_overlay_image(s).inspect(|&i| self.images[i as usize].flags |= iflag::HIDDEN);
        self.units[u as usize].status |= 0x20_0000;
        self.go_underground(u);
        self.set_secondary_order(u, 0x6d);
        self.units[u as usize].visibility = 0x8000_0000;
        self.remove_status_overlays(d, u);
        // 0x68c1b0 etc.: button refresh (UI).
        self.order_now(d, u, 0x75);
        self.order_next_or_idle(d, u);
    }

    /// Burrowed (0x75).
    pub(super) fn order_burrowed(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.unit_type == 0x67 {
            return self.order_next_or_idle(d, u);
        }
        if un.unit_type == 0x26 {
            let h = un.order_queue_head;
            if h != NIL && self.orders[h as usize].order == 0x2a {
                // Hydralisk told to morph while burrowed: come up first.
                self.order_now(d, u, 0x76);
                todo_port!("0x4749a0", "queue the morph after unburrowing");
                return self.order_next_or_idle(d, u);
            }
        }
        let un = &self.units[u as usize];
        if un.ai != NIL && un.main_order_timer == 0 {
            self.units[u as usize].main_order_timer = 7;
            todo_port!("0x4a13c0", "AI: burrowed unit checks whether to come up (0x477160)");
        }
    }

    /// Unburrowing (0x76, 0x4ea680).
    pub(super) fn order_unburrowing(&mut self, d: &Data, u: u32) {
        let mut st = self.units[u as usize].order_state;
        if st == 0 {
            let t = self.units[u as usize].unit_type as usize;
            let s = self.units[u as usize].sprite;
            self.sprites[s as usize].elevation = d.dat.units.elevation[t];
            self.units[u as usize].status &= !0x20_0000;
            self.update_unit_overlaps(d, u);
            self.sprites[s as usize].flags &= !0x40;
            self.set_secondary_order(u, 0x17);
            self.sprite_images_run_anim(d, s, 0x1a);
            // 0x48ed50: the unburrow sound (UI).
            self.units[u as usize].order_state = 1;
            return;
        }
        if st == 1 {
            let un = &mut self.units[u as usize];
            un.visibility = 0x8000_0000;
            un.set_u8_at(0x91, 0);
            un.order_state = 2;
            st = 2;
        }
        if st == 2 {
            if self.units[u as usize].order_signal & 4 == 0 {
                return;
            }
            self.units[u as usize].order_signal &= !4;
            self.come_up(d, u);
            self.reapply_status_overlays(d, u);
            let s = self.units[u as usize].sprite;
            if let Some(i) = self.status_overlay_image(s) {
                let f = &mut self.images[i as usize].flags;
                if *f & iflag::HIDDEN != 0 {
                    *f = (*f & !iflag::HIDDEN) | iflag::REDRAW;
                }
            }
            // 0x498c30: every image takes the unit's heading.
            let h = self.units[u as usize].fl.heading;
            self.sprite_images_set_direction(d, s, h);
            self.units[u as usize].order_state = 3;
            return;
        }
        if self.units[u as usize].order_queue_head == NIL {
            let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
        }
        self.order_next_or_idle(d, u);
        // 0x68c1b0 etc.: button refresh (UI).
    }

    /// 0x4976c0: hide the images of sprite `s` that are not drawn when cloaked.
    fn hide_uncloakable_images(&mut self, d: &Data, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if d.dat.images.draw_if_cloaked[im.image_id as usize] == 0 && im.flags & iflag::HIDDEN == 0 {
                // 0x42d280: screen refresh (UI).
                im.flags |= iflag::HIDDEN;
            }
            i = im.link.next;
        }
    }

    /// 0x497750 / 0x4981b0: the first status overlay image of sprite `s` (images
    /// 0x188..=0x194, else 0x3be / 0x3bf).
    fn status_overlay_image(&self, s: u32) -> Option<u32> {
        let head = self.sprites[s as usize].images.head;
        let find = |lo: u16, hi: u16| {
            let mut i = head;
            while i != NIL {
                let id = self.images[i as usize].image_id;
                if (lo..=hi).contains(&id) {
                    return Some(i);
                }
                i = self.images[i as usize].link.next;
            }
            None
        };
        find(0x188, 0x194).or_else(|| find(0x3be, 0x3bf))
    }

    /// 0x4e9a30: the unit is underground: no path, burrowed (0x10) and needing detection
    /// (0x800), sprite flag 0x40, the burrowed sprite id and elevation 1.
    fn go_underground(&mut self, u: u32) {
        self.path_free(u);
        let s = self.units[u as usize].sprite;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | low;
        un.status |= 0x810;
        let lurker = un.unit_type == 0x67;
        let sp = &mut self.sprites[s as usize];
        sp.flags |= 0x40;
        // 0x499860: the sprite id (selection circle and health bar size).
        sp.sprite_id = if lurker { 0x1f8 } else { 0x12c };
        sp.elevation = 1;
    }

    /// 0x4e99d0: back on the surface: no path, not burrowed, the flingy's sprite id.
    fn come_up(&mut self, d: &Data, u: u32) {
        self.path_free(u);
        let s = self.units[u as usize].sprite;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | low;
        un.status &= !0x810;
        let t = un.unit_type as usize;
        self.sprites[s as usize].sprite_id = d.dat.flingy.sprite[d.dat.units.flingy[t] as usize];
    }

    /// 0x4f46f0: a burrowing unit loses the defense matrix, irradiate, ensnare and plague
    /// overlay images (the timers stay).
    fn remove_status_overlays(&mut self, _d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.defense_matrix_damage != 0 {
            todo_port!("0x454ed0", "defense matrix overlay removed on burrow");
        }
        if un.irradiate_timer != 0 {
            self.remove_overlay_in(u, 0x17c, 0x17e);
        }
        if self.units[u as usize].ensnare_timer != 0 {
            self.remove_overlay_in(u, 0x180, 0x182);
        }
        if self.units[u as usize].plague_timer != 0 {
            self.remove_overlay_in(u, 0x184, 0x186);
        }
    }

    /// 0x4e5cf0: remove the first image with an id in `lo..=hi` from `u`'s sprite, else
    /// from its subunit's.
    pub(super) fn remove_overlay_in(&mut self, u: u32, lo: u16, hi: u16) {
        let mut sprites = vec![self.units[u as usize].sprite];
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            sprites.push(self.units[sub as usize].sprite);
        }
        for s in sprites {
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                if (lo..=hi).contains(&self.images[i as usize].image_id) {
                    self.destroy_image(i);
                    return;
                }
                i = self.images[i as usize].link.next;
            }
        }
    }

    /// 0x492da0: put back the overlays of the spells affecting `u`.
    pub(super) fn reapply_status_overlays(&mut self, _d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.defense_matrix_timer != 0 {
            todo_port!("0x455010", "defense matrix overlay");
        }
        if un.lockdown_timer != 0 {
            todo_port!("0x455380", "lockdown overlay");
        }
        if un.maelstrom_timer != 0 {
            todo_port!("0x4553f0", "maelstrom overlay");
        }
        if un.irradiate_timer != 0 {
            todo_port!("0x454e80", "irradiate overlay");
        }
        if un.ensnare_timer != 0 {
            todo_port!("0x4f45e0", "ensnare overlay");
        }
        if un.stasis_timer != 0 {
            todo_port!("0x4f67b0", "stasis overlay");
        }
        if un.plague_timer != 0 {
            todo_port!("0x4f4550", "plague overlay");
        }
        if un.u8_at(0x126) != 0 {
            todo_port!("0x4f41f0", "acid spore overlay");
        }
    }
}
