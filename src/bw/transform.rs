//! Changing a unit in place: unit type (0x49fed0 / 0x49f860 / 0x49f170), owner
//! (0x49efa0), and a gas building taking over its geyser (0x4678a0).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x49fed0: turn `u` into unit type `t` where it stands.
    pub fn transform_unit(&mut self, d: &Data, u: u32, t: u16) {
        let s = self.units[u as usize].sprite;
        let shown = self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0;
        if shown {
            self.finder_remove(u);
            if self.units[u as usize].status & 2 != 0 {
                let p = self.unit_sprite_pos(u);
                self.mark_building_tiles(d, u, p, false);
            }
            if self.units[u as usize].status & 4 != 0 {
                self.repulse_remove(d, u);
            }
        }
        self.transform_unit_counted(d, u, t);
        if d.dat.units.flags[t as usize] & 1 != 0 && self.units[u as usize].status & 0x300 != 0 {
            let un = &mut self.units[u as usize];
            if un.secondary_order != 0x17 {
                un.secondary_order = 0x17;
                un.secondary_order_pos = (0, 0);
                un.current_build_unit = NIL;
                un.secondary_order_state = 0;
            }
            un.status &= !0x300;
        }
        // 0x47b770, 0x4c3b40, 0x49f7a0: a unit that can no longer be one of several leaves
        // a multiple selection.
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & 8 != 0 && !self.multi_selectable(d, u) && self.selected.len() > 1 && t != 0x24 {
            self.selection_deselect(d, u);
            self.selection_remove_local(d, u);
        }
        let ut = self.units[u as usize].unit_type;
        let mut p = self.unit_sprite_pos(u);
        self.clamp_unit_pos(d, ut, &mut p);
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            self.finder_insert(d, u);
            if self.units[u as usize].status & 2 != 0 {
                let p = self.unit_sprite_pos(u);
                self.mark_building_tiles(d, u, p, true);
            }
            self.update_unit_overlaps(d, u);
            if self.units[u as usize].status & 4 != 0 {
                self.repulse_init(d, u);
            }
        }
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            let st = d.dat.units.subunit1[t as usize];
            self.transform_subunit(d, sub, st);
        }
        self.reapply_status_overlays(d, u);
    }

    /// 0x49f6a0: the turret `sub` becomes type `t` and is put back on its base's
    /// attachment point.
    fn transform_subunit(&mut self, d: &Data, sub: u32, t: u16) {
        let cloak = self.units[sub as usize].status & 0x300;
        let mode = (self.units[sub as usize].status >> 8 & 3) as u8;
        let old_coloring = if mode != 0 {
            let s0 = self.units[sub as usize].sprite;
            (self.images[self.sprites[s0 as usize].main_image as usize].coloring & 0xffff) as u16
        } else {
            0
        };
        self.iscript_unit = Some(sub);
        self.change_unit_type(d, sub, t);
        let base = self.units[sub as usize].subunit;
        self.iscript_unit = Some(base);
        let ss = self.units[sub as usize].sprite;
        self.sprites[ss as usize].flags |= 0x10;
        if cloak != 0 {
            self.sprite_cloak_mode(d, ss, mode, old_coloring);
            if self.units[base as usize].visibility & self.local_vision_mask as u32 != 0 {
                self.sprite_detected(ss);
            }
        }
        let bs = self.units[base as usize].sprite;
        let (ox, oy) = self.image_lo_offset(d, bs, 2, 0);
        self.sprite_set_main_offset(ss, ox, oy);
        let vis = self.sprites[bs as usize].visibility;
        self.sprite_set_visibility(ss, vis);
    }

    /// 0x49f860: 0x49f170 with the unit counters taken out and put back.
    fn transform_unit_counted(&mut self, d: &Data, u: u32, t: u16) {
        let s = self.units[u as usize].sprite;
        let vis = self.sprites[s as usize].visibility;
        // 0x4e6290 (selected sprites), 0x4975d0 / 0x497590: selection, selection circle
        // and health bar images (UI) are dropped here and re-added afterwards.
        // The cloak state and the main image's fade level, to carry over.
        let cloak = self.units[u as usize].status;
        let fade = if (cloak >> 8) & 3 != 0 {
            let mi = self.sprites[s as usize].main_image;
            self.images[mi as usize].coloring as u8
        } else {
            0
        };
        self.modify_unit_counters(d, u, -1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, -1, true);
        }
        self.change_unit_type(d, u, t);
        self.modify_unit_counters(d, u, 1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, 1, true);
        }
        let s = self.units[u as usize].sprite;
        if cloak & 0x300 != 0 {
            self.sprite_cloak_mode(d, s, ((cloak >> 8) & 3) as u8, fade as u16);
            if self.units[u as usize].visibility & self.local_vision_mask as u32 != 0 {
                self.sprite_detected(s);
            }
        }
        self.sprite_set_visibility(s, vis);
    }

    /// 0x49f170: rebuild `u` as type `t` (new sprite and stats), keeping its order,
    /// owner colour and the hp ratio.
    fn change_unit_type(&mut self, d: &Data, u: u32, t: u16) {
        let un = &mut self.units[u as usize];
        let (order, state, pos, target) = (un.order, un.order_state, un.order_target_pos, un.order_target_unit);
        let hp = un.hp;
        un.previous_unit_type = un.unit_type;
        let shown_hp = (hp >> 8) as i16;
        un.previous_hp = if shown_hp == 0 { 1 } else { shown_hp as u16 };
        let s = un.sprite;
        let (x, y, owner) = (self.sprites[s as usize].x, self.sprites[s as usize].y, self.sprites[s as usize].owner);
        if self.sprites[s as usize].images.head != NIL {
            self.free_unit_sprite(u);
        }
        if self.units[u as usize].path != NIL {
            self.path_delete(u);
        }
        let player = self.units[u as usize].player;
        self.initialize_unit_base(d, u, t, x, y, player);
        let old = self.units[u as usize].previous_unit_type;
        let un = &mut self.units[u as usize];
        let st = un.status;
        un.status = st & !0x400;
        let mut nhp = (d.dat.units.hit_points[t as usize] as i32 >> 8) * (hp >> 8);
        let old_max = d.dat.units.hit_points[old as usize] as i32 >> 8;
        nhp = if old_max == 0 { 1 } else { nhp / old_max };
        un.hp = if nhp << 8 == 0 { 1 } else { nhp << 8 };
        self.refresh_strength(d, u);
        let un = &mut self.units[u as usize];
        un.status = ((d.dat.orders.interruptible[order as usize] == 0) as u32) << 12 | (st & 0xffff_cbff);
        let s = un.sprite;
        if self.sprites[s as usize].owner != owner {
            self.sprites[s as usize].owner = owner;
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                self.images[i as usize].flags |= super::sprite::iflag::REDRAW;
                i = self.images[i as usize].link.next;
            }
        }
        let un = &mut self.units[u as usize];
        if un.status & 2 != 0 {
            let slot = un.build_queue_slot as usize;
            un.build_queue[slot] = super::train::NONE;
        }
        un.order_target_unit = target;
        un.order = order;
        un.order_state = state;
        un.order_target_pos = pos;
        let set = if un.status & 1 == 0 { 0xe4 } else { t };
        if !self.disabled(u) || d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 != 0 || set == 0xe4 {
            self.units[u as usize].button_set = set;
        }
    }

    /// 0x49efa0: give `u` to `player` (`score`: count it in the new owner's score).
    pub fn change_owner(&mut self, d: &Data, u: u32, player: u8, score: bool) {
        if self.units[u as usize].player == player {
            return;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x4a1e50", "AI: unit leaves its town");
        }
        // 0x45cd00: a hatchery morphing into a lair / hive (and similar) counts as done.
        let un = &self.units[u as usize];
        let morphing = un.status & 1 == 0 && matches!(un.build_queue[un.build_queue_slot as usize], 0x85 | 0x84 | 0x89 | 0x90 | 0x92);
        if morphing {
            self.units[u as usize].status |= 1;
        }
        self.modify_unit_counters(d, u, -1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, -1, false);
        }
        // 0x4e6290 for a selected sprite (flag 8): selection lists (UI).
        // Player unit list (0x6283f8, +0x68 / +0x6c).
        let (prev, next) = (self.units[u as usize].player_prev, self.units[u as usize].player_next);
        if prev != NIL {
            self.units[prev as usize].player_next = next;
        }
        if next != NIL {
            self.units[next as usize].player_prev = prev;
        }
        let old = self.units[u as usize].player as usize;
        if self.player_units[old] == u {
            self.player_units[old] = next;
        }
        self.units[u as usize].player = player;
        let head = self.player_units[player as usize];
        self.units[u as usize].player_prev = NIL;
        self.units[u as usize].player_next = head;
        if head != NIL {
            self.units[head as usize].player_prev = u;
        }
        self.player_units[player as usize] = u;
        self.modify_unit_counters(d, u, 1);
        if self.units[u as usize].status & 1 != 0 {
            self.modify_completed_counters(d, u, 1, score);
        }
        if morphing {
            self.units[u as usize].status &= !1;
        }
        self.drop_allied_targets(d, u);
        let t = self.units[u as usize].unit_type as usize;
        let g = d.dat.units.staredit_group_flags[t];
        if g & 1 == 0 && g & 4 != 0 && self.units[u as usize].status & 2 != 0 {
            self.psi.dirty = true;
        }
        if self.player_type(player) == 1 {
            self.ai_take_unit(u);
        }
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.change_owner(d, sub, player, score);
        }
        self.apply_speed_upgrades(d, u);
        if d.dat.units.flags[t] & 8 != 0 {
            // A worker carrying a flag drops it when it joins the flag's owner (or a
            // non-player).
            let p = self.units[u as usize].ptr_at(0xc0);
            if p != NIL && self.units[p as usize].unit_type == 0xd7 && (player == self.units[p as usize].player || player > 7) {
                self.drop_powerup(d, u);
            }
        }
    }

    /// 0x476d30: units targeting `u` with weapon orders stop if it is now an ally, and
    /// `u` drops an allied target.
    fn drop_allied_targets(&mut self, d: &Data, u: u32) {
        let allied = |st: &State, a: u32, t: u32| {
            let mut p = st.units[t as usize].player;
            if p == 0xb {
                p = st.sprites[st.units[t as usize].sprite as usize].owner;
            }
            st.alliances[st.units[a as usize].player as usize][p as usize] != 0
        };
        let tgt = self.units[u as usize].order_target_unit;
        if tgt != NIL && d.dat.orders.use_weapon_targeting[self.units[u as usize].order as usize] != 0 && allied(self, u, tgt) {
            self.units[u as usize].order_target_unit = NIL;
        }
        for list in [self.visible_units.head, self.hidden_units.head] {
            let mut a = list;
            while a != NIL {
                let next = self.units[a as usize].link.next;
                let sub = self.units[a as usize].subunit;
                let c = if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 { sub } else { a };
                let cu = &self.units[c as usize];
                if cu.order_target_unit == u && d.dat.orders.use_weapon_targeting[cu.order as usize] != 0 && allied(self, c, u) {
                    self.units[c as usize].order_target_unit = NIL;
                }
                a = next;
            }
        }
    }

    /// 0x4678a0: worker `w`'s gas building of type `t` takes over the geyser at its
    /// order target. Returns the building (the former geyser).
    pub fn take_over_geyser(&mut self, d: &Data, w: u32, t: u16) -> Option<u32> {
        let mut g = self.units[w as usize].order_target_unit;
        if g == NIL || self.units[g as usize].unit_type != 0xbc {
            let p = self.units[w as usize].order_target_pos;
            g = self.unit_of_type_at(0xbc, p.0, p.1);
        }
        if g == NIL {
            return None;
        }
        // 0x6d5be4 / 0x4d5bb0: UI refresh of a selected geyser.
        self.modify_completed_counters(d, g, -1, true);
        self.units[g as usize].status &= !1;
        let player = self.units[w as usize].player;
        self.change_owner(d, g, player, false);
        self.set_unit_colors(d, g, player);
        self.transform_unit(d, g, t);
        let un = &self.units[g as usize];
        if !self.disabled(g) || d.dat.units.flags[un.unit_type as usize] & 1 != 0 {
            self.units[g as usize].button_set = 0xe7;
        }
        // 0x498d70(0x159): the building's base image under the main image.
        let s = self.units[g as usize].sprite;
        self.add_image_below_main(d, s, 0x159);
        self.units[g as usize].hp = (d.dat.units.hit_points[t as usize] / 10) as i32;
        Some(g)
    }

    /// 0x49e4e0: `u`'s sprite (and its turret's) take `player`'s colours.
    pub(super) fn set_unit_colors(&mut self, d: &Data, mut u: u32, player: u8) {
        loop {
            let s = self.units[u as usize].sprite;
            if self.sprites[s as usize].owner != player {
                self.sprites[s as usize].owner = player;
                let mut i = self.sprites[s as usize].images.head;
                while i != NIL {
                    self.images[i as usize].flags |= super::sprite::iflag::REDRAW;
                    i = self.images[i as usize].link.next;
                }
            }
            u = self.units[u as usize].subunit;
            if u == NIL || d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 == 0 {
                return;
            }
        }
    }

    /// 0x47c9f0: give `u` to `player`: the new owner gets the techs and upgrade levels
    /// that matter to it (tables 0x5011ca / 0x5012e2), queues and research stop, and
    /// the unit starts the new owner's idle order.
    pub fn give_unit(&mut self, d: &Data, u: u32, player: u8) {
        let t = self.units[u as usize].unit_type;
        let old = self.units[u as usize].player as usize;
        let p = player as usize;
        // 0x47b3c0 for each: techs the old owner has.
        for tech in give_techs(t) {
            let tech = tech as usize;
            if self.players.tech_researched[old][tech] != 0 && self.players.tech_researched[p][tech] == 0 && (tech < 0x18 || self.is_expansion) {
                self.players.tech_researched[p][tech] = 1;
            }
        }
        // 0x47b340 for each: upgrade levels the old owner has.
        for up in give_upgrades(t) {
            let up = up as usize;
            let lvl = self.players.upgrade_level(old as u8, up);
            if self.players.upgrade_level(player, up) < lvl {
                if up < 0x2e {
                    self.players.upgrade_sc[p][up] = lvl;
                } else if self.is_expansion {
                    self.players.upgrade_bw[p][up - 0x2e] = lvl;
                }
            }
        }
        // 0x49a7f0, 0x49f7a0: out of every selection, and the local one.
        self.selections_remove_unit(u);
        self.selection_remove_local(d, u);
        if self.units[u as usize].status & 2 != 0 {
            let un = &self.units[u as usize];
            if un.queue_slot_holds_unit(0) {
                self.clear_build_queue(d, u);
                // 0x499d00(0x14, 1)
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0x14);
            }
            if self.units[u as usize].u8_at(0xc8) != 0x2c {
                self.cancel_research(d, u);
            }
            if self.units[u as usize].u8_at(0xc9) != 0x3d {
                self.cancel_upgrade(d, u);
            }
        }
        if super::unit::has_hangar(t) {
            self.clear_build_queue(d, u);
        }
        self.change_owner(d, u, player, true);
        if player < 8 {
            self.set_unit_colors(d, u, player);
        }
        // 0x47b940 for each: the new owner's upgrades take effect.
        for up in give_upgrades(t) {
            if self.players.upgrade_level(self.units[u as usize].player, up as usize) != 0 {
                let pl = self.units[u as usize].player;
                self.apply_upgrade(d, pl, up as u8);
            }
        }
        let un = &self.units[u as usize];
        let c = un.current_build_unit;
        let building_addon = un.secondary_order == 0x25 && un.status & 2 != 0 && c != NIL && self.units[c as usize].status & 1 == 0;
        if !building_addon
            && un.status & 1 != 0
            && d.dat.units.flags[t as usize] & 0x800 == 0
            && !matches!(t, 0x49 | 0x55 | 0xe)
            && un.status & 0x40 == 0
        {
            let o = match self.player_types[p] {
                1 => d.dat.units.ai_computer_idle[t as usize],
                3 => 0xa1,
                7 => 0xa2,
                _ => d.dat.units.ai_human_idle[t as usize],
            };
            self.order_now(d, u, o);
        }
    }

    /// 0x47cb90: `u` becomes neutral (its addon in progress is cancelled).
    pub fn make_neutral(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let c = un.current_build_unit;
        if un.secondary_order == 0x25 && un.status & 2 != 0 && c != NIL && self.units[c as usize].status & 1 == 0 {
            self.cancel_unit(d, c);
        }
        self.give_unit(d, u, 0xb);
        if self.units[u as usize].status & 0x4000_0000 == 0 {
            self.set_secondary_order(u, 0x17);
        }
    }

    /// 0x48a1c0: `player` leaves the game: their triggers go, and unless already
    /// settled they are defeated (0x489fc0).
    pub fn leave_game(&mut self, d: &Data, player: u8) {
        self.settle_player(d, player, 2);
    }

    /// 0x48a180: `player` dropped: as 0x48a1c0, with result 1.
    pub fn player_dropped(&mut self, d: &Data, player: u8) {
        self.settle_player(d, player, 1);
    }

    /// Common body of 0x48a180 / 0x48a1c0.
    fn settle_player(&mut self, d: &Data, player: u8, result: u8) {
        let p = player as usize;
        // 0x402330: free the player's trigger list.
        if let Some(l) = self.trig.lists.get_mut(p) {
            l.clear();
        }
        if self.player_left[p] != 0 {
            return;
        }
        self.player_left[p] = result;
        self.player_defeated(d, player);
    }

    /// 0x4f3c60: `carrier` loses the powerup it carries, which dies.
    pub(super) fn kill_carried_powerup(&mut self, d: &Data, carrier: u32) {
        if self.units[carrier as usize].resource_type == 0 {
            return;
        }
        self.remove_carried_image(carrier);
        let pw = self.units[carrier as usize].ptr_at(0xc0);
        if pw != NIL {
            self.units[pw as usize].order_flags |= 4;
            self.kill_unit(d, pw);
        }
        self.units[carrier as usize].resource_type = 0;
        self.units[carrier as usize].set_ptr_at(0xc0, NIL);
    }

    /// 0x489fc0: a defeated player's slot turns into a left one and their units become
    /// neutral (capture the flag / sudden death: they die).
    pub fn player_defeated(&mut self, d: &Data, player: u8) {
        let p = player as usize;
        // ponytail: team melee (0x57f0b4 / 0x596875) changes the whole team's slots;
        // 0x57f0b4 is never set here.
        match self.player_types[p] {
            1 => self.player_types[p] = 0xb,
            2 => self.player_types[p] = 10,
            _ => {}
        }
        if matches!(self.template[8], 4 | 5) {
            // Capture the flag / sudden death: the defeated player's units die; a carried
            // powerup is taken from its carrier and dies.
            let mut u = self.player_units[p];
            while u != NIL {
                let next = self.units[u as usize].player_next;
                let un = &self.units[u as usize];
                if d.dat.units.flags[un.unit_type as usize] & 0x10 == 0 && un.sprite != NIL && un.order != 0 {
                    let carrier = un.ptr_at(0xd4);
                    if d.dat.units.flags[un.unit_type as usize] & 0x800 != 0 && carrier != NIL {
                        self.kill_carried_powerup(d, carrier);
                    } else {
                        self.kill_unit(d, u);
                    }
                }
                u = next;
            }
        } else {
            let mut u = self.player_units[p];
            while u != NIL {
                let next = self.units[u as usize].player_next;
                self.make_neutral(d, u);
                u = next;
            }
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8 / 0x68c1ec: UI refresh flags.
        }
        if self.replay {
            // 0x6d0f18 / 0x57f0b0 / 0x57eeb8: the replay stops showing their vision.
            self.local_vision_mask &= !(1u8 << (player & 7));
        }
    }

    /// 0x464930: a lifting building lets go of its addon, which becomes neutral (0x47cb90).
    pub fn detach_addon(&mut self, d: &Data, b: u32) {
        let a = self.units[b as usize].ptr_at(0xc0);
        self.make_neutral(d, a);
        self.units[b as usize].set_ptr_at(0xc0, NIL);
        self.addon_detached_sound(a);
        let saved = self.iscript_unit;
        self.iscript_unit = Some(a);
        let s = self.units[a as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x12);
        self.iscript_unit = saved;
    }
}

