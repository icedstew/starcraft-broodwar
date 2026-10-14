//! Completing a created unit: finish_unit_pre (0x4a01f0), placement (0x49ec30),
//! finish_unit_post (0x49fa40), show_unit (0x4e6490), set_unit_position (0x4eb9f0).

use super::list::{self, NIL};
use super::unit::status;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x4a01f0 finish_unit_pre.
    pub fn finish_unit_pre(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        let un = &mut self.units[u as usize];
        if un.remaining_build_time != 0 {
            un.hp = d.dat.units.hit_points[t as usize] as i32;
            un.shields = (d.dat.units.shields[t as usize] as i32) << 8;
            un.remaining_build_time = 0;
        }
        let st = un.status;
        let disabled = st & status::DOODAD_DISABLED != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0;
        if !disabled || d.dat.units.flags[t as usize] & 1 != 0 || t == 0xe4 {
            un.button_set = t;
        }
        if st & status::GROUNDED_BUILDING == 0 {
            if st & 0x10000 != 0 {
                let mut dir = d.dat.units.direction[t as usize];
                if dir == 0x20 {
                    dir = (self.rng.rand(0x24) & 0x1f) as u8;
                }
                self.subunit_face(d, u, dir.wrapping_mul(8));
            }
            let t = self.units[u as usize].unit_type;
            if t > 0xca && t < 0xd6 {
                self.show_unit(d, u);
            }
        } else {
            let un = &mut self.units[u as usize];
            un.parasite_flags = 0;
            un.is_blind = 0;
            self.reset_unit_image(d, u, false);
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0x10);
        }
    }

    /// 0x49ec30: place a hidden unit at a free spot near its position. False if there is none.
    pub fn place_unit(&mut self, d: &Data, u: u32) -> bool {
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            return true;
        }
        let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
        let Some(p) = self.find_unit_placement(d, u, p, None, false) else {
            let owner = self.units[u as usize].player;
            self.show_last_error(owner);
            return false;
        };
        self.set_unit_position(d, u, p.0, p.1);
        let sub = self.units[u as usize].subunit;
        let t = self.units[u as usize].unit_type as usize;
        if sub != NIL && d.dat.units.flags[t] & 0x10 == 0 {
            self.modify_completed_counters(d, u, 1, false);
            let st = self.units[sub as usize].unit_type as usize;
            self.units[sub as usize].status |= status::COMPLETED;
            self.units[sub as usize].hp = d.dat.units.hit_points[st] as i32;
            self.set_unit_position(d, sub, p.0, p.1);
        }
        true
    }


    /// 0x4eb9f0: teleport a unit.
    pub fn set_unit_position(&mut self, d: &Data, u: u32, x: i16, y: i16) {
        let mut p = (x, y);
        let t = self.units[u as usize].unit_type;
        self.clamp_unit_pos(d, t, &mut p);
        let s = self.units[u as usize].sprite;
        let old = (self.sprites[s as usize].x as i32, self.sprites[s as usize].y as i32);
        let f = &mut self.units[u as usize].fl;
        f.pos = p;
        f.exact = ((p.0 as i32) << 8, (p.1 as i32) << 8);
        self.move_sprite(s, p.0, p.1);
        if self.units[u as usize].sprite != NIL && self.units[u as usize].order != 0 {
            self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
        }
        if d.dat.units.flags[t as usize] & 0x10 == 0 {
            self.finder_move(u, p.0 as i32 - old.0, p.1 as i32 - old.1);
            if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
                self.update_unit_overlaps(d, u);
            }
        }
    }

    /// 0x49fa40 finish_unit_post.
    pub fn finish_unit_post(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[t] & 0x400 == 0 {
            self.units[u as usize].status |= status::COMPLETED;
        } else {
            self.modify_unit_counters(d, u, -1);
            self.units[u as usize].status |= status::COMPLETED;
            self.modify_unit_counters(d, u, 1);
        }
        self.modify_completed_counters(d, u, 1, true);
        if d.dat.units.flags[t] & 0x40_0000 != 0 {
            todo_port!("0x49b5b0", "units.dat flag 0x400000 on completion");
        }
        let t16 = self.units[u as usize].unit_type;
        let s = self.units[u as usize].sprite;
        if t16 == 0x21 || t16 == 0x65 {
            // Scanner sweep / map revealer: hidden list -> scanner list.
            let mut l = self.hidden_units;
            list::remove(&mut self.units, u, &mut l);
            self.hidden_units = l;
            let mut l = self.scanner_units;
            list::insert_after_head(&mut self.units, u, &mut l);
            self.scanner_units = l;
            self.unit_reveal(d, u);
        } else if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 && t16 != 0x49 && t16 != 0x55 {
            self.show_unit(d, u);
        }
        if matches!(t16, 0xcb | 0xd1 | 0xd2 | 0xd3 | 0xd4 | 0xd5) {
            let un = &mut self.units[u as usize];
            un.status |= 0x300;
            un.visibility = 0x8000_0000;
            un.ai_action_flag = 0;
        }
        let ptype = self.player_type(self.units[u as usize].player);
        if ptype == 3 {
            self.units[u as usize].order_flags |= 1;
            if self.units[u as usize].order != 0 {
                loop {
                    let tl = self.units[u as usize].order_queue_tail;
                    if tl == NIL {
                        break;
                    }
                    let o = self.orders[tl as usize].order;
                    if d.dat.orders.interruptible[o as usize] != 0 || o == 0xa1 {
                        self.order_remove(d, u, tl);
                    } else {
                        break;
                    }
                }
                self.order_queue_append(d, u, 0xa1, 0xe4, (0, 0), NIL);
            }
            self.prepare_next_order(d, u);
        } else {
            let order = match ptype {
                7 => 0xa2,
                1 => d.dat.units.ai_computer_idle[t],
                _ => d.dat.units.ai_human_idle[t],
            };
            self.order_now(d, u, order);
        }
        if d.dat.units.flags[t] & 0x800 != 0 {
            let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
            let un = &mut self.units[u as usize];
            un.set_u16_at(0xd0, p.0 as u16);
            un.set_u16_at(0xd2, p.1 as u16);
            un.order_flags |= 1;
            let order = d.dat.units.ai_human_idle[t];
            self.issue_order(d, u, order, true, (0, 0), NIL, 0xe4);
            self.prepare_next_order(d, u);
        }
        todo_port!("0x431800", "unit air/ground strength (AI)");
        self.units[u as usize].air_strength = 0;
        self.units[u as usize].ground_strength = 0;
    }

    /// 0x4e6490: make a hidden unit visible: finder, vision, movement reset, visible list.
    pub fn show_unit(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            return;
        }
        self.sprites[s as usize].flags &= !super::sprite::sflag::HIDDEN;
        let sub = self.units[u as usize].subunit;
        let sub_is_turret = |st: &State| sub != NIL && d.dat.units.flags[st.units[sub as usize].unit_type as usize] & 0x10 != 0;
        if sub_is_turret(self) {
            let ss = self.units[sub as usize].sprite;
            self.sprites[ss as usize].flags &= !super::sprite::sflag::HIDDEN;
        }
        self.unit_reveal(d, u);
        self.update_unit_visibility(d, u);
        self.finder_insert(d, u);
        if self.units[u as usize].status & status::GROUNDED_BUILDING != 0 {
            let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
            self.mark_building_tiles(d, u, p, true);
        }
        self.update_unit_overlaps(d, u);
        if self.units[u as usize].status & status::IN_AIR != 0 {
            self.repulse_init(d, u);
        }
        self.path_free(u);
        let elev = self.sprites[s as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
        if sub_is_turret(self) {
            self.path_free(sub);
            let ss = self.units[sub as usize].sprite;
            let e = self.sprites[ss as usize].elevation;
            let su = &mut self.units[sub as usize];
            su.movement_state = 0;
            su.pathing_flags = (su.pathing_flags & !1) | (e < 0xc) as u8;
        }
        let mut l = self.hidden_units;
        list::remove(&mut self.units, u, &mut l);
        self.hidden_units = l;
        let mut l = self.visible_units;
        list::insert_after_head(&mut self.units, u, &mut l);
        self.visible_units = l;
    }

    /// 0x4e6340: take a unit off the map: references, finder, tiles, visible list out,
    /// hidden list in.
    pub fn hide_unit(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return;
        }
        let mut o = self.visible_units.head;
        while o != NIL {
            let next = self.units[o as usize].link.next;
            self.forget_unit_pub(d, o, u);
            o = next;
        }
        self.untarget_bullets_pub(u);
        // 0x49a7f0: out of every selection (0x49f7a0, the local UI selection, is the front end's).
        self.selections_remove_unit(u);
        self.finder_remove(u);
        if self.units[u as usize].status & status::GROUNDED_BUILDING != 0 {
            let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
            self.mark_building_tiles(d, u, p, false);
        }
        if self.units[u as usize].status & status::IN_AIR != 0 {
            self.repulse_remove(d, u);
        }
        self.path_free(u);
        let elev = self.sprites[s as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
        let mut l = self.visible_units;
        list::remove(&mut self.units, u, &mut l);
        self.visible_units = l;
        let mut l = self.hidden_units;
        list::insert_after_head(&mut self.units, u, &mut l);
        self.hidden_units = l;
        self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
        self.sprite_set_visibility(s, 0);
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            let ss = self.units[sub as usize].sprite;
            self.sprites[ss as usize].flags |= super::sprite::sflag::HIDDEN;
            self.sprite_set_visibility(ss, 0);
        }
    }

    /// 0x4e42a0: release the unit's path (same as 0x42f740).
    pub(super) fn path_free(&mut self, u: u32) {
        self.path_delete(u);
    }

    /// 0x488d50: completed unit counts, supply provided, and (with `score`) build/unit scores.
    pub fn modify_completed_counters(&mut self, d: &Data, u: u32, delta: i32, score: bool) {
        let un = &self.units[u as usize];
        if un.status & status::HALLUCINATION != 0 {
            return;
        }
        let t = un.unit_type as usize;
        if d.dat.units.flags[t] & 0x10 != 0 {
            return;
        }
        let p = un.player as usize;
        let pl = &mut self.players;
        pl.completed_counts[t][p] += delta;
        let g = d.dat.units.staredit_group_flags[t];
        let prov = d.dat.units.supply_provided[t] as i32 * delta;
        if g & 1 != 0 {
            pl.supply_provided[0][p] += prov;
        } else if g & 2 != 0 {
            pl.supply_provided[1][p] += prov;
        } else if g & 4 != 0 {
            pl.supply_provided[2][p] += prov;
        }
        if g & 0x20 != 0 {
            pl.factories_completed[p] += delta;
        }
        if g & 8 != 0 {
            pl.men_completed[p] += delta;
        } else if g & 0x10 != 0 {
            pl.buildings_completed[p] += delta;
        }
        let bs = d.dat.units.build_score[t] as i32 * delta;
        if score && p != 0xb {
            if g & 8 != 0 {
                match t {
                    0x2c | 0x3e | 0x3f | 0x44 | 0x67 => pl.unit_score[p] += bs,
                    _ => {
                        pl.units_produced[p] += delta;
                        pl.unit_score[p] += bs;
                    }
                }
            } else if g & 0x10 != 0 {
                match t {
                    0x84 | 0x85 | 0x89 | 0x90 | 0x92 => pl.building_score[p] += bs,
                    _ => {
                        pl.buildings_constructed[p] += delta;
                        pl.building_score[p] += bs;
                    }
                }
            }
        }
        if pl.completed_counts[t][p] < 0 {
            pl.completed_counts[t][p] = 0;
        }
    }
}
