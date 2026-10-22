//! Order queue (1.16.1 COrder pool 0x6416a0, free list 0x64b2e0) and order issuing.

use super::list::{self, Link, List, Node, NIL};
use super::{Data, State};
use crate::game::flingy::mflags;

/// 0x48c590: 2000 queued-order nodes.
pub const MAX_ORDERS: usize = 2000;

/// COrder (20 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Order {
    pub link: Link,
    /// +0x8
    pub order: u8,
    /// +0xa
    pub unit_type: u16,
    /// +0xc
    pub pos: (i16, i16),
    /// +0x10
    pub target: u32,
}

impl Default for Order {
    fn default() -> Order {
        Order { link: Link::default(), order: 0, unit_type: 0, pos: (0, 0), target: NIL }
    }
}

impl Node for Order {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

impl State {
    /// 0x48c510: take a node from the free list.
    fn order_alloc(&mut self, order: u8, unit_type: u16, pos: (i16, i16), target: u32) -> Option<u32> {
        let n = self.free_orders.head;
        if n == NIL {
            return None;
        }
        let o = &mut self.orders[n as usize];
        o.order = order;
        o.unit_type = unit_type;
        o.pos = pos;
        o.target = target;
        let mut l = self.free_orders;
        list::remove(&mut self.orders, n, &mut l);
        self.free_orders = l;
        self.order_count += 1;
        Some(n)
    }

    fn unit_queue(&self, u: u32) -> List {
        let un = &self.units[u as usize];
        List { head: un.order_queue_head, tail: un.order_queue_tail }
    }

    fn set_unit_queue(&mut self, u: u32, l: List) {
        let un = &mut self.units[u as usize];
        un.order_queue_head = l.head;
        un.order_queue_tail = l.tail;
    }

    /// 0x474540 / 0x4745f0 without a free order (0x48c510): "Unable to add order."
    fn order_failed(&mut self, u: u32) {
        self.local_error_line(self.units[u as usize].player, 0x368);
    }

    fn bump_queue_count(&mut self, d: &Data, u: u32, order: u8) {
        if d.dat.orders.highlight[order as usize] != 0xffff {
            let un = &mut self.units[u as usize];
            un.order_queue_count = un.order_queue_count.wrapping_add(1);
        }
    }

    /// 0x4742d0: unlink `n` from the unit's queue and return it to the free list (insert_after_head).
    pub fn order_remove(&mut self, d: &Data, u: u32, n: u32) {
        let order = self.orders[n as usize].order;
        let un = &mut self.units[u as usize];
        if d.dat.orders.highlight[order as usize] != 0xffff {
            un.order_queue_count = un.order_queue_count.wrapping_sub(1);
        }
        if un.order_queue_count == 0xff {
            un.order_queue_count = 0;
        }
        let mut q = self.unit_queue(u);
        list::remove(&mut self.orders, n, &mut q);
        self.set_unit_queue(u, q);
        self.order_count -= 1;
        let mut f = self.free_orders;
        list::insert_after_head(&mut self.orders, n, &mut f);
        self.free_orders = f;
    }

    /// 0x4744d0: empty the order queue.
    pub fn clear_order_queue(&mut self, d: &Data, u: u32) {
        while self.units[u as usize].order_queue_head != NIL {
            let n = self.units[u as usize].order_queue_head;
            self.order_remove(d, u, n);
        }
    }

    /// 0x4745f0: queue an order after `after` (or at the end when NIL).
    fn order_insert_after(&mut self, d: &Data, u: u32, order: u8, unit_type: u16, pos: (i16, i16), target: u32, after: u32) -> Option<u32> {
        let Some(n) = self.order_alloc(order, unit_type, pos, target) else {
            self.order_failed(u);
            return None;
        };
        self.bump_queue_count(d, u, order);
        let mut q = self.unit_queue(u);
        let at = if after != NIL { after } else if q.tail != NIL { q.tail } else { q.head };
        if at == NIL {
            q.head = n;
            q.tail = n;
        } else {
            list::insert_after(&mut self.orders, at, n, &mut q);
        }
        self.set_unit_queue(u, q);
        Some(n)
    }