/// 0x5011ca: unit types and the techs (up to 5, 0x2c ends) a new owner gets from them.
const GIVE_TECHS: [(u16, [u16; 5]); 23] = [
    (0x0, [0x0, 0x2c, 0, 0, 0]),
    (0x20, [0x0, 0x2c, 0, 0, 0]),
    (0x9, [0x6, 0x7, 0x2, 0x2c, 0]),
    (0x1, [0x1, 0xa, 0x2c, 0, 0]),
    (0x2, [0x3, 0x2c, 0, 0, 0]),
    (0x5, [0x5, 0x2c, 0, 0, 0]),
    (0x1e, [0x5, 0x2c, 0, 0, 0]),
    (0x8, [0x9, 0x2c, 0, 0, 0]),
    (0xc, [0x8, 0x2c, 0, 0, 0]),
    (0x2d, [0xc, 0x12, 0xd, 0x11, 0x2c]),
    (0x25, [0xb, 0x2c, 0, 0, 0]),
    (0x26, [0xb, 0x2c, 0, 0, 0]),
    (0x29, [0xb, 0x2c, 0, 0, 0]),
    (0x32, [0xb, 0x2c, 0, 0, 0]),
    (0x2e, [0xb, 0xe, 0xf, 0x10, 0x2c]),
    (0x43, [0x17, 0x13, 0x14, 0x2c, 0]),
    (0x47, [0x15, 0x16, 0x2c, 0, 0]),
    (0x22, [0x22, 0x18, 0x1e, 0x2c, 0]),
    (0x3c, [0x19, 0x2c, 0, 0, 0]),
    (0x3d, [0x1c, 0x2c, 0, 0, 0]),
    (0x3f, [0x1b, 0x1d, 0x1f, 0x2c, 0]),
    (0x67, [0x20, 0x2c, 0, 0, 0]),
    (0x26, [0x20, 0x2c, 0, 0, 0]),
];

