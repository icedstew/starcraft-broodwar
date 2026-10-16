//! High templars merging into archons: the Archon Merge command (0x4c0e90), ArchonWarp
//! (0x69, 0x4948b0) and CompletingArchonSummon (0x6a, 0x493b10); dark templars into
//! dark archons: Dark Archon Meld (0x4c0cd0) and DarkArchonMeld (0xb7, 0x494690).

use super::list::NIL;
use super::{Data, State};

impl State {
    /// 0x4c0e90: Archon Merge: each selected high templar pairs with the nearest one
    /// selected after it (0x4bfa80) and both walk to each other.
    pub fn command_archon_merge(&mut self, d: &Data, player: u8, sel: &[u32]) {
        self.command_meld(d, player, sel, 0x17, 0x43, 0x69);
    }

    /// 0x4c0cd0: Dark Archon Meld, the same for dark templars (tech 0x1c, order 0xb7).
    pub fn command_dark_archon_meld(&mut self, d: &Data, player: u8, sel: &[u32]) {
        self.command_meld(d, player, sel, 0x1c, 0x3d, 0xb7);
    }

    fn command_meld(&mut self, d: &Data, player: u8, sel: &[u32], tech: u16, templar: u16, order: u8) {
        let units = self.live_selection(sel);
        let Some(&first) = units.first() else { return };
        if self.can_use_tech(d, first, tech, player) != Ok(1) {
            return;
        }
        let mut ht: Vec<u32> = units.into_iter().filter(|&u| self.units[u as usize].unit_type == templar).collect();
        let n = ht.len();
        for i in 0..n.saturating_sub(1) {
            let a = ht[i];
            if a == NIL {
                continue;
            }
            let Some(b) = self.nearest_templar(a, &mut ht[i + 1..]) else { continue };
            self.queue_meld(d, a, b, order);
            self.queue_meld(d, b, a, order);
        }
        // 0x68c1b0 etc.: button refresh (UI).
    }

    /// 0x4bfa80: the nearest of `rest` to `a` by sprite position; the chosen slot gets
    /// the previous best back (so the winner is taken out of the list).
    fn nearest_templar(&self, a: u32, rest: &mut [u32]) -> Option<u32> {
        let pa = self.unit_sprite_pos(a);
        let mut best = NIL;
        let mut best_d = 0x7fff_ffffu32;
        for slot in rest.iter_mut() {
            let c = *slot;
            if c == NIL {
                continue;
            }
            let pc = self.unit_sprite_pos(c);
            let (dx, dy) = (pc.0 as i32 - pa.0 as i32, pc.1 as i32 - pa.1 as i32);
            let dist = (dy * dy + dx * dx) as u32;
            if dist < best_d {
                *slot = best;
                best_d = dist;
                best = c;
            }
        }
        (best != NIL).then_some(best)
    }

