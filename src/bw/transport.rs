//! Units entering transports and bunkers (1.16.1 EnterTransport, PickupBunker and the
//! load itself). A transport keeps its passengers' handles in `loaded_units` (+0xb0).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x402140: distance (0x40c360) from the unit's exact position (+0x2c/+0x30) to (x, y).
    pub(super) fn exact_distance_to(&self, u: u32, x: i32, y: i32) -> u32 {
        let (ex, ey) = self.units[u as usize].fl.exact;
        super::path::approx_distance(ex, x, ey, y)
    }

    /// 0x4e6ef0 (EBX = `u`): the closest of `u`'s player's completed, grounded transports that can
    /// take `u` (or one that already has `u` as its order target). Returns whether one
    /// was found (a hurt transport counts even when none can load `u`) and the unit.
    pub(super) fn nearest_transport_for(&self, d: &Data, u: u32) -> (bool, u32) {
        let mut found = NIL;
        let mut result = false;
        let mut best = 99999999u32;
        let mut c = self.player_units[self.units[u as usize].player as usize];
        while c != NIL {
            let cu = &self.units[c as usize];
            let next = cu.player_next;
            let t = cu.unit_type as usize;
            if cu.status & 0x4000_0000 == 0
                && (cu.unit_type != 0x2a || self.players.upgrade_sc[cu.player as usize][0x18] != 0)
                && d.dat.units.space_provided[t] != 0
                && cu.status & 1 != 0
                && cu.status & 0x2_0000 != 0
            {
                if cu.order_target_unit == u {
                    return (true, c);
                }
                let hp = d.dat.units.hit_points[t] as i32;
                let mut max = hp >> 8;
                if max == 0 {
                    max = (cu.hp + 0xff) >> 8;
                    if max == 0 {
                        max = 1;
                    }
                }
                let m3 = max * 3;
                if ((m3 + ((m3 >> 31) & 3)) >> 2) <= (cu.hp + 0xff) >> 8
                    || (d.dat.units.shield_enable[t] != 0 && cu.shields >> 8 == d.dat.units.shields[t] as i32)
                {
                    result = true;
                    if self.can_load(d, c, u) && !matches!(cu.order, 0x5e | 0x6f) {
                        let ok = cu.order != 0x70 || {
                            let (a, b) = (cu.order_target_pos, self.units[u as usize].order_target_pos);
                            super::path::region_at(&self.sai, a.0 as i32, a.1 as i32) == super::path::region_at(&self.sai, b.0 as i32, b.1 as i32)
                        };
                        if ok {
                            let sp = self.unit_sprite_pos(c);
                            let dist = self.exact_distance_to(u, sp.0 as i32, sp.1 as i32);
                            if dist < best {
                                found = c;
                                best = dist;
                            }
                        }
                    }
                }
            }
            c = next;
        }
        (result, found)
    }

    /// 0x4e6c40: the passenger in slot `i`, if that handle is still a live unit.
    pub fn loaded_unit(&self, tr: u32, i: usize) -> Option<u32> {
        self.unit_from_handle(self.units[tr as usize].loaded_units[i] as u32)
    }

    /// 0x4e6d00 (EAX = transport): call `f` on each live passenger, in slot order over the
    /// transport type's passenger capacity; true as soon as one call returns true.
    pub(super) fn any_passenger(&mut self, d: &Data, tr: u32, mut f: impl FnMut(&mut Self, u32) -> bool) -> bool {
        let n = d.dat.units.space_provided[self.units[tr as usize].unit_type as usize] as usize;
        for i in 0..n.min(8) {
            if let Some(c) = self.unit_from_handle(self.units[tr as usize].loaded_units[i] as u32) {
                if f(self, c) {
                    return true;
                }
            }
        }
        false
    }

    /// 0x4e6d90 (EAX = transport): one of its 8 slots holds a live unit whose type has
    /// flag 8 in units.dat.
    pub(super) fn has_passenger_with_flag8(&self, d: &Data, tr: u32) -> bool {
        (0..8).any(|i| {
            self.loaded_unit(tr, i).is_some_and(|c| d.dat.units.flags[self.units[c as usize].unit_type as usize] & 8 != 0)
        })
    }

    /// 0x4e7080: a passenger of `tr` carries (flag 8, +0xc0) a flag (0xd7).
    pub(super) fn passenger_carries_flag(&self, d: &Data, tr: u32) -> bool {
        (0..8).filter_map(|i| self.loaded_unit(tr, i)).any(|c| {
            let cu = &self.units[c as usize];
            let p = cu.ptr_at(0xc0);
            d.dat.units.flags[cu.unit_type as usize] & 8 != 0 && p != NIL && self.units[p as usize].unit_type == 0xd7
        })
    }

    /// 0x4e6e00: transport `tr` can take `u` now (complete, not disabled, same owner,
    /// enough room; bunkers take Terran infantry only).
    pub fn can_load(&self, d: &Data, tr: u32, u: u32) -> bool {
        let t = &self.units[tr as usize];
        if t.status & 1 == 0 || t.status & 0x400 != 0 || t.lockdown_timer != 0 || t.stasis_timer != 0 || t.maelstrom_timer != 0 {
            return false;
        }
        let un = &self.units[u as usize];
        if un.status & 0x10 != 0 || un.player != t.player {
            return false;
        }
        let tt = t.unit_type as usize;
        let space = |c: u32| d.dat.units.space_required[self.units[c as usize].unit_type as usize] as u32;
        if d.dat.units.flags[tt] & 1 != 0 {
            // 0x47b150: Terran, one slot.
            let g = d.dat.units.staredit_group_flags[un.unit_type as usize];
            let terran = g & 1 == 0 && g & 4 == 0 && g & 2 != 0;
            if !terran || space(u) > 1 {
                return false;
            }
        }
        let cap = d.dat.units.space_provided[tt] as u32;
        let used: u32 = (0..cap as usize).filter_map(|i| self.loaded_unit(tr, i)).map(space).sum();
        space(u) <= cap.wrapping_sub(used)
    }

    /// 0x4e7cf0: EnterTransport: walk to the order target and get in once touching it;
    /// a mobile transport is told to come and pick the unit up.
    pub fn order_enter_transport(&mut self, d: &Data, u: u32) {
        let tr = self.units[u as usize].order_target_unit;
        let ok = tr != NIL && {
            let t = &self.units[tr as usize];
            t.status & 0x4000_0000 == 0
                && (t.unit_type != 0x2a || self.players.upgrade_sc[t.player as usize][0x18] != 0)
                && d.dat.units.space_provided[t.unit_type as usize] != 0
                && self.can_load(d, tr, u)
        };
        if !ok {
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.order_nothing(d, u);
                return;
            }
            let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
            self.start_order_now(d, u, o);
            return;
        }
        if self.units[u as usize].order_state == 0 && self.units[tr as usize].order != 0x5e {
            if self.units[tr as usize].status & 0x2_0000 != 0 {
                self.queue_front_target(d, tr, 0x5e, u);
                self.order_nothing(d, tr);
            }
            self.units[u as usize].order_state = 1;
        }
        let p = self.unit_sprite_pos(tr);
        if self.units[u as usize].fl.next_target_waypoint != p {
            self.units[u as usize].fl.next_target_waypoint = p;
        }
        self.follow_move(d, u, tr);
        if self.box_distance_within(d, u, 1, tr) {
            self.load_unit(d, tr, u);
        }
    }

    /// 0x4e73b0: PickupBunker: a bunker's order to take its target in.
    pub fn order_pickup_bunker(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            if self.can_load(d, u, t) {
                self.order_with_target(d, u, 0x5c, t);
            }
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.order_nothing(d, u);
            return;
        }
        let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
        self.start_order_now(d, u, o);
    }

    /// 0x4e7b70: Pickup4 (also the hidden order 0x60): `u` finds a transport (or uses its
    /// order target), has it come to `u` (0x5e) and, once touching it, gets in and stops.
    pub(super) fn pickup4(&mut self, d: &Data, u: u32) {
        // +0x4c: a neutral (player 11) unit stops.
        if self.units[u as usize].player == 0xb {
            let o = if self.units[u as usize].status & 0x40 != 0 { 0x17 } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
            self.units[u as usize].order_flags |= 1;
            self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
            self.order_nothing(d, u);
            return;
        }
        if self.units[u as usize].status & 0x40 != 0 {
            return;
        }
        if self.ai_choose_target(d, u, false, true) {
            return;
        }
        let mut tr = self.units[u as usize].order_target_unit;
        if tr == NIL {
            let (found, c) = self.nearest_transport_for(d, u);
            self.units[u as usize].order_target_unit = c;
            if !found {
                self.order_done_or_next(d, u);
                return;
            }
            tr = c;
            if tr == NIL {
                return;
            }
        } else if !self.can_carry(d, tr) || self.units[tr as usize].order_target_unit != u {
            self.units[u as usize].order_target_unit = NIL;
            return;
        }
        if !self.can_load(d, tr, u) {
            self.units[u as usize].order_target_unit = NIL;
            return;
        }
        if self.box_distance_within(d, u, 1, tr) {
            self.load_unit(d, tr, u);
            let p = self.units[u as usize].order_target_pos;
            self.order_at(d, u, 0x70, p);
            if self.units[tr as usize].ai != NIL {
                let tp = self.unit_sprite_pos(tr);
                self.queue_order_at(d, tr, 6, tp, false);
            }
        } else {
            self.queue_front_target(d, u, 0x5e, tr);
            self.order_nothing(d, tr);
            let tp = self.unit_sprite_pos(tr);
            if self.region_reachable(d, u, tp) {
                self.set_target_waypoint(u, tp);
                self.follow_move(d, u, tr);
            }
        }
    }

    /// 0x4e78e0: `u` gets into transport `tr`: it takes the first free slot, leaves the
    /// map, and in a bunker sits at the bunker's position (status 0x20).
    pub fn load_unit(&mut self, d: &Data, tr: u32, u: u32) {
        let cap = d.dat.units.space_provided[self.units[tr as usize].unit_type as usize] as usize;
        let mut slot = 0;
        while slot < cap && self.loaded_unit(tr, slot).is_some() {
            slot += 1;
        }
        // 0x48ed50 (EBX = 0x28 + the transport's race, ESI = the transport): the load sound.
        let race = Self::supply_race_of(d, self.units[tr as usize].unit_type);
        self.unit_sound(0x28 + race, tr, 0);
        if self.units[u as usize].unit_type == 7 && self.units[u as usize].ai != NIL {
            todo_port!("0x4e78e0", "AI town builder SCV loaded");
        }
        self.units[u as usize].connected_unit = tr;
        let h = if u + 1 < 0x6a5 { self.unit_handle(u) as u16 } else { 0 };
        if slot < 8 {
            self.units[tr as usize].loaded_units[slot] = h;
        }
        self.units[u as usize].status |= 0x40;
        self.hide_unit(d, u);
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0xc);
        if self.units[tr as usize].status & 2 == 0 {
            return;
        }
        let p = self.unit_sprite_pos(tr);
        let mut c = u;
        loop {
            {
                let un = &mut self.units[c as usize];
                un.status = (un.status & !0x2_0000) | 0x20;
            }
            self.path_free(c);
            let elev = self.sprites[self.units[c as usize].sprite as usize].elevation;
            {
                let un = &mut self.units[c as usize];
                un.movement_state = 0;
                un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
            }
            if c == u {
                self.place_unit_at(d, u, p.0, p.1);
            }
            let sub = self.units[u as usize].subunit;
            if c == u && sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
                c = sub;
                continue;
            }
            break;
        }
    }

    /// 0x4e7110: `tr` carries at least one live unit.
    pub fn has_loaded_units(&self, tr: u32) -> bool {
        (0..8).any(|i| self.loaded_unit(tr, i).is_some())
    }

    /// 0x4c1cc0: Unload All command: selected transports with passengers get Unload (0x6f).
    pub fn command_unload_all(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            if self.can_issue_order(d, u, 0x6f, player).is_ok() && self.has_loaded_units(u) {
                self.command_order(d, u, 1, queued, 0x6f, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// 0x4c24f0: Unload command: drop the passenger with handle `h` if it is the
    /// player's and loaded.
    pub fn command_unload(&mut self, d: &Data, player: u8, h: u16) {
        if let Some(u) = self.unit_from_handle(h as u32) {
            let un = &self.units[u as usize];
            if un.connected_unit != NIL && un.status & 0x40 != 0 && un.player == player {
                self.unload_unit(d, u);
            }
        }
    }

    /// 0x4e80d0: Unload. A bunker empties at once; a flying transport drops one
    /// passenger per call (its order timer spaces them).
    pub fn order_unload(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 2 == 0 && self.units[u as usize].main_order_timer != 0 {
            return;
        }
        let mut found = false;
        let mut done = false;
        for i in 0..8 {
            if let Some(c) = self.loaded_unit(u, i) {
                found = true;
                done = self.unload_unit(d, c);
                if self.units[u as usize].status & 2 == 0 {
                    break;
                }
            }
        }
        let grounded = self.units[u as usize].status & 2 != 0;
        if found && !done {
            if self.units[u as usize].ai != NIL && !grounded {
                self.ai_transport_pick_region(d, u);
                return;
            }
        } else if done && !grounded && self.has_loaded_units(u) {
            return;
        }
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].order_queue_head == NIL {
            if self.units[u as usize].ai == NIL {
                let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
            } else {
                // 0x474810 inlined.
                self.issue_order(d, u, 0x9c, true, (0, 0), NIL, 0xe4);
            }
        }
        self.order_nothing(d, u);
    }

    /// 0x43df30: an AI transport that could not unload goes to a neighbouring region of
    /// its own: one of its AI's base regions (state 4, 5, 8, 9), else a neutral one
    /// (state 0), else any other; the last match in each class wins. Without a candidate
    /// it stays in its region. An unwalkable target region triggers 0x43db50 instead.
    fn ai_transport_pick_region(&mut self, d: &Data, u: u32) {
        let p = self.units[u as usize].player;
        let (x, y) = self.unit_sprite_pos(u);
        let here = super::path::region_at(&self.sai, x as i32, y as i32);
        let n = self.sai.regions[here as usize].walkable_neighbors as usize;
        let (mut base, mut neutral, mut other) = (0x9c4u16, 0x9c4u16, 0x9c4u16);
        for i in 0..n {
            let nb = self.sai.neighbor(here as usize, i);
            match self.ai.regions[p as usize][nb as usize].state {
                4 | 5 | 8 | 9 => base = nb,
                0 => neutral = nb,
                _ => other = nb,
            }
        }
        let r = if base != 0x9c4 {
            base
        } else if neutral != 0x9c4 {
            neutral
        } else if other != 0x9c4 {
            other
        } else {
            here
        };
        let reg = &self.sai.regions[r as usize];
        if reg.flags == 0x1ffd {
            self.ai_ghost_to_enemy_region(d, u);
        } else {
            let c = reg.center;
            self.order_at(d, u, 0x70, ((c.0 >> 8) as i16, (c.1 >> 8) as i16));
        }
    }

    /// 0x4e76c0: a free spot for passenger `u` next to transport `tr`.
    fn unload_spot(&mut self, d: &Data, tr: u32, u: u32) -> Option<(i16, i16)> {
        let p = self.unit_sprite_pos(tr);
        // 0x4ebae0 (a copy of 0x4eb9f0).
        self.place_unit_at(d, u, p.0, p.1);
        self.find_unit_placement(d, u, p, None, false)
    }

    /// 0x4e7f70: passenger `u` gets out of its transport if the transport can let it.
    pub fn unload_unit(&mut self, d: &Data, u: u32) -> bool {
        let tr = self.units[u as usize].connected_unit;
        if tr == NIL {
            return false;
        }
        let t = &self.units[tr as usize];
        let grounded = t.status & 2 != 0;
        if (t.main_order_timer != 0 && !grounded) || t.status & 0x400 != 0 || t.lockdown_timer != 0 || t.stasis_timer != 0 || t.maelstrom_timer != 0 {
            return false;
        }
        let Some(p) = self.unload_spot(d, tr, u) else {
            return false;
        };
        self.units[tr as usize].main_order_timer = 0xf;
        self.place_unit_at(d, u, p.0, p.1);
        if self.units[u as usize].subunit != NIL {
            let sub = self.units[u as usize].subunit;
            self.place_unit_at(d, sub, p.0, p.1);
        }
        self.order_start(d, u);
        self.leave_transport(d, u, false);
        let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
        self.start_order_now(d, u, o);
        // 0x68c1e8 etc.: UI refresh flags.
        if !grounded {
            let ut = self.units[u as usize].unit_type;
            if ut == 0x53 {
                self.units[u as usize].main_order_timer = 0x1e;
            } else {
                let gw = self.ground_weapon(d, u);
                if gw != 0x82 {
                    self.units[u as usize].ground_cooldown = self.weapon_cooldown(d, u, gw);
                }
                let aw = d.dat.units.air_weapon[ut as usize];
                if aw != 0x82 {
                    self.units[u as usize].air_cooldown = self.weapon_cooldown(d, u, aw);
                }
                self.units[u as usize].spell_cooldown = 0x1e;
            }
        }
        true
    }

    /// 0x4e7e10: `u` leaves its transport's slot. Unloaded (`dying` false) it shows on
    /// the map; when it or the transport dies (0x401ea0, argument 1) it is only moved
    /// to the transport's position.
    pub(super) fn leave_transport(&mut self, d: &Data, u: u32, dying: bool) {
        if self.units[u as usize].status & 0x40 == 0 {
            return;
        }
        let tr = self.units[u as usize].connected_unit;
        if !dying || tr != NIL {
            let h = if u + 1 < 0x6a5 { self.unit_handle(u) as u16 } else { 0 };
            let cap = d.dat.units.space_provided[self.units[tr as usize].unit_type as usize] as usize;
            let slot = (0..cap).find(|&i| self.units[tr as usize].loaded_units.get(i) == Some(&h)).unwrap_or(cap);
            if dying {
                let p = self.unit_sprite_pos(tr);
                // 0x4ebae0 (a copy of 0x4eb9f0).
                self.place_unit_at(d, u, p.0, p.1);
            } else {
                // 0x48ed50 (EBX = 0x2b + the transport's race, ESI = the transport): the
                // unload sound.
                let race = Self::supply_race_of(d, self.units[tr as usize].unit_type);
                self.unit_sound(0x2b + race, tr, 0);
                self.show_unit(d, u);
            }
            if slot < 8 {
                self.units[tr as usize].loaded_units[slot] = 0;
            }
            self.units[u as usize].connected_unit = NIL;
        }
        self.units[u as usize].status &= !0x40;
        self.set_grounded(d, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.set_grounded(d, sub);
        }
    }

    /// 0x4030d0: back on the ground: status 0x20 off, 0x20000 from units.dat, path reset.
    fn set_grounded(&mut self, d: &Data, u: u32) {
        let f = d.dat.units.flags[self.units[u as usize].unit_type as usize];
        let un = &mut self.units[u as usize];
        un.status = (un.status & !0x2_0020) | ((f >> 10) & 0x2_0000);
        self.path_free(u);
        let elev = self.sprites[self.units[u as usize].sprite as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
    }

    /// 0x4790a0: BunkerGuard: a unit in a bunker that can shoot from it attacks what
    /// is in range, looking for a new target every 15 frames.
    pub fn order_bunker_guard(&mut self, d: &Data, u: u32) {
        if !matches!(self.units[u as usize].unit_type, 0 | 0x14 | 1 | 0x10 | 100 | 99 | 0x68 | 0x20 | 10) {
            return;
        }
        self.units[u as usize].status |= 0x800_0000;
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            self.units[sub as usize].status |= 0x800_0000;
        }
        if !self.attack_in_place(d, u) {
            if self.units[u as usize].main_order_timer == 0 {
                self.units[u as usize].main_order_timer = 0xf;
                let t = self.find_target_in_range(d, u);
                self.units[u as usize].order_target_unit = t;
                if t != NIL {
                    self.units[u as usize].order_process_timer = 0;
                }
            }
        } else {
            let p = self.units[u as usize].order_target_pos;
            if self.units[u as usize].fl.next_target_waypoint != p {
                self.units[u as usize].fl.next_target_waypoint = p;
            }
        }
    }

    /// 0x4e7700: MoveUnload: fly to the order position (or the target unit's position
    /// when the order started), then Unload there.
    pub fn order_move_unload(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if self.units[u as usize].order_state == 0 && t != NIL {
            let p = self.unit_sprite_pos(t);
            let un = &mut self.units[u as usize];
            un.order_state = 1;
            un.order_target_pos = p;
            un.order_target_unit = NIL;
        }
        let p = self.units[u as usize].order_target_pos;
        let e = self.units[u as usize].fl.exact;
        let dist = super::path::approx_distance(e.0, (p.0 as i32) << 8, e.1, (p.1 as i32) << 8);
        if dist & !0xff > 0x1000 {
            self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
            if self.units[u as usize].ai != NIL {
                todo_port!("0x4e7700", "AI transport unloads early near its target");
            }
            return;
        }
        self.order_queue_front(d, u, 0x6f, (0, 0));
        self.order_nothing(d, u);
    }

    /// 0x4e75d0: PickupTransport: go to the order target and pick it up (it gets
    /// EnterTransport); then wait in PickupIdle.
    pub fn order_pickup_transport(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.order_nothing(d, u);
            } else {
                let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
                self.order_now(d, u, o);
            }
            return;
        }
        if self.units[t as usize].ai != NIL && self.units[t as usize].order != 0x60 {
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].order_state != 0 {
            return;
        }
        if !self.can_load(d, u, t) {
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 7;
        if !self.box_distance_within(d, u, 1, t) {
            self.move_to_unit_aim(d, u, t);
            return;
        }
        self.unit_halt_facing(d, u);
        if !matches!(self.units[t as usize].order, 0x5c | 0x60) {
            self.order_with_target(d, t, 0x5c, u);
        }
        if self.units[u as usize].ai != NIL {
            self.units[u as usize].order_state = 1;
            return;
        }
        self.queue_front_target(d, u, 0x5d, t);
        self.order_nothing(d, u);
    }

    /// 0x4e7300: PickupIdle: pick up the target again if it still fits.
    pub fn order_pickup_idle(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL && self.can_load(d, u, t) {
            self.queue_front_target(d, u, 0x5e, t);
            self.order_nothing(d, u);
            return;
        }
        if self.units[u as usize].ai != NIL {
            self.units[u as usize].order_flags |= 1;
            // 0x474810 inlined.
            self.issue_order(d, u, 0x9c, true, (0, 0), NIL, 0xe4);
            self.order_nothing(d, u);
        }
    }
}