/// 0x5012e2: unit types and the upgrades (up to 4, 0x3d ends) a new owner gets.
const GIVE_UPGRADES: [(u16, [u16; 4]); 25] = [
    (0x0, [0x10, 0x3d, 0, 0]),
    (0x2, [0x11, 0x3d, 0, 0]),
    (0x9, [0x13, 0x3d, 0, 0]),
    (0x1, [0x14, 0x15, 0x3d, 0]),
    (0x8, [0x16, 0x3d, 0, 0]),
    (0xc, [0x17, 0x3d, 0, 0]),
    (0x2a, [0x18, 0x19, 0x1a, 0x3d]),
    (0x25, [0x1b, 0x1c, 0x3d, 0]),
    (0x26, [0x1d, 0x1e, 0x3d, 0]),
    (0x2d, [0x1f, 0x3d, 0, 0]),
    (0x2e, [0x20, 0x3d, 0, 0]),
    (0x42, [0x21, 0x3d, 0, 0]),
    (0x41, [0x22, 0x3d, 0, 0]),
    (0x53, [0x23, 0x24, 0x3d, 0]),
    (0x45, [0x25, 0x3d, 0, 0]),
    (0x54, [0x26, 0x27, 0x3d, 0]),
    (0x43, [0x28, 0x3d, 0, 0]),
    (0x46, [0x29, 0x2a, 0x3d, 0]),
    (0x48, [0x2b, 0x3d, 0, 0]),
    (0x47, [0x2c, 0x3d, 0, 0]),
    (0x3, [0x36, 0x3d, 0, 0]),
    (0x27, [0x35, 0x34, 0x3d, 0]),
    (0x22, [0x33, 0x3d, 0, 0]),
    (0x3f, [0x31, 0x3d, 0, 0]),
    (0x3c, [0x2f, 0x3d, 0, 0]),
];

/// 0x4e2c00: the techs (table 0x5012de) a unit of type `t` carries to a new owner.
fn give_techs(t: u16) -> impl Iterator<Item = u16> {
    GIVE_TECHS.iter().rev().filter(move |(ty, _)| *ty == t).flat_map(|(_, l)| l.iter().copied().take_while(|&x| x != 0x2c))
}

/// 0x4e2b50: the upgrades (table 0x5013dc) a unit of type `t` carries to a new owner.
fn give_upgrades(t: u16) -> impl Iterator<Item = u16> {
    GIVE_UPGRADES.iter().rev().filter(move |(ty, _)| *ty == t).flat_map(|(_, l)| l.iter().copied().take_while(|&x| x != 0x3d))
}
