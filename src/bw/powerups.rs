//! Powerups (UMS maps): flags (0xd7), Khalis / Uraj crystals (0x80 / 0x81) and the
//! other carryable items. Worker fields: +0xc0 the carried powerup, +0xe0 bit 4 set while
//! carrying one. Powerup fields: +0xd0 its home spot, +0xd4 the carrier.

use super::list::NIL;
use super::{Data, State};

const FLAG: u16 = 0xd7;

impl State {
    /// 0x4014e0: the powerup (+0xc0) carried by unit `u` if its units.dat flags have
    /// bit 8 (worker), else NIL.
    pub(super) fn carried_powerup(&self, d: &Data, u: u32) -> u32 {
        let un = &self.units[u as usize];
        if d.dat.units.flags[un.unit_type as usize] & 8 != 0 { un.ptr_at(0xc0) } else { NIL }
    }

    /// 0x4f3ca0: worker `c` picks up powerup `p` (dropping a mineral / gas chunk first).
    fn pick_up_powerup(&mut self, d: &Data, c: u32, p: u32) {
        if self.units[c as usize].resource_type != 0 {
            if self.units[c as usize].ptr_at(0xc0) != NIL {
                return;
            }
            self.remove_carried_image(c);
            self.units[c as usize].resource_type = 0;
        }
        let pt = self.units[p as usize].unit_type;
        if pt == FLAG {
            let owner = self.units[p as usize].player;
            self.ctf_event(owner, 0x7c, true);
        }
        let image = match pt {
            0x80 => 0x3be,
            0x81 => 0x3bf,
            t => t + 0xb1,
        };
        self.hide_unit(d, p);
        self.units[p as usize].order_flags |= 1;
        if self.units[p as usize].order != 0 {
            loop {
                let tail = self.units[p as usize].order_queue_tail;
                if tail == NIL {
                    break;
                }
                let o = self.orders[tail as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != 0x17 {
                    break;
                }
                self.order_remove(d, p, tail);
            }
            self.order_queue_append(d, p, 0x17, 0xe4, (0, 0), NIL);
        }
        self.prepare_next_order(d, p);
        self.units[p as usize].set_ptr_at(0xd4, c);
        let s = self.units[c as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        if let Some(img) = self.create_image(d, s, main, image, 0, 0, 0) {
            if self.images[img as usize].modifier == 0xe {
                let pl = self.units[c as usize].player as u64;
                let i = &mut self.images[img as usize];
                i.coloring = (i.coloring & !0xffff_ffff) | pl;
            }
            if self.images[img as usize].flags & 0x80 == 0 {
                self.images[img as usize].flags |= 0x80;
                self.image_update_special_offset(d, img);
            }
        }
        self.units[c as usize].set_ptr_at(0xc0, p);
        self.units[c as usize].resource_type |= 4;
        // 0x68c1e8 etc.: UI refresh.
    }

    /// 0x4f3b70: a worker carrying a powerup drops it where it stands (when the spot is
    /// open ground), else the powerup goes home.
    pub(super) fn drop_powerup(&mut self, d: &Data, u: u32) {
        let r = self.units[u as usize].resource_type;
        if r == 0 || r & 3 != 0 {
            return;
        }
        let p = self.units[u as usize].ptr_at(0xc0);
        self.remove_carried_image(u);
        let (x, y) = self.unit_sprite_pos(u);
        let pt = self.units[p as usize].unit_type;
        let here = self.tile_flags_match(x, y, u32::MAX, 0x800_0000) && self.terrain_fits(d, pt, x, y);
        let spot = if here {
            (x, y)
        } else {
            (self.units[p as usize].u16_at(0xd0) as i16, self.units[p as usize].u16_at(0xd2) as i16)
        };
        // 0x6284a0: the whole map as the search box.
        let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
        let q = self.find_unit_placement(d, p, spot, Some(b), false).unwrap_or(spot);
        self.place_unit_at(d, p, q.0, q.1);
        let o = if pt == FLAG && here { 0x11 } else { 0x61 };
        self.order_now(d, p, o);
        self.units[p as usize].set_ptr_at(0xd4, NIL);
        self.units[u as usize].resource_type = 0;
        self.units[u as usize].set_ptr_at(0xc0, NIL);
    }

    /// 0x4f3e10: PowerupIdle: shown again if hidden; a worker that walks over it picks
    /// it up (filter 0x440930).
    pub(super) fn order_powerup_idle(&mut self, d: &Data, p: u32) {
        let s = self.units[p as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            self.show_unit(d, p);
        }
        let (x, y) = self.unit_sprite_pos(p);
        let b = [x.wrapping_sub(0x18), y.wrapping_sub(0x18), x.wrapping_add(0x18), y.wrapping_add(0x18)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let pu = &self.units[p as usize];
        let c = found.into_iter().find(|&c| {
            let cu = &self.units[c as usize];
            d.dat.units.flags[cu.unit_type as usize] & 8 != 0
                && cu.status & 1 != 0
                && cu.status & 0x10 == 0
                && cu.status & 0x4000_0000 == 0
                && cu.ptr_at(0xc0) == NIL
                && !(pu.unit_type == FLAG && pu.player == cu.player)
        });
        if let Some(c) = c {
            self.pick_up_powerup(d, c, p);
            // 0x68c1b0 etc.: UI refresh.
        }
    }

    /// 0x4e3de0: "%s had his flag ..." (network.tbl `fmt`, 0-based) about `player`.
    // ponytail: team games (0x57f0b4 / 0x596875: the team's name) are never set here.
    fn ctf_notice(&mut self, player: u8, fmt: u16) {
        self.notices.push((fmt, player));
    }

    /// 0x4e3f30 (taken, 0x7c) / 0x4e3e90 (captured, 0x80): in capture the flag, the
    /// local race's flag sound (0x20 + race; 0x4f3ea0 has none) and the notice.
    // ponytail: the sound's volume gate (0x68c468) is not ported (as in 0x48f230).
    pub(super) fn ctf_event(&mut self, player: u8, fmt: u16, sound: bool) {
        if self.template[8] != 4 {
            return;
        }
        if sound {
            self.ui_sounds.push(0x20 + self.player_races[self.local_player as usize] as u16);
        }
        self.ctf_notice(player, fmt);
    }

    /// 0x4f3ea0: UnusedPowerup: a dropped flag waits 0x151 frames, then goes home.
    pub(super) fn order_unused_powerup(&mut self, d: &Data, p: u32) {
        self.order_powerup_idle(d, p);
        if self.units[p as usize].order_state == 0 {
            self.units[p as usize].remaining_build_time = 0x151;
            self.units[p as usize].order_state = 1;
            return;
        }
        let t = self.units[p as usize].remaining_build_time;
        if t != 0 {
            self.units[p as usize].remaining_build_time = t - 1;
            return;
        }
        let home = (self.units[p as usize].u16_at(0xd0) as i16, self.units[p as usize].u16_at(0xd2) as i16);
        self.place_unit_at(d, p, home.0, home.1);
        if self.template[8] == 4 {
            let owner = self.units[p as usize].player;
            self.ctf_notice(owner, 0x7e);
        }
        self.order_now(d, p, 0x61);
    }
}
