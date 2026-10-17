//! Killing units: the Die order and taking a unit out of the game.

use super::finder::Axis;
use super::list::{self, NIL};
use super::{Data, State};
use crate::todo_port;

impl State {
    /// Still in the game: has a sprite and is not dying (0x49a850's test).
    pub fn unit_alive(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        un.sprite != NIL && (un.order != 0 || un.order_state != 1)
    }

    /// 0x489350: player `p` is out of the game (observer slots 10 / 11, or a player
    /// whose 0x58d700 result is 1 or 2).
    pub(super) fn player_out(&self, p: u8) -> bool {
        if p >= 8 {
            return false;
        }
        let ty = self.player_type(p);
        ty == 10 || ty == 11 || (matches!(ty, 1 | 2) && matches!(self.player_left[p as usize], 1 | 2))
    }

    /// 0x488af0: count `u`'s death for its owner and the kill for `killer`.
    pub(super) fn count_death(&mut self, d: &Data, u: u32, killer: u8) {
        let p = self.units[u as usize].player as usize;
        let t = self.units[u as usize].unit_type as usize;
        let g = d.dat.units.staredit_group_flags[t];
        if self.units[u as usize].status & 0x4000_0000 != 0 || d.dat.units.flags[t] & 0x10 != 0 {
            return;
        }
        let pl = &mut self.players;
        if g & 8 != 0 {
            pl.men_lost[p] += 1;
        } else if g & 0x10 != 0 {
            pl.buildings_lost[p] += 1;
        }
        if g & 0x20 != 0 {
            pl.factories_lost[p] += 1;
        }
        pl.deaths[t][p] += 1;
        let k = killer as usize;
        if k == p || k >= 8 || self.player_out(killer) {
            return;
        }
        let score = d.dat.units.destroy_score[t] as i32;
        let pl = &mut self.players;
        if g & 8 != 0 || t == 0x23 || t == 0x24 {
            pl.men_killed[k] += 1;
            pl.kill_score[k] += score;
        } else if g & 0x10 != 0 {
            pl.buildings_razed[k] += 1;
            pl.razing_score[k] += score;
        }
        if g & 0x20 != 0 {
            pl.factories_razed[k] += 1;
        }
        pl.kills[t][k] += 1;
    }