    /// 0x4c0e90 / 0x4c0cd0 for one templar: replace interruptible / meld orders with
    /// the meld `order` toward `partner`.
    fn queue_meld(&mut self, d: &Data, u: u32, partner: u32, order: u8) {
        self.units[u as usize].order_flags |= 1;
        let p = self.unit_sprite_pos(partner);
        if self.units[u as usize].order != 0 {
            loop {
                let h = self.units[u as usize].order_queue_head;
                if h == NIL {
                    break;
                }
                let o = self.orders[h as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != order {
                    break;
                }
                self.order_remove(d, u, h);
            }
            self.order_queue_append(d, u, order, 0xe4, p, partner);
        }
        self.prepare_next_order(d, u);
    }

    /// 0x4eb960: move toward (x, y) unless the unit was ordered moments ago.
    fn move_unless_recent(&mut self, d: &Data, u: u32, p: (i16, i16)) {
        if self.units[u as usize].recent_order_timer != 0 {
            return;
        }
        self.set_move_target_xy(d, u, p);
    }

    /// 0x4948b0: ArchonWarp: walk to the partner (to the midpoint when close) and merge
    /// within 2 pixels.
    pub(super) fn order_archon_warp(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        let valid = t != NIL && {
            let tu = &self.units[t as usize];
            tu.player == self.units[u as usize].player && tu.order == 0x69 && tu.order_target_unit == u
        };
        if !valid {
            self.unit_halt(d, u);
            let f = &mut self.units[u as usize].fl;
            if f.move_target != f.next_target_waypoint {
                f.next_target_waypoint = f.move_target;
            }
            if self.units[u as usize].order_queue_head == NIL {
                let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
                self.order_now(d, u, o);
            } else {
                self.units[u as usize].order_flags |= 1;
                self.prepare_next_order(d, u);
            }
            return;
        }
        if self.units[u as usize].status & 0x10_0000 != 0 {
            let range = ((self.units[u as usize].fl.next_speed * 2) >> 8) as u32;
            if self.box_distance_within(d, u, range, t) {
                self.units[u as usize].status &= !0x10_0000;
                self.queue_front(d, u, 0x96, (0, 0), NIL);
            }
        }
        let (pu, pt) = (self.unit_sprite_pos(u), self.unit_sprite_pos(t));
        let dist = super::path::approx_distance(pu.0 as i32, pt.0 as i32, pu.1 as i32, pt.1 as i32);
        if dist <= 2 {
            return self.merge_archon(d, u, t);
        }
        if self.units[u as usize].order_state != 0 && self.unit_at_move_target(u) == 2 {
            self.unit_halt_facing(d, u);
            return self.order_done_or_next(d, u);
        }
        self.units[u as usize].order_state = 1;
        if dist > 10 {
            self.move_unless_recent(d, u, pt);
        } else {
            let mid = (((pu.0 as i32 + pt.0 as i32) / 2) as i16, ((pu.1 as i32 + pt.1 as i32) / 2) as i16);
            self.move_unless_recent(d, u, mid);
        }
    }

    /// 0x4948b0 at the merge: `u` becomes the archon with the worse of both templars'
    /// spell states (0x493180), `t` disappears.
    fn merge_archon(&mut self, d: &Data, u: u32, t: u32) {
        self.combine_spell_states(u, t);
        self.transform_unit(d, u, 0x44);
        let s = self.units[u as usize].sprite;
        self.sprites[s as usize].flags &= 0x7f;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.image_run_anim(d, i, 0xd);
            i = next;
        }
        self.set_button_set(d, u, 0xe4);
        // 0x4c36c0: button refresh (UI).
        let k = self.units[t as usize].kill_count;
        self.units[u as usize].kill_count = self.units[u as usize].kill_count.wrapping_add(k);
        self.units[t as usize].order_flags |= 4;
        self.kill_unit(d, t);
        self.units[u as usize].status &= !0x2_0000;
        self.reset_path_state(u);
        self.order_now(d, u, 0x6a);
    }

    /// 0x493180: `u` keeps the longer timer of each spell state of `t` (and some of its
    /// other status fields).
    fn combine_spell_states(&mut self, u: u32, t: u32) {
        let src = self.units[t as usize].clone();
        let un = &mut self.units[u as usize];
        un.defense_matrix_damage = un.defense_matrix_damage.max(src.defense_matrix_damage);
        un.defense_matrix_timer = un.defense_matrix_timer.max(src.defense_matrix_timer);
        un.stim_timer = un.stim_timer.max(src.stim_timer);
        un.ensnare_timer = un.ensnare_timer.max(src.ensnare_timer);
        un.lockdown_timer = un.lockdown_timer.max(src.lockdown_timer);
        un.maelstrom_timer = un.maelstrom_timer.max(src.maelstrom_timer);
        if un.irradiate_timer < src.irradiate_timer {
            un.irradiate_timer = src.irradiate_timer;
            un.irradiated_by = src.irradiated_by;
            un.irradiate_player = src.irradiate_player;
        }
        un.stasis_timer = un.stasis_timer.max(src.stasis_timer);
        un.plague_timer = un.plague_timer.max(src.plague_timer);
        un.parasite_flags |= src.parasite_flags;
        un.is_blind |= src.is_blind;
        un.acid_spore_count = src.acid_spore_count;
        un.acid_spore_time = src.acid_spore_time;
    }

