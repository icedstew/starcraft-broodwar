//! Workers mining minerals: Harvest1 / MoveToMinerals / WaitForMinerals /
//! MiningMinerals / ReturnMinerals and the resource searches they use.
//!
//! Worker union (+0xc0): +0xc4 target resource position, +0xc8 target resource,
//! +0xce gathering flag, +0xcf carried amount, +0xd0 resource being gathered,
//! +0xd4 / +0xd8 links in the resource's wait list. Resource union: +0xd0 amount,
//! +0xd3 being gathered, +0xd4 first waiting worker. Unit pointers in the union
//! are stored as index + 1 (0 = none).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

fn is_mineral(t: u16) -> bool {
    (0xb0..=0xb2).contains(&t)
}

impl super::unit::Unit {
    pub fn ptr_at(&self, off: usize) -> u32 {
        let i = off - 0xc0;
        let v = u32::from_le_bytes([self.union_c0[i], self.union_c0[i + 1], self.union_c0[i + 2], self.union_c0[i + 3]]);
        if v == 0 { NIL } else { v - 1 }
    }
    pub fn set_ptr_at(&mut self, off: usize, u: u32) {
        let v = if u == NIL { 0 } else { u + 1 };
        let i = off - 0xc0;
        self.union_c0[i..i + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn pos_at(&self, off: usize) -> (i16, i16) {
        (self.u16_at(off) as i16, self.u16_at(off + 2) as i16)
    }
    fn set_pos_at(&mut self, off: usize, p: (i16, i16)) {
        self.set_u16_at(off, p.0 as u16);
        self.set_u16_at(off + 2, p.1 as u16);
    }
}

impl State {
    fn target_type(&self, u: u32) -> Option<u16> {
        let t = self.units[u as usize].order_target_unit;
        (t != NIL).then(|| self.units[t as usize].unit_type)
    }

    /// The idle order (0x9c for AI units).
    fn idle_order(&self, d: &Data, u: u32) -> u8 {
        if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] }
    }

    /// Shared opening of the harvest orders: a worker carrying a powerup stops and
    /// goes on with its queue (or idles).
    fn drop_harvest_for_powerup(&mut self, d: &Data, u: u32) {
        self.unit_halt(d, u);
        let un = &mut self.units[u as usize];
        if un.fl.next_target_waypoint != un.fl.move_target {
            un.fl.next_target_waypoint = un.fl.move_target;
        }
        if un.order_queue_head != NIL {
            un.order_flags |= 1;
            self.prepare_next_order(d, u);
        } else {
            let o = self.idle_order(d, u);
            self.order_now(d, u, o);
        }
    }

    fn carries_powerup(&self, u: u32) -> bool {
        let r = self.units[u as usize].resource_type;
        r != 0 && r & 3 == 0
    }

    /// 0x468aa0: first harvest step queues 0x97 in front of the queue.
    fn harvest_mark(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x80_0000 != 0 {
            return;
        }
        self.units[u as usize].status |= 0x80_0000;
        self.queue_front(d, u, 0x97, (0, 0), NIL);
    }