    /// 0x475710: give `u` the Die order (clearing its queue).
    pub fn kill_unit(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].resource_type != 0 {
            self.drop_powerup(d, u);
        }
        self.clear_order_queue(d, u);
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
        self.ai_release_unit(d, u, true);
    }

    /// 0x479480: Die.
    pub fn order_die(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_flags & 4 != 0 {
            self.hide_unit(d, u);
        }
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            // 0x496f00: the turret's image joins the unit's sprite for the death.
            let ss = self.units[sub as usize].sprite;
            let s = self.units[u as usize].sprite;
            if ss != NIL && s != NIL {
                self.move_main_image(ss, s);
            }
            let ssub = self.units[sub as usize].subunit;
            if ssub != NIL && d.dat.units.flags[self.units[ssub as usize].unit_type as usize] & 0x10 != 0 {
                self.remove_subunit(d, ssub);
                self.units[sub as usize].subunit = NIL;
            }
            self.remove_unit(d, sub);
            self.free_unit_sprite(sub);
            self.units[u as usize].subunit = NIL;
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
            // 0x48ed50 (EBX = 0x26b, ESI = the unit): the hallucination death sound.
            self.unit_sound(0x26b, u, 0);
            let (x, y, elevation, vis) = {
                let sp = &self.sprites[s as usize];
                (sp.x, sp.y, sp.elevation, sp.visibility)
            };
            // 0x488210: the hallucination death sprite, one layer above.
            if let Some(t) = self.create_thingy(d, 0x176, x, y, 0) {
                let ts = self.thingies[t as usize].sprite;
                self.sprites[ts as usize].elevation = elevation.wrapping_add(1);
                self.sprite_set_visibility(ts, vis);
            }
        }
        self.hide_unit(d, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.remove_subunit(d, sub);
            self.units[u as usize].subunit = NIL;
        }
        self.remove_unit(d, u);
        self.free_unit_sprite(u);
    }

    /// 0x49fd00: a dying building's addon in progress, addon, research, upgrade and
    /// training queue end; a zerg building's creep starts receding.
    fn dying_building_cleanup(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 2 == 0 {
            return;
        }
        let un = &self.units[u as usize];
        let c = un.current_build_unit;
        if un.secondary_order == 0x25 && c != NIL && self.units[c as usize].status & 1 == 0 {
            self.cancel_unit(d, c);
        }
        if self.units[u as usize].ptr_at(0xc0) != NIL {
            self.detach_addon(d, u);
        }
        self.abort_research(d, u);
        self.abort_upgrade(d, u);
        let un = &self.units[u as usize];
        if un.queue_slot_holds_unit(0) {
            self.clear_build_queue(d, u);
        }
        // 0x48e6e0: minimap (UI).
        if d.dat.units.staredit_group_flags[self.units[u as usize].unit_type as usize] & 1 != 0 {
            // A dying lair / hive / ... morph counts as complete; while its creep cannot
            // all be queued to recede it stays (0x4000) for the dead-unit step to retry.
            if self.morphing_zerg_building(u) {
                self.units[u as usize].status |= 1;
            }
            if !self.creep_recede_unit(d, u) {
                self.units[u as usize].status |= 0x4000;
            }
        }
    }

    /// 0x4a0080: a subunit goes to the dead list and out of its owner's units; a
    /// visible gas building turns back into a neutral geyser. True when either happened.
    fn remove_subunit_or_geyser(&mut self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].unit_type;
        let flags = d.dat.units.flags[t as usize];
        if flags & 0x10 != 0 {
            let mut l = self.dead_units;
            list::insert_after_head(&mut self.units, u, &mut l);
            self.dead_units = l;
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
            self.modify_unit_counters(d, u, -1);
            self.units[u as usize].subunit = NIL;
            return true;
        }
        let s = self.units[u as usize].sprite;
        if matches!(t, 0x6e | 0x95 | 0x9d) && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            // A dying gas building turns back into a neutral geyser.
            if t == 0x95 {
                let (x, y) = (self.sprites[s as usize].x as i32, self.sprites[s as usize].y as i32);
                self.creep_clear_under(d, 0x95, x, y);
            }
            if self.units[u as usize].status & 1 != 0 {
                self.modify_completed_counters(d, u, -1, false);
            }
            self.units[u as usize].status &= !1;
            self.transform_unit(d, u, 0xbc);
            let un = &mut self.units[u as usize];
            un.order = d.dat.units.ai_computer_idle[0xbc]; // 0x662268
            un.order_target_unit = NIL;
            un.order_target_pos = (0, 0);
            un.order_state = 0;
            un.order_unit_type = 0xe4;
            un.hp = d.dat.units.hit_points[0xbc] as i32;
            self.change_owner(d, u, 0xb, false);
            self.set_unit_colors(d, u, 0xb);
            self.units[u as usize].status |= 1;
            self.modify_completed_counters(d, u, 1, true);
            // 0x48e1e0: UI only (the placement highlight: footprint_sprite 0x463a10 when the
            // active placement type 0x64088a is 0x6e / 0x9d / 0x95, then flag 0x6d5be4).
            return true;
        }
        false
    }

    /// 0x4a0740: take a dying unit out of the game (lists, finder, counters).
    pub fn remove_unit(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        let flags = d.dat.units.flags[t as usize];
        if flags & 1 != 0 {
            self.map_changed = true;
        }
        self.remove_unit_type_specific(d, u);
        self.remove_loaded_units(d, u);
        self.clear_references(d, u);
        self.untarget_bullets(u);
        self.ai_release_unit(d, u, true);
        if self.remove_subunit_or_geyser(d, u) {
            return;
        }
        self.unit_halt(d, u);
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            // 0x46a560
            self.finder_remove(u);
            if self.units[u as usize].status & 2 != 0 {
                let p = self.unit_sprite_pos(u);
                self.mark_building_tiles(d, u, p, false);
            }
            if self.units[u as usize].status & 4 != 0 {
                self.repulse_remove(d, u);
            }
        }
        if flags & 1 == 0 {
            self.units[u as usize].is_cloaked = 0;
            self.unlink_cloaked(u);
            if self.units[u as usize].status & 0x100 != 0 {
                self.end_cloak(d, u);
            }
        }
        self.dying_building_cleanup(d, u);
        if flags & 0x20 != 0 && self.units[u as usize].u8_at(0xcb) != 0 {
            // 0x469ec0: the landing spot it had reserved.
            let p = self.units[u as usize].order_target_pos;
            self.mark_building_tiles(d, u, p, false);
        }
        if self.units[u as usize].path != NIL {
            self.path_delete(u);
        }
        let so = self.units[u as usize].secondary_order;
        let c = self.units[u as usize].current_build_unit;
        if c != NIL && (so == 0x26 || so == 0x3f) {
            self.kill_unit(d, c);
        }
        if so != 0x17 {
            self.set_secondary_order(u, 0x17);
        }
        if self.units[u as usize].resource_type != 0 {
            self.drop_powerup(d, u);
        }
        // 0x49a7f0: out of every selection (selection circles are not ported).
        self.selections_remove_unit(u);
        self.selection_remove_local(d, u);
        self.modify_unit_counters(d, u, -1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, -1, false);
        }
        self.move_to_dead_list(u);
    }

    /// 0x49f4a0: per-type cleanup (workers' carried resources, mines, nydus, ...).
    fn remove_unit_type_specific(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        match t {
            0x48 | 0x52 | 0x53 | 0x51 => self.kill_fighters(d, u),
            1 | 0x10 | 100 | 99 | 0x68 => {
                self.ghost_drop_nuke_dot(d, u);
            }
            _ => {
                // Minerals and completed gas buildings (0x4688b0) hold resources.
                let st = self.units[u as usize].status;
                if (0xb0..=0xb2).contains(&t) || self.is_completed_refinery(u) {
                    self.clear_resource_workers(u);
                }
                match t {
                    0xe => {
                        let c = self.units[u as usize].connected_unit;
                        if c != NIL && self.units[c as usize].unit_type == 0x6c {
                            self.units[c as usize].set_ptr_at(0xd0, NIL);
                            self.units[c as usize].set_ptr_at(0xd4, NIL);
                        }
                    }
                    0x49 | 0x55 if st & 1 != 0 => self.fighter_unlink(u),
                    0x6c => {
                        let n = self.units[u as usize].ptr_at(0xd0);
                        if n != NIL {
                            self.kill_unit(d, n);
                            self.units[u as usize].set_ptr_at(0xd0, NIL);
                        }
                    }
                    0x86 => {
                        let o = self.units[u as usize].ptr_at(0xd0);
                        if o != NIL {
                            self.units[u as usize].set_ptr_at(0xd0, NIL);
                            self.units[o as usize].set_ptr_at(0xd0, NIL);
                            self.kill_unit(d, o);
                        }
                    }
                    0x9c => {
                        let s = self.units[u as usize].ptr_at(0xd0);
                        if s != NIL {
                            self.free_sprite_images(s);
                            self.units[u as usize].set_ptr_at(0xd0, NIL);
                        }
                        self.remove_psi_provider(u);
                        self.psi.dirty = true;
                    }
                    _ => {}
                }
            }
        }
    }

    /// 0x468770: a resource goes away: nobody gathers it and its wait list empties.
    fn clear_resource_workers(&mut self, r: u32) {
        self.units[r as usize].set_u8_at(0xd3, 0);
        let mut w = self.units[r as usize].ptr_at(0xd4);
        while w != NIL {
            let next = self.units[w as usize].ptr_at(0xd8);
            self.leave_wait_list(w, r);
            w = next;
        }
    }

    /// 0x49fdd0: units inside a transport die with it.
    fn remove_loaded_units(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x40 != 0 {
            self.leave_transport(d, u, true);
            return;
        }
        if un.status & 0x4000_0000 != 0 || d.dat.units.space_provided[un.unit_type as usize] == 0 {
            return;
        }
        if un.unit_type == 0x2a && self.players.upgrade_sc[un.player as usize][0x18] == 0 {
            return;
        }
        // A bunker lets its passengers out; other transports take them along.
        let bunker = un.unit_type == 0x7d;
        for i in 0..8 {
            if let Some(p) = self.loaded_unit(u, i) {
                if self.units[p as usize].order != 0 && (!bunker || !self.unload_unit(d, p)) {
                    self.kill_unit(d, p);
                }
            }
        }
        self.units[u as usize].loaded_units = [0; 8];
    }

    /// The visible-units half of 0x49eb70, as inlined in 0x493ca0 (hiding a unit).
    pub(super) fn clear_references_visible(&mut self, d: &Data, u: u32) {
        let mut o = self.visible_units.head;
        while o != NIL {
            let next = self.units[o as usize].link.next;
            self.forget_unit(d, o, u);
            o = next;
        }
    }

    pub(super) fn forget_unit_pub(&mut self, d: &Data, o: u32, dead: u32) {
        self.forget_unit(d, o, dead);
    }

    pub(super) fn untarget_bullets_pub(&mut self, u: u32) {
        self.untarget_bullets(u);
    }

    /// 0x49eb70(1): every unit forgets `dead` (and the UI drops it).
    fn clear_references(&mut self, d: &Data, dead: u32) {
        self.clear_references_visible(d, dead);
        todo_port!("0x49eb70", "selection / unit portrait UI (0x68ac98)");
        let mut o = self.hidden_units.head;
        while o != NIL {
            let next = self.units[o as usize].link.next;
            self.forget_unit(d, o, dead);
            o = next;
        }
        let t = self.units[dead as usize].unit_type as usize;
        let r = self.units[dead as usize].ptr_at(0xd0);
        if d.dat.units.flags[t] & 8 != 0 && r != NIL {
            self.worker_release(d, dead, r);
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
        if d.dat.units.flags[un.unit_type as usize] & 8 == 0 {
            if un.status & 2 != 0 && un.ptr_at(0xc0) == dead {
                un.set_ptr_at(0xc0, NIL);
            }
        } else {
            if un.ptr_at(0xc8) == dead {
                un.set_ptr_at(0xc8, NIL);
            }
            if un.u8_at(0xce) != 0 && un.ptr_at(0xd0) == dead {
                un.set_u8_at(0xce, 0);
                un.set_ptr_at(0xd0, NIL);
            }
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
        self.free_sprite_images(s);
        self.units[u as usize].sprite = NIL;
    }

    /// 0x497b40: free a sprite and its images.
    pub fn free_sprite_images(&mut self, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.destroy_image(i);
            i = next;
        }
        self.free_sprite(s);
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
        self.hp_changed(d, u);
    }

    /// The tail of 0x467340 after the hp is stored: damage flames follow the hp.
    pub(super) fn hp_changed(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 1 == 0 {
            return;
        }
        let s = self.units[u as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        if main != NIL && d.has_damage_overlays(self.images[main as usize].image_id) {
            self.update_damage_overlays(d, u);
        }
        self.refresh_strength(d, u);
    }

    /// 0x4e6090: as many damage flames as the hp below two thirds calls for, one level
    /// (+0xa7) per step: flames go out (0x499580) or appear (0x4993c0).
    pub(super) fn update_damage_overlays(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        let max = d.dat.units.hit_points[self.units[u as usize].unit_type as usize] as i32;
        let third = max / 3;
        let rest = (max - third) as u32 as u64;
        let step = rest / (d.damage_overlay_count(self.images[main as usize].image_id) as u64 + 1);
        let hp = self.units[u as usize].hp;
        let level = (((hp - (rest % step) as i32) - third - 1) / step as i32).max(0) as u8;
        let mut cur = self.units[u as usize].building_overlay_state;
        while cur < level {
            self.remove_damage_overlay(d, s);
            cur += 1;
            self.units[u as usize].building_overlay_state = cur;
        }
        while cur > level {
            self.add_damage_overlay(d, s);
            cur -= 1;
            self.units[u as usize].building_overlay_state = cur;
        }
    }

    /// The damage LO point `idx` of sprite `s`'s main image (x mirrored when flipped).
    fn damage_point(&self, d: &Data, s: u32, idx: u16) -> (i8, i8) {
        let main = self.sprites[s as usize].main_image;
        let img = &self.images[main as usize];
        let (mut x, y) = d.lo_offset(1, img.image_id, img.frame, idx as i8).unwrap_or((0, 0));
        if img.flags & super::sprite::iflag::FLIPPED != 0 {
            x = x.wrapping_neg();
        }
        (x, y)
    }

    /// 0x4993c0: one more flame: a small one (images 0x1c2..) grows into a big one
    /// (0x1d8..); otherwise a new small flame at a random free damage point (RNG site 8).
    fn add_damage_overlay(&mut self, d: &Data, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let id = self.images[i as usize].image_id;
            if (0x1c2..0x1d8).contains(&id) {
                let idx = id - 0x1c2;
                let (x, y) = self.damage_point(d, s, idx);
                self.destroy_image(i);
                let head = self.sprites[s as usize].images.head;
                self.create_image(d, s, head, idx + 0x1d8, x, y, 0);
                return;
            }
            i = self.images[i as usize].link.next;
        }
        let r = self.rng.rand(8);
        let main = self.sprites[s as usize].main_image;
        let slots = (d.damage_overlay_count(self.images[main as usize].image_id) >> 1) as u32;
        if slots == 0 {
            return;
        }
        let mut idx: u16 = 0;
        let mut count = 0u32;
        loop {
            if idx > 0x15 {
                idx = 0;
            }
            let (x, y) = self.damage_point(d, s, idx);
            if x != 0x7f || y != 0x7f {
                count += 1;
                let mut big = false;
                let mut j = self.sprites[s as usize].images.head;
                while j != NIL {
                    if self.images[j as usize].image_id == idx + 0x1d8 {
                        big = true;
                        break;
                    }
                    j = self.images[j as usize].link.next;
                }
                if !big && r % slots + 1 <= count {
                    let head = self.sprites[s as usize].images.head;
                    self.create_image(d, s, head, idx + 0x1c2, x, y, 0);
                    return;
                }
            }
            idx += 1;
        }
    }

    /// 0x499580: one flame less: a big flame shrinks back to a small one, else a small
    /// one goes out.
    fn remove_damage_overlay(&mut self, d: &Data, s: u32) {
        let main = self.sprites[s as usize].main_image;
        if main == NIL || !d.has_damage_overlays(self.images[main as usize].image_id) {
            return;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let id = self.images[i as usize].image_id;
            if (0x1d8..0x1ee).contains(&id) {
                let idx = id - 0x1d8;
                let (x, y) = self.damage_point(d, s, idx);
                self.destroy_image(i);
                let head = self.sprites[s as usize].images.head;
                self.create_image(d, s, head, idx + 0x1c2, x, y, 0);
                return;
            }
            i = self.images[i as usize].link.next;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let id = self.images[i as usize].image_id;
            if (0x1c2..=0x1d7).contains(&id) {
                self.destroy_image(i);
                return;
            }
            i = self.images[i as usize].link.next;
        }
    }

    /// 0x4a0990: a turret (and its own turret) leaves the game with its sprite.
    pub(super) fn remove_subunit(&mut self, d: &Data, u: u32) {
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.remove_subunit(d, sub);
            self.units[u as usize].subunit = NIL;
        }
        self.remove_unit(d, u);
        self.free_unit_sprite(u);
    }

    /// 0x496f00: move sprite `from`'s main image to the front of sprite `to`.
    fn move_main_image(&mut self, from: u32, to: u32) {
        let img = self.sprites[from as usize].main_image;
        if img == NIL {
            return;
        }
        self.sprites[from as usize].main_image = NIL;
        let mut l = self.sprites[from as usize].images;
        list::remove(&mut self.images, img, &mut l);
        self.sprites[from as usize].images = l;
        let mut l = self.sprites[to as usize].images;
        list::insert_after_head(&mut self.images, img, &mut l);
        self.sprites[to as usize].images = l;
        self.images[img as usize].flags &= !0x8;
        self.images[img as usize].sprite = to;
    }
}
