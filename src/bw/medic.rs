//! Medic orders: Medic (idle, 0x463900), MedicHeal (0xb0, 0x464180), HealMove (0xb1,
//! 0x4637b0), MedicHoldPosition (0xb2, 0x464050), MedicHealToIdle (0xb3, 0x463740), the
//! heal step (0x463c40) and the heal target search (0x4422a0).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Healing.
const TECH_HEALING: u16 = 0x22;

impl State {
    /// 0x479fe0: set the order target unit and position (no order change).
    fn set_order_target(&mut self, u: u32, t: u32) {
        let p = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        let un = &mut self.units[u as usize];
        un.order_target_unit = t;
        un.order_target_pos = p;
    }

    /// 0x463530: medic `u` can heal `t` (organic, not a building, grounded, allied,
    /// hurt, not disabled).
    fn medic_can_heal(&self, d: &Data, u: u32, t: u32) -> bool {
        if t == NIL || t == u || self.units[u as usize].status & 0x4000_0000 != 0 {
            return false;
        }
        let tu = &self.units[t as usize];
        let f = d.dat.units.flags[tu.unit_type as usize];
        f & 0x1_0000 != 0
            && f & 1 == 0
            && tu.status & 4 == 0
            && !self.is_enemy(u, t)
            && tu.player < 8
            && self.units[u as usize].player < 8
            && tu.hp < d.dat.units.hit_points[tu.unit_type as usize] as i32
            && !self.disabled(t)
    }

