//! Carriers and reavers with their interceptors and scarabs: hangar lists, the
//! Carrier / Reaver orders and the fighters' own orders.
//!
//! Parent fields: +0xc0 docked list, +0xc4 launched list, +0xc8 docked count, +0xc9
//! launched count. Fighter fields: +0xc0 parent, +0xc4 next, +0xc8 previous, +0xcc 1
//! while launched.

use super::list::NIL;
use super::{Data, State};

/// 0x663320: units.dat attack-unit order (here for interceptors 0x49 / scarabs 0x55).
const INTERCEPTOR: u16 = 0x49;
const SCARAB: u16 = 0x55;

impl State {
    fn is_reaver(&self, u: u32) -> bool {
        super::attack::is_reaver_type(self.units[u as usize].unit_type)
    }

    /// 0x495240 for a unit: head straight for `p` (no path).
    fn unit_move_point(&mut self, u: u32, p: (i16, i16)) {
        let f = &mut self.units[u as usize].fl;
        if f.move_target != p {
            f.move_target = p;
            f.flags = (f.flags & !4) | 1;
            f.next_waypoint = p;
            self.units[u as usize].move_target_unit = NIL;
        }
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// 0x4eb9c0: move to `p` and face it; the 0x4eb820 result.
    pub(super) fn move_and_face(&mut self, d: &Data, u: u32, p: (i16, i16)) -> bool {
        let r = self.set_move_target_xy(d, u, p);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        r
    }

    /// 0x4752b0: give `u` `order` on `t` (or nothing) and start it.
    pub(super) fn order_on(&mut self, d: &Data, u: u32, order: u8, t: u32) {
        self.units[u as usize].order_flags |= 1;
        let p = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        self.issue_order(d, u, order, true, p, t, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// Give up the current order: the next queued one, else idle (0x475000 / 0x475310).
    fn fighter_parent_give_up(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_now(d, u, o);
        }
    }

    /// 0x466440: launch the healthiest completed fighter from `c`'s hangar (none while
    /// interceptors are below half hp); a scarab starts in front of the reaver.
    fn launch_fighter(&mut self, d: &Data, c: u32) -> u32 {
        let mut f = self.units[c as usize].ptr_at(0xc0);
        while f != NIL && self.units[f as usize].status & 1 == 0 {
            f = self.units[f as usize].ptr_at(0xc4);
        }
        if f == NIL {
            return NIL;
        }
        // 1.16.1 compares every later fighter against the first one's hp.
        let first_hp = self.units[f as usize].hp;
        let mut g = self.units[f as usize].ptr_at(0xc4);
        while g != NIL {
            if self.units[g as usize].status & 1 != 0 && first_hp < self.units[g as usize].hp {
                f = g;
            }
            g = self.units[g as usize].ptr_at(0xc4);
        }
        let ft = self.units[f as usize].unit_type;
        if ft == INTERCEPTOR && self.units[f as usize].hp < d.dat.units.hit_points[INTERCEPTOR as usize] as i32 / 2 {
            return NIL;
        }
        let mut p = self.unit_sprite_pos(c);
        if ft == SCARAB {
            let h = self.units[c as usize].fl.heading;
            let (vx, vy) = crate::game::flingy::direction_table()[h as usize];
            p.0 = p.0.wrapping_add(((vx * 0x19) >> 8) as i16);
            p.1 = p.1.wrapping_add(((vy * 0x19) >> 8) as i16);
            self.set_heading_now(d, f, h);
        }
        // 0x68c1b0 etc.: UI refresh.
        if self.units[c as usize].ptr_at(0xc0) == f {
            let next = self.units[f as usize].ptr_at(0xc4);
            self.units[c as usize].set_ptr_at(0xc0, next);
        }
        let (next, prev) = (self.units[f as usize].ptr_at(0xc4), self.units[f as usize].ptr_at(0xc8));
        if next != NIL {
            self.units[next as usize].set_ptr_at(0xc8, prev);
        }
        if prev != NIL {
            self.units[prev as usize].set_ptr_at(0xc4, next);
        }
        let n = self.units[c as usize].u8_at(0xc8);
        self.units[c as usize].set_u8_at(0xc8, n.wrapping_sub(1));
        let out = self.units[c as usize].ptr_at(0xc4);
        self.units[f as usize].set_ptr_at(0xc4, out);
        self.units[f as usize].set_ptr_at(0xc8, NIL);
        if out != NIL {
            self.units[out as usize].set_ptr_at(0xc8, f);
        }
        let n = self.units[c as usize].u8_at(0xc9);
        self.units[c as usize].set_u8_at(0xc9, n.wrapping_add(1));
        self.units[c as usize].set_ptr_at(0xc4, f);
        self.place_unit_at(d, f, p.0, p.1);
        self.show_unit(d, f);
        self.units[f as usize].set_u8_at(0xcc, 1);
        f
    }

    /// 0x4652a0: a fighter that dies leaves its parent's lists.
    pub(super) fn fighter_unlink(&mut self, f: u32) {
        let parent = self.units[f as usize].ptr_at(0xc0);
        if parent == NIL {
            self.units[f as usize].set_ptr_at(0xc4, NIL);
            self.units[f as usize].set_ptr_at(0xc8, NIL);
            return;
        }
        let launched = self.units[f as usize].u8_at(0xcc) != 0;
        let (next, prev) = (self.units[f as usize].ptr_at(0xc4), self.units[f as usize].ptr_at(0xc8));
        if prev == NIL {
            self.units[parent as usize].set_ptr_at(if launched { 0xc4 } else { 0xc0 }, next);
        } else {
            self.units[prev as usize].set_ptr_at(0xc4, next);
        }
        let k = if launched { 0xc9 } else { 0xc8 };
        let n = self.units[parent as usize].u8_at(k);
        self.units[parent as usize].set_u8_at(k, n.wrapping_sub(1));
        if next != NIL {
            self.units[next as usize].set_ptr_at(0xc8, prev);
        }
    }

    /// 0x466170: a dying carrier / reaver: docked fighters die, launched interceptors
    /// fly on for 15..45 frames.
    pub(super) fn kill_fighters(&mut self, d: &Data, c: u32) {
        while self.units[c as usize].u8_at(0xc8) != 0 {
            let f = self.units[c as usize].ptr_at(0xc0);
            let next = self.units[f as usize].ptr_at(0xc4);
            self.units[c as usize].set_ptr_at(0xc0, next);
            self.units[f as usize].set_ptr_at(0xc0, NIL);
            self.kill_unit(d, f);
            let n = self.units[c as usize].u8_at(0xc8);
            self.units[c as usize].set_u8_at(0xc8, n - 1);
        }
        let mut f = self.units[c as usize].ptr_at(0xc4);
        while self.units[c as usize].u8_at(0xc9) != 0 {
            self.units[f as usize].set_ptr_at(0xc0, NIL);
            if self.units[f as usize].unit_type != SCARAB {
                let r = self.rng.rand(0x27);
                let t = ((r * 0x1f) >> 15) as u16 + 0xf;
                let fu = &mut self.units[f as usize];
                if fu.remove_timer == 0 || t < fu.remove_timer {
                    fu.remove_timer = t;
                }
            }
            let next = self.units[f as usize].ptr_at(0xc4);
            self.units[f as usize].set_ptr_at(0xc8, NIL);
            self.units[f as usize].set_ptr_at(0xc4, NIL);
            let n = self.units[c as usize].u8_at(0xc9);
            self.units[c as usize].set_u8_at(0xc9, n - 1);
            f = next;
        }
        self.units[c as usize].set_ptr_at(0xc4, NIL);
    }

    /// 0x465780: launched interceptors go after the carrier's target.
    fn fighters_attack_target(&mut self, d: &Data, c: u32) {
        let mut f = self.units[c as usize].ptr_at(0xc4);
        while f != NIL {
            let cs = self.units[c as usize].sprite;
            let e = self.sprites[cs as usize].elevation.wrapping_sub(1);
            let fs = self.units[f as usize].sprite;
            self.sprites[fs as usize].elevation = e;
            let t = self.units[c as usize].order_target_unit;
            self.units[f as usize].order_flags |= 1;
            let p = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
            let o = d.dat.units.ai_attack_unit[INTERCEPTOR as usize];
            self.issue_order(d, f, o, true, p, t, 0xe4);
            self.prepare_next_order(d, f);
            f = self.units[f as usize].ptr_at(0xc4);
        }
    }

    /// 0x465480: the ground path to the order target is shorter than `leash`.
    fn within_leash(&mut self, u: u32, leash: i32) -> bool {
        let p = self.units[u as usize].order_target_pos;
        leash > self.path_distance(u, p)
    }

    /// 0x465390 / 0x465360: the order a carrier / reaver idles or fights in (hold
    /// position variants stay).
    fn carrier_idle_order(&self, u: u32) -> u8 {
        let o = self.units[u as usize].order;
        if o == 0x37 {
            return 0x37;
        }
        if self.is_reaver(u) {
            if o == 0x3e { 0x3e } else { 0x3a }
        } else if o == 0x39 {
            0x39
        } else {
            0x32
        }
    }

    fn carrier_fight_order(&self, u: u32) -> u8 {
        let o = self.units[u as usize].order;
        if self.is_reaver(u) {
            if o == 0x3e { 0x3e } else { 0x3d }
        } else if o == 0x39 {
            0x39
        } else {
            0x38
        }
    }

    /// 0x465a60: keep or find a target within `range` (and `leash` for reavers); true
    /// when there is one to launch at.
    fn carrier_acquire(&mut self, d: &Data, u: u32, range: u32, leash: i32) -> bool {
        let o = self.units[u as usize].order;
        let hold = o == 0x39 || o == 0x3e;
        if hold && self.units[u as usize].fl.flags & 2 != 0 {
            self.unit_halt(d, u);
        }
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            if self.units[t as usize].status & 0x400_0000 == 0 && self.can_attack(d, u, t, true) {
                self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
                let ok = self.order_target_visible(u) && self.in_attack_range(d, u, range, t) && (leash == 0 || self.within_leash(u, leash));
                if ok {
                    self.unit_halt(d, u);
                } else {
                    // 0x57eee8: computer players (type 1) keep chasing a target they
                    // were told to attack (status 8).
                    let computer = self.player_type(self.units[u as usize].player) == 1;
                    if (hold || self.units[u as usize].status & 0x800_0000 != 0) && (!computer || self.units[u as usize].status & 8 == 0) {
                        let un = &mut self.units[u as usize];
                        if un.order_target_unit == un.auto_target {
                            un.auto_target = NIL;
                        }
                        un.order_target_unit = NIL;
                        return false;
                    }
                    let p = self.units[u as usize].order_target_pos;
                    if !self.move_and_face(d, u, p) {
                        return false;
                    }
                }
            } else {
                let un = &mut self.units[u as usize];
                if un.order_target_unit == un.auto_target {
                    un.auto_target = NIL;
                }
                un.order_target_unit = NIL;
            }
        }
        let un = &self.units[u as usize];
        if un.u8_at(0xc9) as u32 + un.u8_at(0xc8) as u32 == 0 {
            self.unit_halt(d, u);
            self.fighter_parent_give_up(d, u);
            return false;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return false;
        }
        let old = self.units[u as usize].order_target_unit;
        if old == NIL {
            if self.units[u as usize].order_queue_head != NIL {
                self.prepare_next_order(d, u);
                return false;
            }
        }
        if old == NIL || self.units[u as usize].status & 0x800_0000 != 0 {
            self.pick_better_target(d, u);
            let t = self.units[u as usize].unit_type;
            if matches!(t, 0x48 | 0x52) && self.units[u as usize].order_target_unit != old {
                self.fighters_attack_target(d, u);
            }
        }
        if self.units[u as usize].order_target_unit == NIL {
            let t = self.find_target_in_range(d, u);
            if t == NIL {
                if !hold {
                    self.units[u as usize].order = self.carrier_idle_order(u);
                }
                return false;
            }
            if !hold {
                self.units[u as usize].order = self.carrier_fight_order(u);
            }
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
            // 0x4026a0: auto-targeting from now on.
            self.units[u as usize].status |= 0x800_0000;
            let sub = self.units[u as usize].subunit;
            if sub != NIL {
                self.units[sub as usize].status |= 0x800_0000;
            }
            if leash != 0 && !self.within_leash(u, leash) {
                return false;
            }
            self.units[u as usize].order_target_unit = t;
            return true;
        }
        if !hold {
            self.units[u as usize].order = self.carrier_fight_order(u);
        }
        let t = self.units[u as usize].order_target_unit;
        self.order_target_visible(u) && self.in_attack_range(d, u, range, t) && (leash == 0 || self.within_leash(u, leash))
    }

    /// 0x4666a0: Carrier, CarrierFight, CarrierHoldPosition.
    pub(super) fn order_carrier(&mut self, d: &Data, u: u32) {
        let range = (self.acquisition_range(u) as u8 as u32) << 5;
        if !self.carrier_acquire(d, u, range, 0) {
            if self.units[u as usize].ai != NIL && self.units[u as usize].main_order_timer == 0 {
                self.order_now(d, u, 0x9c);
            }
            return;
        }
        let f = self.launch_fighter(d, u);
        if f == NIL {
            return;
        }
        let ft = self.units[f as usize].unit_type as usize;
        self.units[f as usize].shields = (d.dat.units.shields[ft] as i32) << 8;
        let cs = self.units[u as usize].sprite;
        let e = self.sprites[cs as usize].elevation.wrapping_sub(1);
        let fs = self.units[f as usize].sprite;
        self.sprites[fs as usize].elevation = e;
        let t = self.units[u as usize].order_target_unit;
        self.order_on(d, f, d.dat.units.ai_attack_unit[INTERCEPTOR as usize], t);
        self.units[u as usize].main_order_timer = 7;
    }

    /// 0x4665d0: Reaver, ReaverFight, ReaverHoldPosition.
    pub(super) fn order_reaver(&mut self, d: &Data, u: u32) {
        let r = self.acquisition_range(u) as u8 as u32;
        if !self.carrier_acquire(d, u, r * 0x20, (r * 0x40) as i32) {
            if self.units[u as usize].ai != NIL && self.units[u as usize].main_order_timer == 0 {
                self.order_now(d, u, 0x9c);
            }
            return;
        }
        let un = &mut self.units[u as usize];
        if un.order_target_pos != un.fl.next_target_waypoint {
            un.fl.next_target_waypoint = un.order_target_pos;
        }
        let s = self.unit_sprite_pos(u);
        let w = self.units[u as usize].fl.next_target_waypoint;
        let dir = crate::game::flingy::direction_to(s.0 as i32, s.1 as i32, w.0 as i32, w.1 as i32);
        if (self.units[u as usize].fl.heading as u32).wrapping_sub(dir as u32) > 1 {
            self.units[u as usize].order_process_timer = 0;
            return;
        }
        if self.units[u as usize].status & 0x8000 == 0 && self.units[u as usize].ptr_at(0xc4) == NIL {
            let f = self.launch_fighter(d, u);
            if f != NIL {
                let t = self.units[u as usize].order_target_unit;
                self.order_on(d, f, d.dat.units.ai_attack_unit[SCARAB as usize], t);
                self.units[f as usize].set_ptr_at(0xc0, u);
                self.units[u as usize].main_order_timer = 0x3c;
            }
        }
    }

    /// 0x465910: CarrierStop.
    pub(super) fn order_carrier_stop(&mut self, d: &Data, u: u32) {
        self.unit_halt(d, u);
        let f = &mut self.units[u as usize].fl;
        if f.move_target != f.next_target_waypoint {
            f.next_target_waypoint = f.move_target;
        }
        self.recall_interceptors(d, u);
        let t = self.units[u as usize].order_target_unit;
        self.order_on(d, u, 0x32, t);
    }

    /// 0x4654b0: ReaverStop: scarabs stop (0xa5), the reaver idles (0x3a) on its target.
    pub(super) fn order_reaver_stop(&mut self, d: &Data, u: u32) {
        self.unit_halt(d, u);
        let f = &mut self.units[u as usize].fl;
        if f.move_target != f.next_target_waypoint {
            f.next_target_waypoint = f.move_target;
        }
        let mut s = self.units[u as usize].ptr_at(0xc4);
        while s != NIL {
            self.requeue_tail(d, s, 0xa5, (0, 0), NIL);
            self.prepare_next_order(d, s);
            s = self.units[s as usize].ptr_at(0xc4);
        }
        let t = self.units[u as usize].order_target_unit;
        let p = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        self.requeue_tail(d, u, 0x3a, p, t);
        self.prepare_next_order(d, u);
    }

    /// Order-flag bit 0, then (if not dead) drop interruptible queued orders and repeats
    /// of `order` from the tail and queue `order`.
    fn requeue_tail(&mut self, d: &Data, u: u32, order: u8, p: (i16, i16), t: u32) {
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].order == 0 {
            return;
        }
        loop {
            let tail = self.units[u as usize].order_queue_tail;
            if tail == NIL {
                break;
            }
            let o = self.orders[tail as usize].order;
            if d.dat.orders.interruptible[o as usize] == 0 && o != order {
                break;
            }
            self.order_remove(d, u, tail);
        }
        self.order_queue_append(d, u, order, 0xe4, p, t);
    }