    /// 0x494690: DarkArchonMeld: walk to the partner and merge within 20 pixels.
    pub(super) fn order_dark_archon_meld(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        let valid = t != NIL && {
            let tu = &self.units[t as usize];
            tu.player == self.units[u as usize].player && tu.order == 0xb7 && tu.order_target_unit == u
        };
        if !valid {
            self.unit_halt(d, u);
            let f = &mut self.units[u as usize].fl;
            if f.move_target != f.next_target_waypoint {
                f.next_target_waypoint = f.move_target;
            }
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.prepare_next_order(d, u);
            } else {
                let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
                self.order_now(d, u, o);
            }
            return;
        }
        if self.units[u as usize].status & 0x10_0000 != 0 {
            let range = ((self.units[u as usize].fl.next_speed * 2) >> 8) as u32;
            if self.box_distance_within(d, u, range, t) {
                self.units[u as usize].status &= !0x10_0000;
                self.queue_front(d, u, 0x96, (0, 0), NIL);
            }
        }
        let (pu, pt) = (self.unit_sprite_pos(u), self.unit_sprite_pos(t));
        let dist = super::path::approx_distance(pu.0 as i32, pt.0 as i32, pu.1 as i32, pt.1 as i32);
        if dist > 0x14 {
            if self.units[u as usize].order_state != 0 && self.unit_at_move_target(u) == 2 {
                self.unit_halt_facing(d, u);
                return self.order_done_or_next(d, u);
            }
            self.units[u as usize].order_state = 1;
            return self.move_unless_recent(d, u, pt);
        }
        self.combine_spell_states(u, t);
        self.transform_unit(d, u, 0x3f);
        self.end_cloak(d, u);
        let s = self.units[u as usize].sprite;
        self.sprites[s as usize].flags &= 0x7f;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.image_run_anim(d, i, 0xd);
            i = next;
        }
        self.set_button_set(d, u, 0xe4);
        let k = self.units[t as usize].kill_count;
        self.units[u as usize].kill_count = self.units[u as usize].kill_count.wrapping_add(k);
        self.units[u as usize].energy = 0x3200;
        self.units[t as usize].order_flags |= 4;
        self.kill_unit(d, t);
        self.units[u as usize].status &= !0x2_0000;
        self.reset_path_state(u);
        self.order_now(d, u, 0x6a);
    }

    /// 0x46a820: drop the path, stop, and recompute the low-elevation pathing bit.
    fn reset_path_state(&mut self, u: u32) {
        self.path_free(u);
        let s = self.units[u as usize].sprite;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | low;
    }

    /// 0x493b10: CompletingArchonSummon: wait out the build time, play the summon, then
    /// become a normal unit.
    pub(super) fn order_completing_archon_summon(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                // 0x466940: count down the build time (by 16 on the fast-build cheat).
                let un = &mut self.units[u as usize];
                let left = un.remaining_build_time;
                if left != 0 {
                    if self.game_options & 2 == 0 {
                        un.remaining_build_time = left - 1;
                    } else {
                        let v = (left as i16).wrapping_sub(0x10);
                        un.remaining_build_time = if v < 0 { 0 } else { v as u16 };
                    }
                    return;
                }
                let s = un.sprite;
                self.sprites[s as usize].flags &= 0x7f;
                let mut i = self.sprites[s as usize].images.head;
                while i != NIL {
                    let next = self.images[i as usize].link.next;
                    self.image_run_anim(d, i, 0);
                    i = next;
                }
                self.units[u as usize].order_state = 1;
            }
            1 if self.units[u as usize].order_signal & 4 != 0 => {
                self.units[u as usize].order_signal &= !4;
                self.units[u as usize].status |= 0x2_0000;
                self.reset_path_state(u);
                let ut = self.units[u as usize].unit_type;
                self.set_button_set(d, u, ut);
                // 0x68c1b0 etc.: button refresh (UI).
                self.unit_ready_sound(d, u);
                let idle = d.dat.units.ai_human_idle[ut as usize];
                self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
                self.order_next_or_idle(d, u);
            }
            _ => {}
        }
    }
}
