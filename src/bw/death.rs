//! Killing units: the Die order and taking a unit out of the game.

use super::finder::Axis;
use super::list::{self, NIL};
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x475710: give `u` the Die order (clearing its queue).
    pub fn kill_unit(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].resource_type != 0 {
            todo_port!("0x4f3b70", "drop a carried powerup");
        }
        loop {
            let h = self.units[u as usize].order_queue_head;
            if h == NIL {
                break;
            }
            self.order_remove(d, u, h);
        }
        self.units[u as usize].order_flags |= 1;
        let pos = self.units[u as usize].order_target_pos;
        if self.units[u as usize].order != 0 {
            loop {
                let t = self.units[u as usize].order_queue_tail;
                if t == NIL {
                    break;
                }
                let o = self.orders[t as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != 0 {
                    break;
                }
                self.order_remove(d, u, t);
            }
            self.order_queue_append(d, u, 0, 0xe4, pos, NIL);
        }
        self.prepare_next_order(d, u);
        if self.units[u as usize].ai != NIL {
            todo_port!("0x475710", "AI bookkeeping (0x439d60, 0x434c90, 0x4477c0)");
        }
    }

    /// 0x479480: Die.
    pub fn order_die(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_flags & 4 != 0 {
            todo_port!("0x4e6340", "remove the unit from the map before dying");
        }
        if self.units[u as usize].subunit != NIL {
            todo_port!("0x479480", "kill the subunit (0x496f00, 0x4a0990)");
        }
        self.units[u as usize].order_state = 1;
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            let st = self.units[u as usize].status;
            if st & 0x4000_0000 == 0 || (st as i32) < 0 {
                // 0x499d00(1, 1)
                self.sprite_images_run_anim(d, s, 1);
                self.remove_unit(d, u);
                return;
            }
            todo_port!("0x479480", "hallucination death (0x48ed50, sprite 0x176)");
        }
        todo_port!("0x4e6340", "hidden unit dies");
        self.remove_unit(d, u);
        self.free_unit_sprite(u);
    }

    /// 0x4a0740: take a dying unit out of the game (lists, finder, counters).
    pub fn remove_unit(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        let flags = d.dat.units.flags[t as usize];
        if flags & 1 != 0 {
            todo_port!("0x4a0740", "building destroyed flag 0x6d5bd0");
        }
        self.remove_unit_type_specific(d, u);
        self.remove_loaded_units(d, u);
        self.clear_references(d, u);
        self.untarget_bullets(u);
        if self.units[u as usize].ai != NIL {
            todo_port!("0x4a0740", "AI bookkeeping (0x439d60, 0x434c90, 0x4477c0)");
        }
        if flags & 0x10 != 0 || matches!(t, 0x6e | 0x95 | 0x9d) {
            todo_port!("0x4a0080", "subunits and special units leave through 0x4a0080");
            return;
        }
        self.unit_halt(d, u);
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            // 0x46a560
            self.finder_remove(u);
            if self.units[u as usize].status & 2 != 0 {
                todo_port!("0x469ec0", "free the building's tiles");
            }
            if self.units[u as usize].status & 4 != 0 {
                todo_port!("0x4533d0", "air unit repulse grid");
            }
        }
        if flags & 1 == 0 {
            self.units[u as usize].is_cloaked = 0;
            self.unlink_cloaked(u);
            if self.units[u as usize].status & 0x100 != 0 {
                todo_port!("0x49b440", "cloaked unit list");
            }
        }
        if self.units[u as usize].status & 2 != 0 {
            todo_port!("0x49fd00", "building cleanup (addons, nydus, larva, build queue)");
        }
        if flags & 0x20 != 0 && self.units[u as usize].u8_at(0xcb) != 0 {
            todo_port!("0x469ec0", "free the flying building's landing tiles");
        }
        if self.units[u as usize].path != NIL {
            self.path_delete(u);
        }
        let so = self.units[u as usize].secondary_order;
        if self.units[u as usize].current_build_unit != NIL && (so == 0x26 || so == 0x3f) {
            todo_port!("0x4a0740", "kill the unit being trained");
        }
        if so != 0x17 {
            self.set_secondary_order(u, 0x17);
        }
        if self.units[u as usize].resource_type != 0 {
            todo_port!("0x4f3b70", "drop a carried powerup");
        }
        todo_port!("0x49a7f0", "selection circles / UI");
        self.modify_unit_counters(d, u, -1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, -1, false);
        }
        self.move_to_dead_list(u);
    }

    /// 0x49f4a0: per-type cleanup (workers' carried resources, mines, nydus, ...).
    fn remove_unit_type_specific(&mut self, d: &Data, u: u32) {
        let _ = d;
        let t = self.units[u as usize].unit_type;
        match t {
            0x48 | 0x52 | 0x53 | 0x51 => todo_port!("0x466170", "carrier/reaver children"),
            1 | 0x10 | 100 | 99 | 0x68 => {
                if self.units[u as usize].u16_at(0xd0) != 0 || self.units[u as usize].u16_at(0xd2) != 0 {
                    todo_port!("0x49f4a0", "ghost nuke");
                }
            }
            0xb0..=0xb2 => todo_port!("0x468770", "mineral field removal"),
            0xe | 0x49 | 0x55 | 0x6c | 0x86 | 0x9c => todo_port!("0x49f4a0", "special unit removal"),
            _ => {
                todo_port!("0x4688b0", "resource container check");
            }
        }
    }

    /// 0x49fdd0: units inside a transport die with it.
    fn remove_loaded_units(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x40 != 0 {
            todo_port!("0x401ea0", "unit inside a transport");
            return;
        }
        if un.status & 0x4000_0000 != 0 || d.dat.units.space_provided[un.unit_type as usize] == 0 {
            return;
        }
        if un.unit_type == 0x2a && self.players.upgrade_sc[un.player as usize][0x18] == 0 {
            return;
        }
        if un.loaded_units.iter().any(|&h| h != 0) {
            todo_port!("0x49fdd0", "kill loaded units");
        }
        self.units[u as usize].loaded_units = [0; 8];
    }

    /// 0x49eb70(1): every unit forgets `dead` (and the UI drops it).
    fn clear_references(&mut self, d: &Data, dead: u32) {
        let mut o = self.visible_units.head;
        while o != NIL {
            let next = self.units[o as usize].link.next;
            self.forget_unit(d, o, dead);
            o = next;
        }
        todo_port!("0x49eb70", "selection / unit portrait UI (0x68ac98)");
        let mut o = self.hidden_units.head;
        while o != NIL {
            let next = self.units[o as usize].link.next;
            self.forget_unit(d, o, dead);
            o = next;
        }
        let t = self.units[dead as usize].unit_type as usize;
        if d.dat.units.flags[t] & 8 != 0 && self.units[dead as usize].u16_at(0xd0) != 0 {
            todo_port!("0x468ef0", "worker releases its resource target");
        }
    }

    /// 0x49e590: `o` drops every reference to `dead`.
    fn forget_unit(&mut self, d: &Data, o: u32, dead: u32) {
        let dpos = self.unit_sprite_pos(dead);
        let un = &mut self.units[o as usize];
        if un.current_build_unit == dead {
            un.current_build_unit = NIL;
        }
        if un.order_target_unit == dead {
            un.order_target_unit = NIL;
        }
        if un.move_target_unit == dead {
            un.move_target_unit = NIL;
        }
        if un.auto_target == dead {
            un.auto_target = NIL;
        }
        if un.connected_unit == dead {
            un.connected_unit = NIL;
        }
        if un.irradiated_by == dead {
            un.irradiated_by = NIL;
        }
        if d.dat.units.flags[un.unit_type as usize] & 8 != 0 || un.status & 2 != 0 {
            todo_port!("0x49e590", "worker / building union references (+0xc0, +0xc8, +0xd0)");
        }
        let t = un.unit_type;
        if matches!(t, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b) && un.rally_unit == dead {
            un.rally_unit = NIL;
            un.rally_pos = dpos;
        }
        let sub = un.subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.forget_unit(d, sub, dead);
        }
        let mut n = self.units[o as usize].order_queue_head;
        while n != NIL {
            let next = self.orders[n as usize].link.next;
            if self.orders[n as usize].target == dead {
                self.order_remove(d, o, n);
            }
            n = next;
        }
    }

    /// 0x48aac0: bullets aimed at `u` keep its position; ones it fired lose their source.
    fn untarget_bullets(&mut self, u: u32) {
        let p = self.unit_sprite_pos(u);
        let mut b = self.bullets.active.head;
        while b != NIL {
            let bu = &mut self.bullets.pool[b as usize];
            if bu.target == u {
                bu.target_pos = p;
                bu.target = NIL;
            }
            if bu.source == u {
                bu.source = NIL;
            }
            b = bu.link.next;
        }
    }

    /// 0x4916e0: unlink from the list at 0x63ff5c (+0xf0 / +0xf4).
    fn unlink_cloaked(&mut self, u: u32) {
        if self.units[u as usize].is_cloaked != 0 {
            return;
        }
        let (prev, next) = (self.units[u as usize].prev_burrowed, self.units[u as usize].next_burrowed);
        if self.cloak_list_head == u {
            self.cloak_list_head = next;
        }
        if prev != NIL {
            self.units[prev as usize].next_burrowed = next;
        }
        if next != NIL {
            self.units[next as usize].prev_burrowed = prev;
        }
        let un = &mut self.units[u as usize];
        un.status &= !0x800;
        un.prev_burrowed = NIL;
        un.next_burrowed = NIL;
    }

    /// 0x49e3f0: player list out, visible/hidden list out, dead list in.
    fn move_to_dead_list(&mut self, u: u32) {
        let p = self.units[u as usize].player as usize;
        let (prev, next) = (self.units[u as usize].player_prev, self.units[u as usize].player_next);
        if prev != NIL {
            self.units[prev as usize].player_next = next;
        }
        if next != NIL {
            self.units[next as usize].player_prev = prev;
        }
        if self.player_units[p] == u {
            self.player_units[p] = next;
        }
        self.units[u as usize].player_prev = NIL;
        self.units[u as usize].player_next = NIL;
        let t = self.units[u as usize].unit_type;
        let s = self.units[u as usize].sprite;
        if t == 0x21 || t == 0x65 {
            let mut l = self.scanner_units;
            list::remove(&mut self.units, u, &mut l);
            self.scanner_units = l;
        } else if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            let mut l = self.visible_units;
            list::remove(&mut self.units, u, &mut l);
            self.visible_units = l;
        } else {
            let mut l = self.hidden_units;
            list::remove(&mut self.units, u, &mut l);
            self.hidden_units = l;
        }
        let mut l = self.dead_units;
        list::insert_after_head(&mut self.units, u, &mut l);
        self.dead_units = l;
    }

    /// 0x497b40 for a unit: free its sprite and images.
    pub fn free_unit_sprite(&mut self, u: u32) {
        let s = self.units[u as usize].sprite;
        if s == NIL {
            return;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.destroy_image(i);
            i = next;
        }
        self.free_sprite(s);
        self.units[u as usize].sprite = NIL;
    }

    /// 0x49e340: return a dead unit to the free list.
    pub fn free_unit(&mut self, d: &Data, u: u32) {
        self.unit_count -= 1;
        loop {
            let h = self.units[u as usize].order_queue_head;
            if h == NIL {
                break;
            }
            self.order_remove(d, u, h);
        }
        self.units[u as usize].sprite = NIL;
        let mut l = self.dead_units;
        list::remove(&mut self.units, u, &mut l);
        self.dead_units = l;
        let mut f = self.free_units;
        list::insert_after_head(&mut self.units, u, &mut f);
        self.free_units = f;
    }

    /// 0x46a300: remove a unit's boxes from the finder.
    pub fn finder_remove(&mut self, u: u32) {
        if self.units[u as usize].finder_index[0] < 0 {
            return;
        }
        self.finder.count -= 1;
        let fi = self.units[u as usize].finder_index;
        self.finder_remove_one(Axis::X, fi[1] as usize);
        let fi = self.units[u as usize].finder_index;
        self.finder_remove_one(Axis::Y, fi[3] as usize);
        self.units[u as usize].finder_index[1] = -1;
        self.units[u as usize].finder_index[3] = -1;
        self.finder.count -= 1;
        let fi = self.units[u as usize].finder_index;
        self.finder_remove_one(Axis::X, fi[0] as usize);
        let fi = self.units[u as usize].finder_index;
        self.finder_remove_one(Axis::Y, fi[2] as usize);
        self.units[u as usize].finder_index[0] = -1;
        self.units[u as usize].finder_index[2] = -1;
    }

    /// 0x469d70 (x) / 0x469d00 (y): close the hole at `at`.
    fn finder_remove_one(&mut self, ax: Axis, at: usize) {
        let count = self.finder.count;
        if at >= count {
            return;
        }
        let (lo, hi) = match ax {
            Axis::X => (0, 1),
            Axis::Y => (2, 3),
        };
        for i in at + 1..=count {
            let e = match ax {
                Axis::X => self.finder.x[i].0,
                Axis::Y => self.finder.y[i].0,
            };
            if e != 0 {
                let fi = &mut self.units[(e - 1) as usize].finder_index;
                if fi[lo] == i as i32 {
                    fi[lo] -= 1;
                } else if fi[hi] == i as i32 {
                    fi[hi] -= 1;
                }
            }
        }
        let a = match ax {
            Axis::X => &mut self.finder.x,
            Axis::Y => &mut self.finder.y,
        };
        a.copy_within(at + 1..count + 1, at);
    }

    /// 0x467340: set hp, capped at units.dat max.
    pub fn set_unit_hp(&mut self, d: &Data, u: u32, hp: i32) {
        let max = d.dat.units.hit_points[self.units[u as usize].unit_type as usize] as i32;
        self.units[u as usize].hp = hp.min(max);
        if self.units[u as usize].status & 1 != 0 {
            todo_port!("0x467340", "damage overlays (0x4e6090) and AI strength (0x431800)");
        }
    }
}