    /// 0x4745f0 with EDI = 0: append to the queue.
    pub fn order_queue_append(&mut self, d: &Data, u: u32, order: u8, unit_type: u16, pos: (i16, i16), target: u32) -> Option<u32> {
        self.order_insert_after(d, u, order, unit_type, pos, target, NIL)
    }

    /// 0x4749a0: queue `order` (no target) after the queue head (at the end when empty).
    pub(super) fn order_queue_after_head(&mut self, d: &Data, u: u32, order: u8) {
        let h = self.units[u as usize].order_queue_head;
        self.order_insert_after(d, u, order, 0xe4, (0, 0), NIL, h);
    }

    /// 0x474940: append `order` on unit `t` (at its position).
    pub(super) fn order_queue_append_target(&mut self, d: &Data, u: u32, order: u8, t: u32) {
        let p = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        self.order_queue_append(d, u, order, 0xe4, p, t);
    }

    /// 0x4748e0: append `order` at `pos`.
    pub(super) fn order_queue_append_at(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16)) {
        self.order_queue_append(d, u, order, 0xe4, pos, NIL);
    }

    /// 0x474540: queue an order before `before` (AI units are limited to 8 queued orders).
    pub(super) fn order_insert_before(&mut self, d: &Data, u: u32, order: u8, unit_type: u16, pos: (i16, i16), target: u32, before: u32) -> Option<u32> {
        if self.units[u as usize].ai != NIL && self.units[u as usize].order_queue_count > 8 {
            return None;
        }
        let Some(n) = self.order_alloc(order, unit_type, pos, target) else {
            self.order_failed(u);
            return None;
        };
        self.bump_queue_count(d, u, order);
        let mut q = self.unit_queue(u);
        list::insert_before(&mut self.orders, before, n, &mut q);
        self.set_unit_queue(u, q);
        Some(n)
    }

    /// 0x474760: queue `order` (no target) before `before`.
    pub(super) fn queue_order_before(&mut self, d: &Data, u: u32, order: u8, before: u32) -> Option<u32> {
        self.order_insert_before(d, u, order, 0xe4, (0, 0), NIL, before)
    }