    /// 0x474540 / 0x4745f0: queue in front of the queue, or append when it is empty.
    fn queue_front(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32) {
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            self.order_insert_before(d, u, order, 0xe4, pos, target, head);
        } else {
            self.order_queue_append(d, u, order, 0xe4, pos, target);
        }
    }

    /// 0x474a70: queue `order` targeting `t` in front.
    fn queue_front_target(&mut self, d: &Data, u: u32, order: u8, t: u32) {
        let pos = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        self.queue_front(d, u, order, pos, t);
    }

    /// 0x4eb980: move to unit `t` and aim at it.
    fn move_to_unit_aim(&mut self, d: &Data, u: u32, t: u32) -> bool {
        let r = self.move_to_unit(d, u, t);
        let p = self.unit_sprite_pos(t);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        r
    }

    /// 0x474400: remove the first queued `order`.
    fn remove_queued(&mut self, d: &Data, u: u32, order: u8) {
        let mut n = self.units[u as usize].order_queue_head;
        while n != NIL {
            if self.orders[n as usize].order == order {
                self.order_remove(d, u, n);
                return;
            }
            n = self.orders[n as usize].link.next;
        }
    }

    /// Give up on the harvest order: drop interruptible / MoveToMinerals tail orders,
    /// queue MoveToMinerals and start the next order.
    fn harvest_restart(&mut self, d: &Data, u: u32) {
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].order != 0 {
            loop {
                let t = self.units[u as usize].order_queue_tail;
                if t == NIL {
                    break;
                }
                let o = self.orders[t as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != 0x55 {
                    break;
                }
                self.order_remove(d, u, t);
            }
            self.order_queue_append(d, u, 0x55, 0xe4, (0, 0), NIL);
        }
        self.prepare_next_order(d, u);
    }

    /// 0x469500: Harvest1 / MoveToGas.
    pub fn order_harvest1(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if self.carries_powerup(u) {
            self.drop_harvest_for_powerup(d, u);
            return;
        }
        self.harvest_mark(d, u);
        match self.units[u as usize].order_state {
            0 => {
                if t == NIL {
                    self.unit_halt_facing(d, u);
                    self.order_done_or_next(d, u);
                    return;
                }
                if is_mineral(self.units[t as usize].unit_type) {
                    let pos = self.units[u as usize].order_target_pos;
                    self.queue_front(d, u, 0x55, pos, t);
                    self.order_done_or_next(d, u);
                    return;
                }
                if self.move_to_unit_aim(d, u, t) {
                    let un = &mut self.units[u as usize];
                    let p = un.order_target_pos;
                    un.set_pos_at(0xc4, p);
                    un.set_ptr_at(0xc8, t);
                    un.order_state = 3;
                }
            }
            3 => {
                let r = self.unit_at_move_target(u);
                if r == 0 {
                    return;
                }
                if t != NIL && self.is_refinery_of(t, u) {
                    if r == 2 {
                        self.units[u as usize].order_state = 0;
                        return;
                    }
                    if self.units[u as usize].resource_type & 1 != 0 {
                        self.order_now(d, u, 0x54);
                        return;
                    }
                    self.queue_front_target(d, u, 0x52, t);
                    self.order_done_or_next(d, u);
                    return;
                }
                self.unit_halt_facing(d, u);
                self.order_done_or_next(d, u);
            }
            _ => {}
        }
    }

    /// 0x468930: `t` is a completed gas building owned by `u`'s player.
    fn is_refinery_of(&self, t: u32, u: u32) -> bool {
        let tu = &self.units[t as usize];
        tu.status & 1 != 0 && matches!(tu.unit_type, 0x6e | 0x9d | 0x95) && tu.player == self.units[u as usize].player
    }

    /// 0x469240: MoveToMinerals.
    pub fn order_move_to_minerals(&mut self, d: &Data, u: u32) {
        let mut m = self.units[u as usize].order_target_unit;
        if self.carries_powerup(u) {
            self.drop_harvest_for_powerup(d, u);
            return;
        }
        self.harvest_mark(d, u);
        if m == NIL || !is_mineral(self.units[m as usize].unit_type) {
            let un = &mut self.units[u as usize];
            let from = un.pos_at(0xc4);
            let from = if from == (0, 0) {
                self.unit_sprite_pos(u)
            } else {
                un.set_pos_at(0xc4, (0, 0));
                from
            };
            let found = self.find_mineral(d, u, from);
            if found == NIL {
                self.unit_halt(d, u);
                let un = &mut self.units[u as usize];
                if un.fl.next_target_waypoint != un.fl.move_target {
                    un.fl.next_target_waypoint = un.fl.move_target;
                }
                self.order_done_or_next(d, u);
                return;
            }
            m = found;
            let p = self.unit_sprite_pos(m);
            let un = &mut self.units[u as usize];
            un.order_target_unit = m;
            un.order_target_pos = p;
            un.order_state = 0;
        }
        let alt;
        match self.units[u as usize].order_state {
            0 => {
                if !self.move_to_unit_aim(d, u, self.units[u as usize].order_target_unit) {
                    return;
                }
                let un = &mut self.units[u as usize];
                let (p, t) = (un.order_target_pos, un.order_target_unit);
                un.set_pos_at(0xc4, p);
                un.set_ptr_at(0xc8, t);
                un.order_state = 1;
                return;
            }
            1 => {
                if !self.box_distance_within(d, u, 10, m) && self.unit_at_move_target(u) == 0 {
                    return;
                }
                let mut a = NIL;
                if self.units[m as usize].u8_at(0xd3) != 0 {
                    let p = self.unit_sprite_pos(u);
                    a = self.find_free_mineral(d, u, p, false);
                }
                if a == NIL || a == m {
                    self.units[u as usize].order_state = 3;
                    if !self.move_to_minerals_arrived(d, u, m) {
                        return;
                    }
                    return;
                }
                alt = a;
            }
            3 => {
                self.move_to_minerals_arrived(d, u, m);
                return;
            }
            _ => return,
        }
        self.move_to_alt_mineral(d, u, alt);
    }

    /// MoveToMinerals state 3. Returns false when still walking.
    fn move_to_minerals_arrived(&mut self, d: &Data, u: u32, m: u32) -> bool {
        let r = self.unit_at_move_target(u);
        if r == 0 {
            return false;
        }
        if r == 2 {
            let p = self.unit_sprite_pos(u);
            let a = self.find_mineral(d, u, p);
            if a == NIL {
                self.order_done_or_next(d, u);
                return true;
            }
            self.move_to_alt_mineral(d, u, a);
            return true;
        }
        if self.units[u as usize].resource_type & 2 != 0 {
            self.order_now(d, u, 0x5a);
            return true;
        }
        // 0x494bb0
        let un = &mut self.units[u as usize];
        let p = un.order_target_pos;
        if un.fl.next_target_waypoint != p {
            un.fl.next_target_waypoint = p;
        }
        self.queue_front_target(d, u, 0x56, m);
        self.order_done_or_next(d, u);
        true
    }

    fn move_to_alt_mineral(&mut self, d: &Data, u: u32, a: u32) {
        if !self.move_to_unit_aim(d, u, a) {
            return;
        }
        let p = self.unit_sprite_pos(a);
        let un = &mut self.units[u as usize];
        un.set_ptr_at(0xc8, a);
        un.order_target_unit = a;
        un.set_pos_at(0xc4, p);
        un.order_target_pos = p;
    }

    /// 0x468f60: WaitForMinerals.
    pub fn order_wait_for_minerals(&mut self, d: &Data, u: u32) {
        if !self.target_type(u).is_some_and(is_mineral) {
            self.harvest_restart(d, u);
            return;
        }
        if self.units[u as usize].order_state == 0 {
            let m = self.units[u as usize].order_target_unit;
            self.wait_for_mineral(d, u, m);
        }
    }

    /// 0x468d10: take the mineral or join its wait list.
    fn wait_for_mineral(&mut self, d: &Data, u: u32, m: u32) {
        let un = &mut self.units[u as usize];
        un.order_process_timer = 0;
        un.order_state = 2;
        if self.try_take_mineral(d, u, m) {
            return;
        }
        if self.units[u as usize].ptr_at(0xd0) == NIL {
            let first = self.units[m as usize].ptr_at(0xd4);
            let un = &mut self.units[u as usize];
            un.set_ptr_at(0xd0, m);
            un.set_ptr_at(0xd8, first);
            un.set_ptr_at(0xd4, NIL);
            if first != NIL {
                self.units[first as usize].set_ptr_at(0xd4, u);
            }
            self.units[m as usize].set_ptr_at(0xd4, u);
        }
        self.queue_front(d, u, 0x59, (0, 0), NIL);
    }

    /// 0x468c70: start mining `m` if nobody is.
    fn try_take_mineral(&mut self, d: &Data, u: u32, m: u32) -> bool {
        if self.units[m as usize].u8_at(0xd3) != 0 {
            return false;
        }
        self.units[m as usize].set_u8_at(0xd3, 1);
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xce, 1);
        un.set_ptr_at(0xd0, m);
        let order = if un.order == 0x52 {
            0x53
        } else {
            self.queue_front(d, u, 0x58, (0, 0), NIL);
            0x57
        };
        self.queue_front_target(d, u, order, m);
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
        } else {
            let o = self.idle_order(d, u);
            self.order_now(d, u, o);
        }
        true
    }

    /// 0x468970: take the last eligible waiting worker off `m`'s wait list.
    fn next_waiting_worker(&mut self, d: &Data, m: u32) -> u32 {
        let mut w = self.units[m as usize].ptr_at(0xd4);
        let mut pick = NIL;
        while w != NIL {
            let wu = &self.units[w as usize];
            if wu.status & 0x400 == 0 && wu.lockdown_timer == 0 && wu.stasis_timer == 0 && wu.maelstrom_timer == 0 && (wu.order == 0x52 || wu.order == 0x56) {
                pick = w;
            }
            w = wu.ptr_at(0xd8);
        }
        if pick != NIL {
            let (prev, next) = (self.units[pick as usize].ptr_at(0xd4), self.units[pick as usize].ptr_at(0xd8));
            if prev == NIL {
                self.units[m as usize].set_ptr_at(0xd4, next);
            } else {
                self.units[prev as usize].set_ptr_at(0xd8, next);
            }
            if next != NIL {
                self.units[next as usize].set_ptr_at(0xd4, prev);
            }
            let pu = &mut self.units[pick as usize];
            pu.set_ptr_at(0xd4, NIL);
            pu.set_ptr_at(0xd8, NIL);
            pu.set_ptr_at(0xd0, NIL);
            self.remove_queued(d, pick, 0x59);
        }
        pick
    }

    /// 0x468e40: stop gathering from `m` and hand it to the next waiting worker.
    fn release_mineral(&mut self, d: &Data, u: u32, m: u32) {
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xce, 0);
        un.set_ptr_at(0xd0, NIL);
        if m != NIL {
            let c = self.units[m as usize].u8_at(0xd3);
            self.units[m as usize].set_u8_at(0xd3, c.wrapping_sub(1));
            let w = self.next_waiting_worker(d, m);
            if w != NIL {
                self.try_take_mineral(d, w, m);
            }
        }
    }

    /// 0x4697c0: MiningMinerals.
    pub fn order_mining_minerals(&mut self, d: &Data, u: u32) {
        let m = self.units[u as usize].order_target_unit;
        if m == NIL || !is_mineral(self.units[m as usize].unit_type) {
            if self.units[u as usize].u8_at(0xce) != 0 {
                let g = self.units[u as usize].ptr_at(0xd0);
                self.release_mineral(d, u, g);
            }
            self.remove_queued(d, u, 0x58);
            self.harvest_restart(d, u);
            return;
        }
        self.harvest_mark(d, u);
        let s = self.units[u as usize].sprite;
        match self.units[u as usize].order_state {
            0 => {
                if !self.facing_target(u) {
                    return;
                }
                let h = self.units[u as usize].fl.heading;
                let (tx, ty) = crate::game::flingy::direction_table()[h as usize];
                let p = self.unit_sprite_pos(u);
                let un = &mut self.units[u as usize];
                un.order_target_pos = ((((tx * 0x14) >> 8) as i16).wrapping_add(p.0), (((ty * 0x14) >> 8) as i16).wrapping_add(p.1));
                un.order_state = 4;
                self.sprite_images_run_anim(d, s, 0xf);
                let un = &mut self.units[u as usize];
                un.main_order_timer = 0x4b;
                un.order_state = 5;
            }
            4 => {
                self.sprite_images_run_anim(d, s, 0xf);
                let un = &mut self.units[u as usize];
                un.main_order_timer = 0x4b;
                un.order_state = 5;
            }
            5 if self.units[u as usize].main_order_timer == 0 => {
                self.sprite_images_run_anim(d, s, 8);
                self.gather(d, u, m);
                self.release_mineral(d, u, m);
                self.remove_queued(d, u, 0x58);
                let next = if self.units[u as usize].resource_type & 2 != 0 { 0x5a } else { 0x55 };
                self.order_now(d, u, next);
            }
            _ => {}
        }
    }

    /// 0x402be0: `u` faces its target waypoint (or is on it).
    fn facing_target(&self, u: u32) -> bool {
        let f = &self.units[u as usize].fl;
        if f.next_target_waypoint == f.pos {
            return true;
        }
        let p = self.unit_sprite_pos(u);
        let t = f.next_target_waypoint;
        crate::game::flingy::direction_to(p.0 as i32, p.1 as i32, t.0 as i32, t.1 as i32) == f.heading
    }

    /// 0x4696d0: take resources from `r` and carry them.
    fn gather(&mut self, d: &Data, u: u32, r: u32) {
        let t = self.units[r as usize].unit_type;
        let (mut image, mineral) = match t {
            0x6e => (0x193u16, false),
            0x9d => (0x18f, false),
            0x95 => (0x191, false),
            0xb0..=0xb2 => (0x18d, true),
            _ => return,
        };
        let amount = self.take_resource(d, r, mineral);
        if amount < 8 {
            image += 1;
        }
        if amount == 0 {
            return;
        }
        if self.units[u as usize].resource_type & 3 != 0 {
            self.remove_carried_image(u);
            self.units[u as usize].resource_type = 0;
        }
        let mut amount = amount;
        if self.units[u as usize].ai != NIL {
            todo_port!("0x447a70", "AI worker bonus");
        }
        if amount == 0 {
            amount = 0;
        }
        self.carry(d, u, mineral, image, amount);
    }

    /// 0x469650: take up to 8 from a resource (depleting minerals).
    fn take_resource(&mut self, d: &Data, r: u32, mineral: bool) -> u8 {
        let a = self.units[r as usize].resource_amount();
        if a < 8 {
            if mineral {
                self.kill_unit(d, r);
                return a as u8;
            }
            self.units[r as usize].set_u16_at(0xd0, 0);
            return 2;
        }
        let a = a - 8;
        self.units[r as usize].set_u16_at(0xd0, a);
        if mineral {
            if a != 0 {
                self.mineral_frames(d, r);
            } else {
                self.kill_unit(d, r);
            }
        } else if a < 8 {
            todo_port!("0x48ee30", "geyser depleted notice");
        }
        8
    }

    /// 0x4f3af0: start carrying `amount` (minerals or gas), showing the chunk.
    fn carry(&mut self, d: &Data, u: u32, mineral: bool, image: u16, amount: u8) {
        if self.units[u as usize].resource_type != 0 {
            return;
        }
        self.units[u as usize].resource_type = if mineral { 2 } else { 1 };
        let s = self.units[u as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        let saved = self.iscript_unit;
        if let Some(img) = self.create_overlay(d, main, image, 0, 0, true) {
            if self.images[img as usize].flags & 0x80 == 0 {
                self.images[img as usize].flags |= 0x80;
                todo_port!("0x4d5a00", "carried chunk offset from the worker's LO overlay");
            }
        }
        self.iscript_unit = saved;
        self.units[u as usize].set_u8_at(0xcf, amount);
    }

    /// 0x4f3900: remove the carried chunk / powerup image.
    fn remove_carried_image(&mut self, u: u32) {
        let s = self.units[u as usize].sprite;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            let id = self.images[i as usize].image_id;
            if (0x195..0x1a3).contains(&id) {
                self.destroy_image(i);
                break;
            }
            i = next;
        }
        let mut i = self.sprites[s as usize].images.head;
        let mut hit = NIL;
        while i != NIL {
            let id = self.images[i as usize].image_id;
            if (0x188..0x195).contains(&id) {
                hit = i;
                break;
            }
            i = self.images[i as usize].link.next;
        }
        if hit == NIL {
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                let id = self.images[i as usize].image_id;
                if (0x3be..0x3c0).contains(&id) {
                    hit = i;
                    break;
                }
                i = self.images[i as usize].link.next;
            }
        }
        if hit != NIL {
            self.destroy_image(hit);
        }
    }

    /// 0x4f39a0: deliver the carried resources.
    fn deliver(&mut self, u: u32) {
        self.remove_carried_image(u);
        let un = &self.units[u as usize];
        let (p, amount, r) = (un.player as usize, un.u8_at(0xcf) as i32, un.resource_type);
        if r & 2 != 0 {
            self.players.minerals[p] += amount;
            self.players.minerals_gathered[p] += amount;
        } else if r & 1 != 0 {
            self.players.gas[p] += amount;
            self.players.gas_gathered[p] += amount;
        }
        if r & 3 != 0 && self.units[u as usize].ai != NIL {
            todo_port!("0x4f39a0", "AI resource stats");
        }
        let un = &mut self.units[u as usize];
        un.resource_type = 0;
        un.set_u8_at(0xcf, 0);
    }

    /// 0x4690c0: ReturnMinerals / ReturnGas.
    pub fn order_return_resources(&mut self, d: &Data, u: u32) {
        let r = self.units[u as usize].resource_type;
        if r == 0 || r & 3 == 0 {
            self.drop_harvest_for_powerup(d, u);
            return;
        }
        self.harvest_mark(d, u);
        let mut t = self.units[u as usize].order_target_unit;
        if t == NIL || !self.is_drop_off(d, t) {
            t = NIL;
            self.units[u as usize].order_state = 0;
        }
        match self.units[u as usize].order_state {
            0 => {
                if t == NIL {
                    t = self.find_drop_off(d, u);
                    if t == NIL {
                        if self.units[u as usize].ai != NIL {
                            self.order_now(d, u, 0x9c);
                        } else {
                            self.units[u as usize].order_process_timer = 0x4b;
                        }
                        return;
                    }
                }
                self.units[u as usize].order_target_unit = t;
                if self.move_to_unit_aim(d, u, t) {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                let a = self.unit_at_move_target(u);
                if a == 0 {
                    return;
                }
                if a == 2 {
                    let un = &mut self.units[u as usize];
                    un.order_target_unit = NIL;
                    un.order_state = 0;
                    return;
                }
                let order = if self.units[u as usize].ai != NIL { 0x9c } else { 0x51 | ((!r & 1) << 2) };
                let res = self.units[u as usize].ptr_at(0xc8);
                if res != NIL {
                    self.queue_front_target(d, u, order, res);
                } else {
                    self.queue_front(d, u, order, (0, 0), NIL);
                }
                self.order_done_or_next(d, u);
                self.deliver(u);
                if matches!(self.units[t as usize].unit_type, 0x83..=0x85) {
                    todo_port!("0x4e92b0", "hatchery larva wobble counters");
                }
            }
            _ => {}
        }
    }

    /// 0x401cf0: `t` accepts resources (completed resource depot, or a hatchery
    /// morphing on).
    fn is_drop_off(&self, d: &Data, t: u32) -> bool {
        let tu = &self.units[t as usize];
        let ok = tu.status & 1 != 0 || matches!(tu.unit_type, 0x85 | 0x84) || (tu.unit_type == 0x83 && self.morphing_zerg_building(t));
        ok && tu.status & 2 != 0 && d.dat.units.flags[tu.unit_type as usize] & 0x1000 != 0
    }

    /// 0x441310: nearest drop-off of `u`'s player.
    fn find_drop_off(&mut self, d: &Data, u: u32) -> u32 {
        let p = self.units[u as usize].player as usize;
        let c = &self.players.completed_counts;
        if c[0x83][p] == 0 && c[0x9a][p] == 0 && c[0x6a][p] == 0 && c[0x84][p] == 0 && c[0x85][p] == 0 {
            return NIL;
        }
        // 0x4e8740 over the map box (0x6284a0), excluding `u`.
        let s = self.units[u as usize].sprite;
        let (x, y) = (self.sprites[s as usize].x, self.sprites[s as usize].y);
        let fi = self.units[u as usize].finder_index;
        let (ix_asc, iy_asc, ix_desc, iy_desc) = if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            (fi[0] + 1, fi[2] + 1, fi[1] - 1, fi[3] - 1)
        } else {
            let ix = super::finder::search(&self.finder.x, self.finder.count, true, x as i32) as i32;
            let iy = super::finder::search(&self.finder.y, self.finder.count, true, y as i32) as i32;
            (ix, iy, ix - 1, iy - 1)
        };
        todo_port!("0x441310", "map box 0x6284a0 assumed (0, 0, map width, map height)");
        let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
        self.nearest_unit(b, u, (x, y), [ix_desc, iy_desc, ix_asc, iy_asc], &mut |st: &mut State, c: u32| {
            st.is_drop_off(d, c)
                && st.units[c as usize].player == st.units[u as usize].player
                && (st.units[u as usize].pathing_flags & 1 == 0 || st.same_group(u, st.unit_sprite_pos(c)))
        })
    }

    /// 0x442f70: nearest reachable mineral to `from` within 0x180.
    fn find_mineral(&mut self, d: &Data, u: u32, from: (i16, i16)) -> u32 {
        if !self.any_minerals() {
            return NIL;
        }
        self.mineral_search(d, u, from, 0x180, None)
    }

    /// 0x4432d0: nearest mineral nobody is mining (`ai_only`: AI workers skip
    /// fields marked +0xd9).
    fn find_free_mineral(&mut self, d: &Data, u: u32, from: (i16, i16), ai_only: bool) -> u32 {
        if !self.any_minerals() {
            return NIL;
        }
        let r = if ai_only { 0x200 } else { 0x100 };
        self.mineral_search(d, u, from, r, Some(ai_only))
    }

    fn any_minerals(&self) -> bool {
        let c = &self.players.completed_counts;
        c[0xb0][11] != 0 || c[0xb1][11] != 0 || c[0xb2][11] != 0
    }

    /// 0x4e86c0 with 0x442e80 (and 0x443280 when `free` is set).
    fn mineral_search(&mut self, d: &Data, u: u32, from: (i16, i16), radius: i32, free: Option<bool>) -> u32 {
        let (x, y) = from;
        let b = [
            (x as i32 - radius) as i16,
            (y as i32 - radius) as i16,
            (x as i32 + radius) as i16,
            (y as i32 + radius) as i16,
        ];
        let ix = super::finder::search(&self.finder.x, self.finder.count, true, x as i32) as i32;
        let iy = super::finder::search(&self.finder.y, self.finder.count, true, y as i32) as i32;
        let center_elev = self.los_elevation(x as i32, y as i32);
        self.nearest_unit(b, NIL, (x, y), [ix - 1, iy - 1, ix, iy], &mut |st: &mut State, m: u32| {
            if !st.mineral_usable(d, m, u, center_elev, radius) {
                return false;
            }
            match free {
                None => true,
                Some(ai_only) => {
                    if ai_only && st.player_type(st.units[u as usize].player) == 1 && st.units[m as usize].u8_at(0xd9) != 0 {
                        return false;
                    }
                    st.units[m as usize].u8_at(0xd3) == 0
                }
            }
        })
    }

    /// 0x442e80: mineral field `m` is a candidate for worker `u`.
    fn mineral_usable(&self, d: &Data, m: u32, u: u32, center_elev: u8, radius: i32) -> bool {
        let _ = d;
        if !is_mineral(self.units[m as usize].unit_type) || self.mineral_enclosed(d, m) {
            return false;
        }
        let mp = self.unit_sprite_pos(m);
        if self.units[u as usize].pathing_flags & 1 != 0 && !self.same_group(u, mp) {
            return false;
        }
        if self.los_elevation(mp.0 as i32, mp.1 as i32) != center_elev {
            return false;
        }
        let i = (mp.1 as i32 / 32) as usize * self.terrain.width as usize + (mp.0 as i32 / 32) as usize;
        if self.terrain.tile_flags[i] & (1 << ((self.units[u as usize].player as u32 + 8) & 0x1f)) != 0 {
            return false;
        }
        let up = self.unit_sprite_pos(u);
        self.path_distance(m, up) <= radius * 2
    }

    /// 0x42fa30: distance from `m` to `p` (path length for ground units).
    fn path_distance(&self, m: u32, p: (i16, i16)) -> i32 {
        if self.units[m as usize].pathing_flags & 1 == 0 {
            let mp = self.unit_sprite_pos(m);
            return super::path::approx_distance(mp.0 as i32, p.0 as i32, mp.1 as i32, p.1 as i32) as i32;
        }
        todo_port!("0x42fa30", "region path distance for ground units");
        0x7ffe
    }

    /// 0x473560: every tile around the mineral's box is marked 0x8000000.
    fn mineral_enclosed(&self, d: &Data, m: u32) -> bool {
        let ut = self.units[m as usize].unit_type as usize;
        let v = d.dat.units.dimensions[ut];
        let [l, up, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let (sx, sy) = self.unit_sprite_pos(m);
        let div = |v: i32| v / 32;
        let w_tiles = div(r.wrapping_add(l).wrapping_add(1) as i32);
        let h_tiles = div(dn.wrapping_add(up).wrapping_add(1) as i32);
        let top = div(sy.wrapping_sub(up) as i32);
        let mut t0 = top - 1;
        let bot = div(dn.wrapping_add(sy) as i32) + 1;
        let left = div(sx.wrapping_sub(l) as i32);
        let mut l0 = left - 1;
        let right = div(r.wrapping_add(sx) as i32) + 1;
        let (mw, mh) = (self.terrain.width as i32, self.terrain.height as i32);
        let bot = if bot >= mh { mh - 1 } else { bot };
        if t0 < 0 {
            t0 = 0;
        }
        if l0 < 0 {
            l0 = 0;
        }
        let right = if right >= mw { mw - 1 } else { right };
        let f = |x: i32, y: i32| self.terrain.tile_flags[(y * mw + x) as usize] & 0x800_0000 != 0;
        for i in 0..w_tiles {
            if !f(left + i, t0) || !f(left + i, bot) {
                return false;
            }
        }
        for j in 0..h_tiles {
            if !f(l0, top + j) || !f(right, top + j) {
                return false;
            }
        }
        true
    }

    /// 0x4e8320: nearest unit to `p` passing `ok`, scanning the finder outward from
    /// the given indices and shrinking `b` as better ones turn up.
    fn nearest_unit(&mut self, mut b: [i16; 4], exclude: u32, p: (i16, i16), idx: [i32; 4], ok: &mut dyn FnMut(&mut State, u32) -> bool) -> u32 {
        let (x, y) = (p.0 as i32, p.1 as i32);
        let [mut xd, mut yd, mut xa, mut ya] = idx;
        let dx = (x - b[0] as i32).max(b[2] as i32 - x);
        let dy = (y - b[1] as i32).max(b[3] as i32 - y);
        let mut best = super::path::approx_distance(0, dx, 0, dy) as i32;
        let mut result = NIL;
        loop {
            let mut any = false;
            let mut found = false;
            let n = self.finder.count as i32;
            let pos = |st: &State, e: u32| {
                let s = &st.sprites[st.units[(e - 1) as usize].sprite as usize];
                (s.x, s.y)
            };
            let mut consider = |st: &mut State, c: u32, best: &mut i32, result: &mut u32, found: &mut bool| {
                if c == exclude || !ok(st, c) {
                    return;
                }
                let (cx, cy) = st.unit_sprite_pos(c);
                let dd = super::path::approx_distance(cx as i32, x, cy as i32, y) as i32;
                if dd < *best {
                    *best = dd;
                    *result = c;
                    *found = true;
                }
            };
            if xd >= 0 {
                let e = self.finder.x[xd as usize].0;
                let (ux, uy) = pos(self, e);
                if ux < b[0] {
                    xd = -1;
                } else if b[1] <= uy && uy < b[3] {
                    consider(self, e - 1, &mut best, &mut result, &mut found);
                }
                if xd >= 0 {
                    xd -= 1;
                } else {
                    xd = -2;
                }
                any = true;
            }
            if xa < n {
                let e = self.finder.x[xa as usize].0;
                let (ux, uy) = pos(self, e);
                if ux >= b[2] {
                    xa = n;
                } else if b[1] <= uy && uy < b[3] {
                    consider(self, e - 1, &mut best, &mut result, &mut found);
                }
                xa += 1;
                any = true;
            }
            if yd >= 0 {
                let e = self.finder.y[yd as usize].0;
                let (ux, uy) = pos(self, e);
                if uy < b[1] {
                    yd = -1;
                } else if b[0] <= ux && ux < b[2] {
                    consider(self, e - 1, &mut best, &mut result, &mut found);
                }
                if yd >= 0 {
                    yd -= 1;
                } else {
                    yd = -2;
                }
                any = true;
            }
            if ya < n {
                let e = self.finder.y[ya as usize].0;
                let (ux, uy) = pos(self, e);
                if uy >= b[3] {
                    ya = n;
                } else if b[0] <= ux && ux < b[2] {
                    consider(self, e - 1, &mut best, &mut result, &mut found);
                }
                ya += 1;
                any = true;
            }
            if found {
                if (b[0] as i32) < x - best {
                    b[0] = (x - best) as i16;
                }
                if (b[2] as i32) > x + best {
                    b[2] = (x + best) as i16;
                }
                if (b[1] as i32) < y - best {
                    b[1] = (y - best) as i16;
                }
                if (b[3] as i32) > y + best {
                    b[3] = (y + best) as i16;
                }
            }
            if !any {
                return result;
            }
        }
    }
}