    /// 0x465950 / 0x465690: CarrierAttack, ReaverAttack: switch to the fight order on
    /// the target (interceptors join), keeping the queue.
    fn carrier_attack(&mut self, d: &Data, u: u32, carrier: bool) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            self.fighter_parent_give_up(d, u);
            return;
        }
        if !self.can_attack(d, u, t, true) {
            // 0x475260: attack-move (6) to the target's spot instead.
            let p = self.units[u as usize].order_target_pos;
            self.order_issue_now(d, u, 6, p, NIL, 0xe4);
            return;
        }
        let st = self.units[u as usize].status;
        if carrier {
            self.fighters_attack_target(d, u);
        }
        // 0x474a70: the fight order on the target goes in front of the queue.
        let o = self.carrier_fight_order(u);
        let tgt = self.units[u as usize].order_target_unit;
        let p = self.unit_sprite_pos(tgt);
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            self.order_insert_before(d, u, o, 0xe4, p, tgt, head);
        } else {
            self.order_queue_append(d, u, o, 0xe4, p, tgt);
        }
        self.prepare_next_order(d, u);
        // Status bits 3 and 0x8000000 come back as they were.
        let un = &mut self.units[u as usize];
        un.status = (un.status & !0x800_0008) | (st & 0x800_0008);
    }

    /// 0x465950: CarrierAttack.
    pub(super) fn order_carrier_attack(&mut self, d: &Data, u: u32) {
        self.carrier_attack(d, u, true);
    }

    /// 0x465690: ReaverAttack.
    pub(super) fn order_reaver_attack(&mut self, d: &Data, u: u32) {
        self.carrier_attack(d, u, false);
    }

    /// 0x466720: CarrierIgnore2: move to the order position first, then act as a
    /// carrier / reaver.
    pub(super) fn order_carrier_ignore2(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            let p = self.units[u as usize].order_target_pos;
            if !self.move_and_face(d, u, p) {
                return;
            }
            self.units[u as usize].order_state = 1;
        }
        if self.is_reaver(u) {
            self.order_reaver(d, u);
            if self.units[u as usize].order_target_unit != NIL {
                self.units[u as usize].order = 0x3d;
            }
        } else {
            self.order_carrier(d, u);
        }
    }

    /// 0x465d30: a random point within `spread` * [-127, 128] of `p`, kept off the map
    /// edges (by the interceptor's box), as the move target.
    fn fighter_wander(&mut self, d: &Data, u: u32, spread: u8, p: (i16, i16)) {
        let mut dx = self.rng.rand_between(0xb, -0x7f, 0x80) * spread as i32;
        let mut dy = self.rng.rand_between(0xb, -0x7f, 0x80) * spread as i32;
        let dim = d.dat.units.dimensions[INTERCEPTOR as usize];
        let (l, up, r, dn) = (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32);
        let (x, y) = (p.0 as i32, p.1 as i32);
        if x + dx < l || x + dx >= self.map_px.0 as i32 - r {
            dx = -dx;
        }
        if y + dy < up || y + dy >= self.map_px.1 as i32 - dn {
            dy = -dy;
        }
        let mut q = ((x + dx) as i16, (y + dy) as i16);
        let t = self.units[u as usize].unit_type;
        self.clamp_unit_pos(d, t, &mut q);
        self.move_and_face(d, u, q);
    }

    /// 0x465810: an interceptor's target is still worth chasing (else it retargets or
    /// heads home); false when the order changed.
    fn interceptor_target_ok(&mut self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].order_target_unit;
        if (t == NIL || self.units[t as usize].status & 0x400_0000 != 0) && !self.attack_auto_target(d, u) {
            self.requeue_tail(d, u, 0x45, (0, 0), NIL);
            self.prepare_next_order(d, u);
            return false;
        }
        let t = self.units[u as usize].order_target_unit;
        let parent = self.units[u as usize].ptr_at(0xc0);
        self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        if parent != NIL {
            // The parent's acquisition range + 2 tiles.
            let range = ((self.acquisition_range(parent) as u8 as u32) + 2) << 5;
            if !self.in_attack_range(d, parent, range, t) {
                self.order_now(d, u, 0x45);
                return false;
            }
        }
        true
    }

    /// 0x401240: `u` is within `r` of point `p` (from its exact position).
    pub(super) fn near_point(&self, u: u32, r: u32, p: (i16, i16)) -> bool {
        let f = &self.units[u as usize].fl;
        super::path::approx_distance(f.exact.0, (p.0 as i32) << 8, f.exact.1, (p.1 as i32) << 8) >> 8 <= r
    }

    /// 0x495400: chase `t`, leading it by its speed over the time to get there.
    pub(super) fn intercept_move(&mut self, u: u32, t: u32) {
        let speed2 = (self.units[u as usize].fl.top_speed << 1) >> 8;
        let tf = self.units[t as usize].fl.clone();
        let tp = self.unit_sprite_pos(t);
        if tf.move_target == tf.pos || speed2 < 1 {
            self.unit_move_point(u, tp);
            return;
        }
        if tf.flags & 4 != 0 {
            self.unit_move_point(u, tf.move_target);
            return;
        }
        let up = self.unit_sprite_pos(u);
        let time = self.distance_40c300(up.0 as i32, tp.0 as i32, up.1 as i32, tp.1 as i32) / speed2;
        let mut speed = if self.units[u as usize].fl.movement_type == 2 {
            tf.top_speed
        } else {
            ((tf.acceleration as i32 * time) / 2 + tf.next_speed).min(tf.top_speed)
        };
        speed *= time;
        let (vx, vy) = crate::game::flingy::direction_table()[tf.velocity_dir_prev as usize];
        let mut x = ((vx * speed) >> 16) + tp.0 as i32;
        let mut y = ((vy * speed) >> 16) + tp.1 as i32;
        if x < 0 {
            x = 0;
        } else if x >= self.map_px.0 as i32 {
            x = self.map_px.0 as i32 - 1;
        }
        // Over the top edge y is just 0; the bottom is clamped otherwise.
        if y < 0 {
            y = 0;
        } else if y >= self.map_px.1 as i32 {
            y = self.map_px.1 as i32 - 1;
        }
        self.unit_move_point(u, (x as i16, y as i16));
    }

    /// 0x40c300: approximate distance.
    fn distance_40c300(&self, x1: i32, x2: i32, y1: i32, y2: i32) -> i32 {
        let dx = (x1 - x2).unsigned_abs();
        let dy = (y1 - y2).unsigned_abs();
        let (big, small) = if dx < dy { (dy, dx) } else { (dx, dy) };
        let s3 = small * 3;
        let mut r = (big >> 5) + big + (s3 >> 8);
        if ((big >> 2) as i32) < small as i32 {
            r = ((r - (big >> 5)) + (s3 >> 3)).wrapping_sub(big >> 7) + (s3 >> 6);
        }
        r as i32
    }

    /// 0x465f60: InterceptorAttack: fly out, make passes at the target, come home when
    /// shields run low.
    pub(super) fn order_interceptor_attack(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ptr_at(0xc0) != NIL
            && self.units[u as usize].shields < ((d.dat.units.shields[INTERCEPTOR as usize] as i32) << 8) >> 2
        {
            self.order_now(d, u, 0x45);
            return;
        }
        if !self.interceptor_target_ok(d, u) {
            return;
        }
        let tp = self.units[u as usize].order_target_pos;
        let st = self.units[u as usize].order_state;
        match st {
            0 | 1 | 3 => {
                if st == 0 {
                    if self.units[u as usize].ptr_at(0xc0) == NIL {
                        self.kill_unit(d, u);
                        return;
                    }
                    // 0x48ed50 (EBX = 0x268, ESI = the interceptor): the launch sound.
                    self.unit_sound(0x268, u, 0);
                    self.fighter_wander(d, u, 3, tp);
                    self.units[u as usize].main_order_timer = 0xf;
                    self.units[u as usize].order_state = 1;
                }
                if st != 3 {
                    if self.units[u as usize].main_order_timer != 0 {
                        return;
                    }
                    let s = self.units[u as usize].sprite;
                    self.sprites[s as usize].elevation = self.sprites[s as usize].elevation.wrapping_add(2);
                    self.units[u as usize].order_state = 3;
                }
                let t = self.units[u as usize].order_target_unit;
                let mut shoot = t == NIL;
                if t != NIL && self.target_visible(t, u) {
                    let sub = self.units[u as usize].subunit;
                    let wu = if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 { sub } else { u };
                    let wt = self.units[wu as usize].unit_type;
                    let w = if self.units[t as usize].status & 4 != 0 {
                        d.dat.units.air_weapon[wt as usize]
                    } else if wt == 0x67 && self.units[wu as usize].status & 0x10 == 0 {
                        0x82
                    } else {
                        d.dat.units.ground_weapon[wt as usize]
                    };
                    if w < 0x82 {
                        let min = d.dat.weapons.min_range[w as usize];
                        if min == 0 || !self.box_distance_within(d, wu, min, t) {
                            let r = self.weapon_range(d, u, w);
                            shoot = self.in_attack_range(d, wu, r, t);
                        }
                    }
                }
                if shoot {
                    self.fire(d, u, 5);
                }
                let t = self.units[u as usize].order_target_unit;
                let tp = self.units[u as usize].order_target_pos;
                if self.in_attack_range(d, u, 0x32, t) {
                    self.fighter_wander(d, u, 1, tp);
                    self.units[u as usize].order_state = 4;
                } else {
                    self.move_and_face(d, u, tp);
                }
            }
            4 => {
                let mt = self.units[u as usize].fl.move_target;
                if self.near_point(u, 0x32, mt) {
                    self.fighter_wander(d, u, 2, tp);
                    self.units[u as usize].order_state = 5;
                }
            }
            5 => {
                let mt = self.units[u as usize].fl.move_target;
                if self.near_point(u, 0x32, mt) && self.move_and_face(d, u, tp) {
                    self.units[u as usize].order_state = 3;
                }
            }
            _ => {}
        }
    }

    /// 0x465e00: ScarabAttack: launch, home in on the target and blow up next to it.
    pub(super) fn order_scarab_attack(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let un = &mut self.units[u as usize];
                if un.remove_timer == 0 || un.remove_timer > 0x5a {
                    un.remove_timer = 0x5a;
                }
                un.status |= 0x20_0000;
                un.main_order_timer = 7;
                let t = un.order_target_unit;
                if t != NIL {
                    self.move_to_unit(d, u, t);
                    self.units[u as usize].order_state = 2;
                    return;
                }
                let p = self.units[u as usize].order_target_pos;
                if self.move_and_face(d, u, p) {
                    self.units[u as usize].order_state = 2;
                }
            }
            st @ (2 | 3) => {
                if st == 2 && self.units[u as usize].main_order_timer == 0 {
                    self.units[u as usize].status &= !0x20_0000;
                    self.units[u as usize].order_state = 3;
                }
                let t = self.units[u as usize].order_target_unit;
                let hit = if t == NIL {
                    self.unit_at_move_target(u) != 0
                } else {
                    if self.box_distance_within(d, u, 0x100, t) && self.units[t as usize].fl.flags & 2 != 0 {
                        self.intercept_move(u, t);
                    } else {
                        self.follow_move(d, u, t);
                    }
                    let w = d.dat.units.ground_weapon[self.units[u as usize].unit_type as usize];
                    let r = (d.dat.weapons.inner_splash[w as usize] >> 1) as u32;
                    self.box_distance_within(d, u, r, t)
                };
                if hit {
                    self.units[u as usize].order_target_pos = self.unit_sprite_pos(u);
                    let s = self.units[u as usize].sprite;
                    self.sprite_images_run_anim(d, s, 0xd);
                    self.units[u as usize].order_state = 6;
                }
            }
            6 => {
                if self.units[u as usize].order_signal & 1 != 0 {
                    self.units[u as usize].status |= 0x8000_0000;
                    self.kill_unit(d, u);
                }
            }
            _ => {}
        }
    }

    /// 0x466350: InterceptorReturn: fly home and dock.
    pub(super) fn order_interceptor_return(&mut self, d: &Data, u: u32) {
        let parent = self.units[u as usize].ptr_at(0xc0);
        if parent == NIL {
            self.kill_unit(d, u);
            return;
        }
        let p = self.unit_sprite_pos(parent);
        let dist = |st: &State| {
            let f = &st.units[u as usize].fl;
            super::path::approx_distance(f.exact.0, (p.0 as i32) << 8, f.exact.1, (p.1 as i32) << 8) & !0xff
        };
        if self.units[u as usize].order_state == 0 && dist(self) < 0x3c01 {
            self.units[u as usize].order_state = 1;
            if self.units[u as usize].unit_type == INTERCEPTOR {
                let ps = self.units[parent as usize].sprite;
                let e = self.sprites[ps as usize].elevation.wrapping_sub(2);
                let s = self.units[u as usize].sprite;
                self.sprites[s as usize].elevation = e;
            }
        }
        if dist(self) < 0xa01 {
            self.dock_returning(d, parent, u);
            self.order_now(d, u, 0x17);
            // 0x68c1b0 etc.: UI refresh.
        } else {
            self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
        }
    }

    /// 0x466270: a returning fighter moves from the launched list to the docked one.
    fn dock_returning(&mut self, d: &Data, parent: u32, f: u32) {
        let (next, prev) = (self.units[f as usize].ptr_at(0xc4), self.units[f as usize].ptr_at(0xc8));
        if prev == NIL {
            self.units[parent as usize].set_ptr_at(0xc4, next);
        } else {
            self.units[prev as usize].set_ptr_at(0xc4, next);
        }
        if next != NIL {
            self.units[next as usize].set_ptr_at(0xc8, prev);
        }
        let n = self.units[parent as usize].u8_at(0xc9);
        self.units[parent as usize].set_u8_at(0xc9, n.wrapping_sub(1));
        let head = self.units[parent as usize].ptr_at(0xc0);
        self.units[f as usize].set_ptr_at(0xc4, head);
        self.units[f as usize].set_ptr_at(0xc8, NIL);
        if head != NIL {
            self.units[head as usize].set_ptr_at(0xc8, f);
        }
        self.units[parent as usize].set_ptr_at(0xc0, f);
        let n = self.units[parent as usize].u8_at(0xc8);
        self.units[parent as usize].set_u8_at(0xc8, n.wrapping_add(1));
        self.units[f as usize].set_u8_at(0xcc, 0);
        self.hide_unit(d, f);
    }
}