    /// 0x474790: queue `order` at `pos` (no target) before `before`.
    pub(super) fn queue_order_at_before(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), before: u32) -> Option<u32> {
        self.order_insert_before(d, u, order, 0xe4, pos, NIL, before)
    }

    /// 0x4747c0: queue `order` at `pos` on `target` before `before`.
    pub(super) fn queue_order_target_before(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32, before: u32) -> Option<u32> {
        self.order_insert_before(d, u, order, 0xe4, pos, target, before)
    }

    /// 0x4747e0: queue `order` on unit `t` (at its position) before `before`.
    pub(super) fn queue_order_on_unit(&mut self, d: &Data, u: u32, order: u8, t: u32, before: u32) -> Option<u32> {
        let p = self.unit_sprite_pos(t);
        self.order_insert_before(d, u, order, 0xe4, p, t, before)
    }

    /// 0x474d10: issue `order` on unit `t` (at its position; NIL for none), clearing the
    /// interruptible orders queued.
    pub(super) fn issue_order_on_unit(&mut self, d: &Data, u: u32, order: u8, t: u32) {
        let p = if t == NIL { (0, 0) } else { self.unit_sprite_pos(t) };
        self.issue_order(d, u, order, true, p, t, 0xe4);
    }

    /// 0x474810: queue an order. `clear_interruptible` removes interruptible orders (and
    /// repeats of `order`) from the end of the queue first. Dead units (order 0) ignore it.
    pub fn issue_order(&mut self, d: &Data, u: u32, order: u8, clear_interruptible: bool, pos: (i16, i16), target: u32, unit_type: u16) {
        if self.units[u as usize].order == 0 {
            return;
        }
        loop {
            let t = self.units[u as usize].order_queue_tail;
            if t == NIL {
                break;
            }
            let to = self.orders[t as usize].order;
            if (clear_interruptible && d.dat.orders.interruptible[to as usize] != 0) || to == order {
                self.order_remove(d, u, t);
            } else {
                break;
            }
        }
        if order == 0x6d {
            // Re-queue the current order, then put 0x6d in front of it.
            let un = &self.units[u as usize];
            let (cur, cty, cpos, ctgt) = (un.order, un.order_unit_type, un.order_target_pos, un.order_target_unit);
            let head = un.order_queue_head;
            if head == NIL {
                self.order_insert_after(d, u, cur, cty, cpos, ctgt, NIL);
            } else {
                self.order_insert_before(d, u, cur, cty, cpos, ctgt, head);
            }
            let head = self.units[u as usize].order_queue_head;
            self.order_insert_before(d, u, 0x6d, 0xe4, pos, target, head);
            self.units[u as usize].status &= !super::unit::status::CAN_NOT_RECEIVE_ORDERS;
            self.order_done_or_next(d, u);
            return;
        }
        self.order_insert_after(d, u, order, unit_type, pos, target, NIL);
    }

    /// 0x4753a0: start the next queued order, or fall back to the idle order.
    pub fn order_done_or_next(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
            return;
        }
        let order = if self.units[u as usize].ai != NIL {
            0x9c
        } else {
            d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize]
        };
        self.order_now(d, u, order);
    }

    /// 0x4e7420: AI unit `u` is about to move to `p`: when a transport should carry it there,
    /// put the current order back in the queue behind a move (0x60) to `p`, tell its turret
    /// to stop (0x17) and return true (the caller then does not move). Returns false
    /// when it should walk (or cannot move).
    pub(super) fn ai_ride_instead_of_move(&mut self, d: &Data, u: u32, p: (i16, i16)) -> bool {
        let un = &self.units[u as usize];
        if un.order == 0x60 {
            return true;
        }
        let uflags = d.dat.units.flags[un.unit_type as usize];
        if un.status & 4 != 0 || uflags & 0x10 != 0 {
            return false;
        }
        let sp = self.unit_sprite_pos(u);
        if sp == p || un.status & 0x2_0000 == 0 {
            return false;
        }
        if un.unit_type == 7 && un.order == 0x22 {
            let t = un.order_target_unit;
            if t != NIL && self.units[t as usize].status & 4 != 0 {
                return false;
            }
        }
        let r = super::path::region_at(&self.sai, p.0 as i32, p.1 as i32);
        if self.sai.regions[r as usize].flags == 0x1ffd {
            return false;
        }
        let ai = self.units[u as usize].ai;
        if self.same_group(u, p) {
            if ai != NIL && super::ai_town::ai_kind(ai) == super::ai_town::AI_MILITARY {
                let (rp, rid) = self.ai.military.items[super::ai_town::ai_index(ai) as usize].region;
                if matches!(self.reg(rp, rid).state, 1 | 2) {
                    return false;
                }
            }
            if self.region_reachable(d, u, p) {
                if self.units[u as usize].unit_type != 0x53 {
                    return false;
                }
                if (super::path::approx_distance(p.0 as i32, sp.0 as i32, p.1 as i32, sp.1 as i32) as i32) < 0x3c0 {
                    return false;
                }
            }
        }
        if !self.nearest_transport_for(d, u).0 {
            return false;
        }
        let order = self.units[u as usize].order;
        let (tp, tu) = (self.units[u as usize].order_target_pos, self.units[u as usize].order_target_unit);
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            // 0x4747c0
            self.order_insert_before(d, u, order, 0xe4, tp, tu, head);
        } else {
            // 0x474cf0
            self.issue_order(d, u, order, false, tp, tu, 0xe4);
        }
        // 0x474790
        let head = self.units[u as usize].order_queue_head;
        self.order_insert_before(d, u, 0x60, 0xe4, p, NIL, head);
        let sub = self.units[u as usize].subunit;
        if self.is_turret(d, sub) {
            self.order_now(d, sub, 0x17);
        }
        self.order_next_or_idle(d, u);
        self.set_auto_targeting(u);
        true
    }

    /// 0x475310: replace the queue with `order` (no target) and start it.
    pub fn order_now(&mut self, d: &Data, u: u32, order: u8) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, order, true, (0, 0), NIL, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// 0x474c30: issue `order` targeting `target`/`pos`, clearing interruptible orders, and start it.
    pub fn order_target(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, order, true, pos, target, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// 0x474fb0: issue `order` at a position with a unit type, and start it.
    pub fn order_pos_type(&mut self, d: &Data, u: u32, order: u8, unit_type: u16, x: i16, y: i16) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, order, true, (x, y), NIL, unit_type);
        self.prepare_next_order(d, u);
    }

    /// 0x475260: replace the queue with `order` at `pos` and start it.
    pub fn order_at(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16)) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, order, true, pos, NIL, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// 0x474c70: queue `order` at `pos` (`clear`: drop the interruptible orders first).
    pub fn queue_order_at(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), clear: bool) {
        self.issue_order(d, u, order, clear, pos, NIL, 0xe4);
    }

    /// 0x474cf0: queue `order` at `pos` on `target` (`clear`: drop the interruptible orders first).
    pub fn queue_order_target(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32, clear: bool) {
        self.issue_order(d, u, order, clear, pos, target, 0xe4);
    }

    /// 0x475350: the current order is over at `pos`: start the next queued one, else the AI
    /// idle order (0x9c), else the unit's idle order at `pos`.
    pub fn order_done_at(&mut self, d: &Data, u: u32, pos: (i16, i16)) {
        if self.units[u as usize].order_queue_head != NIL {
            self.prepare_next_order(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_at(d, u, o, pos);
        }
    }

    /// 0x474d60: queue `order` without a target.
    pub fn queue_order(&mut self, d: &Data, u: u32, order: u8, clear: bool) {
        self.issue_order(d, u, order, clear, (0, 0), NIL, 0xe4);
    }

    /// 0x494bb0: aim the unit's next target waypoint at `p` (only written when it differs).
    pub fn set_target_waypoint(&mut self, u: u32, p: (i16, i16)) {
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// 0x475220: issue_order + prepare_next_order.
    pub fn order_issue_now(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32, unit_type: u16) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, order, true, pos, target, unit_type);
        self.prepare_next_order(d, u);
    }

    /// 0x479fa0: set the current order with a target unit.
    fn set_order_target_unit(&mut self, u: u32, order: u8, target: u32) {
        let pos = if target != NIL {
            let s = self.units[target as usize].sprite;
            (self.sprites[s as usize].x, self.sprites[s as usize].y)
        } else {
            (0, 0)
        };
        let un = &mut self.units[u as usize];
        un.order = order;
        un.order_target_unit = target;
        un.order_target_pos = pos;
        un.order_state = 0;
        un.order_unit_type = 0xe4;
    }

    /// 0x475000: pop the queue head into the current order.
    pub fn prepare_next_order(&mut self, d: &Data, u: u32) {
        let head = self.units[u as usize].order_queue_head;
        let un = &self.units[u as usize];
        if un.ai != NIL && head != NIL && un.status & super::unit::status::BURROWED != 0 {
            let o = self.orders[head as usize].order;
            let t = un.unit_type;
            if o != 0 && o != 0x76 && o != 0x75 && o != 0x9c && t != 0xd && (t != 0x67 || (o != 2 && o != 0xb)) {
                // Burrowed AI unit: unburrow (0x76) first.
                let s = un.sprite;
                let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
                self.queue_order_at_before(d, u, 0x76, p, head);
                self.units[u as usize].status &= !super::unit::status::CAN_NOT_RECEIVE_ORDERS;
                self.order_done_or_next(d, u);
                return;
            }
        }
        let st = self.units[u as usize].status;
        if st & 0x3000 != 0 && (head == NIL || self.orders[head as usize].order != 0) {
            return;
        }
        if head == NIL {
            return;
        }
        self.order_remove(d, u, head);
        let o = self.orders[head as usize];
        let un = &mut self.units[u as usize];
        let not_interruptible = d.dat.orders.interruptible[o.order as usize] == 0;
        un.order_process_timer = 0;
        un.recent_order_timer = 0;
        un.order_flags &= !1;
        un.status = (un.status & 0xf7ff_eff7) | ((not_interruptible as u32) << 12);
        if o.target != NIL {
            self.set_order_target_unit(u, o.order, o.target);
        } else {
            un.order_state = 0;
            un.order_target_pos = o.pos;
            un.order_target_unit = NIL;
            un.order = o.order;
            un.order_unit_type = if o.unit_type == 0xe4 { 0xe4 } else { o.unit_type };
        }
        if self.units[u as usize].ai == NIL {
            self.units[u as usize].auto_target = NIL;
        }
        self.order_start(d, u);
        let t = self.units[u as usize].unit_type as usize;
        let sub = self.units[u as usize].subunit;
        if d.dat.units.flags[t] & 0x10 != 0 || sub == NIL {
            return;
        }
        let st_ = self.units[sub as usize].unit_type as usize;
        if d.dat.units.flags[st_] & 0x10 == 0 {
            return;
        }
        let u_ = &d.dat.units;
        let order = if o.order == u_.ai_return_to_idle[t] {
            u_.ai_return_to_idle[st_]
        } else if o.order == u_.ai_attack_unit[t] {
            u_.ai_attack_unit[st_]
        } else if o.order == u_.ai_attack_move[t] {
            u_.ai_attack_move[st_]
        } else if d.dat.orders.subunit_inherits[o.order as usize] != 0 {
            o.order
        } else {
            return;
        };
        if o.target != NIL {
            self.order_target(d, sub, order, o.pos, o.target);
        } else {
            self.order_pos_type(d, sub, order, o.unit_type, o.pos.0, o.pos.1);
        }
    }

    /// 0x476550: clear "nobrk", sprite flag 0x80 and the flingy STARTING flag; leave attack
    /// anims, run with `u` as the iscript unit and order unit (0x6d11fc / 0x6d11f4 saved,
    /// set and put back around the 0x497880 call; Ghidra's decompile drops this).
    pub(crate) fn order_start(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.status &= !super::unit::status::NO_BRK_CODE_START;
        let s = un.sprite;
        self.sprites[s as usize].flags &= 0x7f;
        let saved = (self.iscript_unit, self.iscript_order_unit);
        (self.iscript_unit, self.iscript_order_unit) = (Some(u), Some(u));
        self.sprite_end_attack_anim(d, s);
        (self.iscript_unit, self.iscript_order_unit) = saved;
        self.units[u as usize].fl.flags &= !mflags::STARTING;
    }

    /// 0x497880: switch a sprite's images out of attack/cast animations.
    fn sprite_end_attack_anim(&mut self, d: &Data, s: u32) {
        let mi = self.sprites[s as usize].main_image;
        let id = self.sprites[s as usize].sprite_id;
        let anim = match self.images[mi as usize].iscript.anim {
            3 | 6 => 9,
            2 | 5 => 8,
            7 => 0xc,
            0xd if id == 0x1e9 => 0xc,
            0xf if matches!(id, 0xef | 0x8c | 0xc2) => 8,
            _ => return,
        };
        self.sprite_images_run_anim(d, s, anim);
    }
}
