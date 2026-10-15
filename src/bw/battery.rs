//! Shield batteries (RechargeShieldsUnit 0x42, RechargeShieldsBattery 0x43, secondary
//! ShieldBattery 0x44) and arbiters (InitializeArbiter 0x82, secondary CloakNearbyUnits
//! 0x83).

use super::list::NIL;
use super::{Data, State};

const BATTERY: u16 = 0xac;

impl State {
    /// 0x4743d0: set the secondary order (state and target cleared) if it differs.
    pub(super) fn set_secondary(&mut self, u: u32, o: u8) {
        let un = &mut self.units[u as usize];
        if un.secondary_order != o {
            un.secondary_order = o;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
    }

    /// 0x493520: battery `b` can recharge `u` (own completed Protoss unit with missing
    /// shields, not a fighter; a working, charged battery; AI units not already on it).
    pub(super) fn can_recharge(&self, d: &Data, u: u32, b: u32) -> bool {
        let un = &self.units[u as usize];
        let bu = &self.units[b as usize];
        let st = un.status;
        let ok = un.player == bu.player
            && d.dat.units.shield_enable[un.unit_type as usize] != 0
            && st & 1 != 0
            && st & 0x2_0000 != 0
            && st & 2 == 0
            && self.race_of(d, u) == 2
            && un.shields < (d.dat.units.shields[un.unit_type as usize] as i32) << 8
            && bu.status & 1 != 0
            && bu.energy != 0
            && !self.disabled(b)
            && !super::unit::is_fighter(un.unit_type);
        if !ok {
            return false;
        }
        if un.ai == NIL {
            return true;
        }
        if un.order == 0x42 || un.order == 0x60 {
            return false;
        }
        let h = un.order_queue_head;
        h == NIL || self.orders[h as usize].order != 0x42
    }

    /// 0x4934b0: move up to 0x500 shield points (half as much energy) from `b` to `u`.
    fn recharge_step(&mut self, d: &Data, u: u32, b: u32) {
        let max = (d.dat.units.shields[self.units[u as usize].unit_type as usize] as i32) << 8;
        let missing = max - self.units[u as usize].shields;
        let (mut cost, mut gain) = (0x280i32, 0x500i32);
        if missing < 0x500 {
            cost = missing / 2;
            gain = missing;
        }
        let e = self.units[b as usize].energy as i32;
        if e < cost {
            gain = e * 2;
            cost = e;
        }
        let s = (self.units[u as usize].shields + gain).min(max);
        self.units[u as usize].shields = s;
        if self.game_options & 0x800 == 0 {
            self.units[b as usize].energy = (e - cost) as u16;
        }
    }

    fn battery_give_up(&mut self, d: &Data, u: u32) {
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

    /// 0x493dd0: RechargeShieldsUnit: go to the battery and recharge.
    pub(super) fn order_recharge_shields_unit(&mut self, d: &Data, u: u32) {
        let b = self.units[u as usize].order_target_unit;
        if b == NIL || self.units[b as usize].energy == 0 || self.disabled(b) {
            self.battery_give_up(d, u);
            return;
        }
        let mut st = self.units[u as usize].order_state;
        if st == 0 {
            if self.move_to_unit_aim(d, u, b) {
                self.units[u as usize].order_state = 1;
            }
            return;
        }
        if st == 1 {
            match self.unit_at_move_target(u) {
                0 => {
                    if !self.box_distance_within(d, u, 0x80, b) {
                        return;
                    }
                }
                1 => {}
                _ => {
                    self.order_done_or_next(d, u);
                    return;
                }
            }
            self.unit_halt_facing(d, u);
            self.units[u as usize].order_state = 2;
            st = 2;
        }
        if st == 2 {
            self.set_secondary(b, 0x44);
            // 0x47b720: the recharge overlay for the unit's size, above its main image.
            let f = d.dat.units.flags[self.units[u as usize].unit_type as usize];
            let size = if f & 0x200_0000 != 0 { 1 } else { ((f >> 25) & 2) as u16 };
            let s = self.units[u as usize].sprite;
            let main = self.sprites[s as usize].main_image;
            self.create_image(d, s, main, 0x170 + size, 0, 0, 0);
            // 0x4749d0: order 0x5b first in the queue.
            self.order_queue_front(d, u, 0x5b, (0, 0));
            self.units[u as usize].order_state = 3;
            st = 3;
        }
        if st == 3 {
            if self.units[b as usize].order_target_unit == NIL {
                self.units[b as usize].order_target_unit = u;
            }
            self.recharge_step(d, u, b);
            let max = (d.dat.units.shields[self.units[u as usize].unit_type as usize] as i32) << 8;
            if self.units[u as usize].shields >= max || self.units[b as usize].energy == 0 {
                if self.units[b as usize].order_target_unit == u {
                    self.units[b as usize].order_target_unit = NIL;
                }
                self.order_done_or_next(d, u);
            }
        }
    }

    /// 0x493990: RechargeShieldsBattery: the battery calls its target (or every unit
    /// within 0x80 of the clicked spot, 0x493900) over to recharge.
    pub(super) fn order_recharge_shields_battery(&mut self, d: &Data, b: u32) {
        self.set_secondary(b, 0x44);
        let t = self.units[b as usize].order_target_unit;
        if t != NIL && self.can_recharge(d, t, b) {
            self.units[t as usize].order_flags |= 1;
            let p = self.unit_sprite_pos(b);
            self.issue_order(d, t, 0x31, true, p, b, 0xe4);
            self.prepare_next_order(d, t);
        } else {
            let (x, y) = self.units[b as usize].order_target_pos;
            let bx = [x.wrapping_sub(0x80), y.wrapping_sub(0x80), x.wrapping_add(0x80), y.wrapping_add(0x80)];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, bx);
            let found = search.results(start);
            search.pop();
            self.search = search;
            for c in found {
                if self.can_recharge(d, c, b) {
                    self.units[c as usize].order_flags |= 1;
                    let p = self.unit_sprite_pos(b);
                    self.issue_order(d, c, 0x31, true, p, b, 0xe4);
                    self.prepare_next_order(d, c);
                }
            }
        }
        self.battery_give_up(d, b);
    }

    /// 0x4932d0: ShieldBattery (secondary): the battery's working animation (0x13 on,
    /// 0x14 off) while a unit recharges from it.
    pub(super) fn secondary_shield_battery(&mut self, d: &Data, b: u32) {
        let t = self.units[b as usize].order_target_unit;
        match self.units[b as usize].secondary_order_state {
            0 => {
                if t == NIL {
                    return;
                }
                let s = self.units[b as usize].sprite;
                self.sprite_images_run_anim(d, s, 0x13);
            }
            1 => {
                if t != NIL {
                    if self.units[t as usize].order == 0x42 {
                        return;
                    }
                    self.units[b as usize].order_target_unit = NIL;
                }
                self.units[b as usize].secondary_order_state = 2;
                return;
            }
            2 => {
                if t == NIL {
                    let s = self.units[b as usize].sprite;
                    self.sprite_images_run_anim(d, s, 0x14);
                    self.units[b as usize].secondary_order_state = 0;
                    return;
                }
            }
            _ => return,
        }
        self.units[b as usize].secondary_order_state = 1;
    }

    /// Follow (0x47c7b0) onto a shield battery: recharge there (0x42) when it can.
    pub(super) fn follow_onto_battery(&mut self, d: &Data, u: u32, b: u32) -> bool {
        if self.units[b as usize].unit_type != BATTERY || !self.can_recharge(d, u, b) {
            return false;
        }
        self.units[u as usize].order_flags |= 1;
        let p = self.unit_sprite_pos(b);
        self.issue_order(d, u, 0x42, true, p, b, 0xe4);
        self.prepare_next_order(d, u);
        true
    }

    /// 0x493a80: InitializeArbiter: queue the idle order, start the CloakNearbyUnits
    /// secondary order.
    pub(super) fn order_initialize_arbiter(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head == NIL {
            self.units[u as usize].order_flags |= 1;
            // 0x6648df: the arbiter's (0x47) idle order, whatever the type.
            let o = d.dat.units.ai_return_to_idle[0x47];
            self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
            self.prepare_next_order(d, u);
        }
        if self.units[u as usize].status & 0x4000_0000 == 0 {
            self.set_secondary(u, 0x83);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x491c20: CloakNearbyUnits: own units within the arbiter's (air) weapon range ask
    /// to stay cloaked (not buildings, arbiters, nukes, hallucinations).
    pub(super) fn secondary_cloak_nearby(&mut self, d: &Data, u: u32) {
        let w = d.dat.units.air_weapon[self.units[u as usize].unit_type as usize];
        let r = self.weapon_range(d, u, w) as i32;
        let (x, y) = self.unit_sprite_pos(u);
        let (x, y) = (x as i32, y as i32);
        let mut b = [(x - r) as i16, (y - r) as i16, (x + r) as i16, (y + r) as i16];
        // Clamped to the map; a box over the left / top edge keeps its far side.
        if b[0] < 0 {
            b[0] = 0;
        } else if b[2] as i32 > self.map_px.0 as i32 {
            b[2] = self.map_px.0 as i16;
        }
        if b[1] < 0 {
            b[1] = 0;
        } else if b[3] as i32 > self.map_px.1 as i32 {
            b[3] = self.map_px.1 as i16;
        }
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let player = self.units[u as usize].player;
        for c in found {
            let cu = &self.units[c as usize];
            let t = cu.unit_type;
            let f = d.dat.units.flags[t as usize];
            if matches!(t, 0x47 | 0x56 | 0xe)
                || f & 1 != 0
                || cu.status & 0x4000_0000 != 0
                || f & 0x800 != 0
                || cu.order == 0xae
                || cu.player != player
                || !self.box_distance_within(d, u, r as u32, c)
            {
                continue;
            }
            self.secondary_cloak(c);
            self.units[c as usize].status |= 0x800;
            // 0x68c1b0 etc.: UI refresh.
        }
    }
}