    /// 0x4422a0: the unit medic `u` should heal next. Human medics take the nearest
    /// within 0xa0; AI medics take the nearest of their player's units no other medic
    /// is already healing.
    pub(super) fn find_heal_target(&mut self, d: &Data, u: u32) -> u32 {
        let player = self.units[u as usize].player;
        if self.can_use_tech(d, u, TECH_HEALING, player).is_err() {
            return NIL;
        }
        if self.units[u as usize].ai == NIL {
            // 0x4e87e0 with radius 0xa0 and filter 0x441240.
            let (x, y) = self.unit_sprite_pos(u);
            let b = [x - 0xa0, y - 0xa0, x + 0xa0, y + 0xa0];
            return self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
                let cu = &st.units[c as usize];
                cu.is_being_healed == 0 && cu.status & 0x4000_0000 == 0 && st.medic_can_heal(d, u, c)
            });
        }
        let mut cands: Vec<u32> = Vec::new();
        let mut medic = NIL;
        let mut c = self.player_units[player as usize];
        while c != NIL {
            let cu = &self.units[c as usize];
            let next = cu.player_next;
            if cu.sprite != NIL
                && cu.order != 0
                && self.sprites[cu.sprite as usize].flags & 0x20 == 0
                && cu.status & 1 != 0
                && cu.is_being_healed == 0
                && self.medic_can_heal(d, u, c)
                && cu.status & 0x4000_0000 == 0
            {
                if medic == NIL && cu.unit_type == 0x22 {
                    medic = c;
                }
                if d.dat.units.flags[cu.unit_type as usize] & 0x1_0000 != 0 {
                    cands.push(c);
                }
            }
            c = next;
        }
        // Drop the targets other medics (from the first one found on) are healing.
        while medic != NIL {
            let m = &self.units[medic as usize];
            let head = m.order_queue_head;
            let healing = m.order == 0xb0 || (head != NIL && self.orders[head as usize].order == 0xb0);
            if healing && !cands.is_empty() {
                let t = if m.order == 0xb0 { m.order_target_unit } else { self.orders[head as usize].target };
                if let Some(slot) = cands.iter_mut().find(|c| **c == t) {
                    *slot = NIL;
                }
            }
            medic = m.player_next;
        }
        let (mx, my) = self.unit_sprite_pos(u);
        let mut best = 99_999_999u32;
        let mut result = NIL;
        for &c in &cands {
            if c == NIL {
                continue;
            }
            let (cx, cy) = self.unit_sprite_pos(c);
            let dist = super::path::approx_distance(mx as i32, cx as i32, my as i32, cy as i32);
            if dist < best {
                best = dist;
                result = c;
            }
        }
        result
    }

    /// 0x491480: `u` has less than `cost` energy (never with the energy cheat).
    pub(super) fn energy_short(&self, u: u32, cost: u16) -> bool {
        self.game_options & 0x800 == 0 && self.units[u as usize].energy < cost
    }

    /// 0x491460: `u` pays `cost` energy (not with the energy cheat).
    pub(super) fn spend_energy(&mut self, u: u32, cost: u16) {
        if self.game_options & 0x800 == 0 {
            self.units[u as usize].energy -= cost;
        }
    }

    /// 0x463c40: heal `t` once if in range. 0 = can't heal, 1 = healed, 2 = out of
    /// energy, 3 = out of range. A target someone else is healing is swapped for a new one.
    fn heal_step(&mut self, d: &Data, u: u32, t: u32) -> u8 {
        let player = self.units[u as usize].player;
        if self.can_use_tech(d, u, TECH_HEALING, player).is_err() {
            return 0;
        }
        if t == NIL || !self.medic_can_heal(d, u, t) {
            return 0;
        }
        let mut t = t;
        if self.units[t as usize].is_being_healed != 0 {
            t = self.find_heal_target(d, u);
            if t == NIL {
                return 0;
            }
            self.set_order_target(u, t);
            self.move_to_unit(d, u, t);
        }
        if !self.box_distance_within(d, u, 0x1e, t) {
            return 3;
        }
        let tu = &self.units[t as usize];
        let amount = (d.dat.units.hit_points[tu.unit_type as usize] as i32).wrapping_sub(tu.hp) as u32;
        let amount = amount.min(200);
        let cost = (amount >> 1) as u16;
        if self.energy_short(u, cost) {
            return 2;
        }
        self.spend_energy(u, cost);
        let hp = self.units[t as usize].hp + amount as i32;
        self.set_unit_hp(d, t, hp);
        self.units[t as usize].is_being_healed = 1;
        1
    }

    /// Shared tail of MedicHeal / MedicHoldPosition after a heal: play the heal
    /// animation and queue MedicHealToIdle once, then face the target. True when the
    /// target waypoint moved.
    fn healing(&mut self, d: &Data, u: u32) -> bool {
        if self.units[u as usize].order_signal & 1 == 0 {
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xd);
            let head = self.units[u as usize].order_queue_head;
            if head == NIL || self.orders[head as usize].order != 0xb3 {
                self.order_queue_front(d, u, 0xb3, (0, 0));
            }
        }
        let t = self.units[u as usize].order_target_unit;
        let p = self.unit_sprite_pos(t);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
            return true;
        }
        false
    }

    /// 0x464180: MedicHeal.
    pub(super) fn order_medic_heal(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        let r = self.heal_step(d, u, t);
        if r == 0 {
            self.unit_halt(d, u);
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.prepare_next_order(d, u);
            } else if self.units[u as usize].ai != NIL {
                self.order_now(d, u, 0x9c);
            } else {
                let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                self.order_now(d, u, o);
            }
            return;
        }
        if self.units[u as usize].order_state == 0 {
            let t = self.units[u as usize].order_target_unit;
            if t == NIL || !self.follow_move(d, u, t) || self.unit_at_move_target(u) == 2 {
                self.order_done_or_next(d, u);
                return;
            }
            let t = self.units[u as usize].order_target_unit;
            if self.box_distance_within(d, u, 0x1e, t) {
                self.unit_halt(d, u);
                self.units[u as usize].order_state = 1;
            }
        } else if r == 1 {
            self.healing(d, u);
        } else {
            self.units[u as usize].order_state = 0;
        }
    }

    /// 0x4637b0: HealMove (attack-move for medics): move, healing whatever turns up.
    pub(super) fn order_heal_move(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            if self.medic_can_heal(d, u, t) {
                self.order_now(d, u, 0xb0);
                self.set_order_target(u, t);
                self.move_to_unit(d, u, t);
                return;
            }
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        }
        if self.units[u as usize].recent_order_timer == 0 {
            let p = self.units[u as usize].order_target_pos;
            if !self.set_move_target_xy(d, u, p) {
                return;
            }
        }
        if self.units[u as usize].fl.move_target == self.unit_sprite_pos(u) {
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let h = self.find_heal_target(d, u);
        if h == NIL {
            return;
        }
        let (pos, tgt) = (self.units[u as usize].order_target_pos, self.units[u as usize].order_target_unit);
        self.order_target(d, u, 0xb0, pos, tgt);
        self.move_to_unit(d, u, h);
        self.set_order_target(u, h);
        // 0x474940 / 0x4748e0: resume the heal-move afterwards.
        if t != NIL {
            let tp = self.unit_sprite_pos(t);
            self.order_queue_append(d, u, 0xb1, 0xe4, tp, t);
        } else {
            self.order_queue_append(d, u, 0xb1, 0xe4, pos, NIL);
        }
    }

    /// 0x464050: MedicHoldPosition.
    pub(super) fn order_medic_hold_position(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            self.unit_halt(d, u);
            let f = &mut self.units[u as usize].fl;
            if f.move_target != f.next_target_waypoint {
                f.next_target_waypoint = f.move_target;
            }
            self.units[u as usize].status |= 0x800_0000;
            let sub = self.units[u as usize].subunit;
            if sub != NIL {
                self.units[sub as usize].status |= 0x800_0000;
            }
            self.units[u as usize].order_state = 1;
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xc);
        }
        let t = self.units[u as usize].order_target_unit;
        match self.heal_step(d, u, t) {
            1 => {
                self.healing(d, u);
                return;
            }
            r => {
                if r == 3 {
                    self.units[u as usize].order_target_unit = NIL;
                }
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xc);
            }
        }
        if self.units[u as usize].main_order_timer == 0 {
            self.units[u as usize].main_order_timer = 0xf;
            let h = self.find_heal_target(d, u);
            self.units[u as usize].order_target_unit = h;
            if h != NIL {
                self.units[u as usize].order_target_pos = self.unit_sprite_pos(h);
            }
        }
    }

    /// 0x463740: MedicHealToIdle.
    pub(super) fn order_medic_heal_to_idle(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].fl.flags & 2 == 0 {
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xc);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x463900: Medic (idle): heal the nearest hurt unit.
    pub(super) fn order_medic(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ai != NIL {
            todo_port!("0x4a13c0", "AI spellcasting (needs AI structs)");
        }
        let h = self.find_heal_target(d, u);
        if h != NIL {
            self.units[u as usize].order_flags |= 1;
            if self.units[u as usize].order != 0 {
                loop {
                    let tail = self.units[u as usize].order_queue_tail;
                    if tail == NIL {
                        break;
                    }
                    let o = self.orders[tail as usize].order;
                    if d.dat.orders.interruptible[o as usize] == 0 && o != 0xb0 {
                        break;
                    }
                    self.order_remove(d, u, tail);
                }
                self.order_queue_append(d, u, 0xb0, 0xe4, (0, 0), NIL);
            }
            self.prepare_next_order(d, u);
            self.set_order_target(u, h);
            self.move_to_unit(d, u, h);
            return;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x463900", "AI medic: back to its guard position (0xa3 via 0x475260)");
        }
    }
}
