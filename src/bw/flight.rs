//! Terran buildings lifting off (1.16.1 command 0x2f, orders BuildingLiftOff and
//! LiftingOff), with the lift-off dust.

use super::list::NIL;
use super::{Data, State};

impl State {
    /// 0x4c1620: command 0x2f, lift the one selected building and fly it to (x, y).
    pub fn command_lift(&mut self, d: &Data, player: u8, sel: &[u32], x: i16, y: i16) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        let un = &self.units[u as usize];
        if self.busy_building(u) || un.u8_at(0xc8) != 0x2c || un.u8_at(0xc9) != 0x3d || un.status & 2 == 0 {
            return;
        }
        if d.dat.units.flags[un.unit_type as usize] & 0x20 == 0 {
            return;
        }
        // The inline checks are 0x46dc20's for order 0x48.
        if self.can_issue_order(d, u, 0x48, player).is_err() {
            return;
        }
        if x as u16 >= self.map_px.0 || y as u16 >= self.map_px.1 {
            return;
        }
        // BuildingLiftOff now, then the idle order queued above it.
        self.order_at(d, u, 0x48, (x, y));
        let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.queue_order_at(d, u, idle, (x, y.wrapping_sub(0x2a)), false);
    }

    /// 0x46a560: take a unit out of the unit finder, its building tiles and the repulse grid.
    pub(super) fn unit_leave_map_grids(&mut self, d: &Data, u: u32) {
        self.finder_remove(u);
        if self.units[u as usize].status & 2 != 0 {
            let p = self.unit_sprite_pos(u);
            self.mark_building_tiles(d, u, p, false);
        }
        if self.units[u as usize].status & 4 != 0 {
            self.repulse_remove(d, u);
        }
    }

    /// 0x46a5a0: put a unit back into the unit finder, building tiles and repulse grid.
    pub(super) fn unit_enter_map_grids(&mut self, d: &Data, u: u32) {
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

    /// 0x401360: set or clear the air status (0x4), dropping the path and movement state.
    pub(super) fn set_air(&mut self, u: u32, air: bool) {
        let un = &mut self.units[u as usize];
        un.status = (un.status & !4) | if air { 4 } else { 0 };
        self.path_free(u);
        let elev = self.sprites[self.units[u as usize].sprite as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
    }

    /// 0x463470: no longer a grounded building; 0x20000 unless a subunit type.
    pub(super) fn lift_off_status(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        let un = &mut self.units[u as usize];
        un.status = (un.status & !2) | 0x1_0000;
        if d.dat.units.flags[t as usize] & 0x10 == 0 {
            un.status |= 0x2_0000;
        } else {
            un.status &= !0x2_0000;
        }
        self.path_free(u);
        let elev = self.sprites[self.units[u as usize].sprite as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
    }

    /// 0x4634e0: grounded building again.
    pub(super) fn land_status(&mut self, u: u32) {
        let un = &mut self.units[u as usize];
        un.status = (un.status & 0xfffc_ffff) | 2;
        self.path_free(u);
        let elev = self.sprites[self.units[u as usize].sprite as usize].elevation;
        let un = &mut self.units[u as usize];
        un.movement_state = 0;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
    }

    /// 0x4649b0: BuildingLiftOff: the building leaves the ground grid and becomes a
    /// flyer, then LiftingOff (0x4a) animates it.
    pub fn order_building_lift_off(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        if self.units[u as usize].status & 2 == 0 || d.dat.units.flags[t as usize] & 0x20 == 0 {
            if self.units[u as usize].order_queue_head == NIL {
                let idle = d.dat.units.ai_return_to_idle[t as usize];
                self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
            }
            self.order_next_or_idle(d, u);
            return;
        }
        if self.units[u as usize].ptr_at(0xc0) != NIL {
            self.detach_addon(d, u);
        }
        // 0x47b2e0: town halls (and the other rally-capable types) rally to themselves.
        if matches!(t, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b) {
            self.units[u as usize].rally_unit = u;
        }
        self.unit_leave_map_grids(d, u);
        self.lift_off_status(d, u);
        let s = self.units[u as usize].sprite;
        self.sprites[s as usize].elevation = 0xc;
        self.set_air(u, true);
        self.unit_enter_map_grids(d, u);
        // The shadow (the sprite's last image) stops following the building's height.
        let tail = self.sprites[s as usize].images.tail;
        if tail != NIL && self.images[tail as usize].modifier == 10 {
            self.images[tail as usize].flags |= super::sprite::iflag::FREEZE_Y;
        }
        let (x, y) = self.unit_sprite_pos(u);
        let p = (x, y.wrapping_sub(0x2a));
        if self.units[u as usize].fl.next_target_waypoint != p {
            self.units[u as usize].fl.next_target_waypoint = p;
        }
        if t == 0x82 {
            self.creep_recede_unit(d, u);
        }
        self.units[u as usize].order = 0x4a;
    }

    /// 0x463ac0: LiftingOff: turn, rise to flying height with the lift-off animation and
    /// dust, then the next order.
    pub fn order_lifting_off(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type as usize;
        let s = self.units[u as usize].sprite;
        // Flying height: 0x2a, or 7 for units.dat flag 8.
        let height: i16 = if d.dat.units.flags[t] & 8 != 0 { 7 } else { 0x2a };
        let mut state = self.units[u as usize].order_state;
        if state == 0 {
            let sp = (self.sprites[s as usize].x, self.sprites[s as usize].y);
            let f = &self.units[u as usize].fl;
            let dir = crate::game::flingy::direction_to(sp.0 as i32, sp.1 as i32, f.next_target_waypoint.0 as i32, f.next_target_waypoint.1 as i32);
            if f.next_target_waypoint != f.pos && f.velocity_dir_prev != dir {
                return;
            }
            state = 1;
        }
        if state == 1 {
            let tail = self.sprites[s as usize].images.tail;
            let ty = if tail != NIL { self.images[tail as usize].y as i16 } else { 0 };
            let sp = (self.sprites[s as usize].x, self.sprites[s as usize].y);
            let p = (sp.0, ty - height + sp.1);
            self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
            self.units[u as usize].order_state = 2;
            return;
        }
        if state == 2 {
            self.sprite_images_run_anim(d, s, 0x12);
            self.units[u as usize].order_state = 3;
            self.dust(d, s, 4);
            let f = &mut self.units[u as usize].fl;
            f.top_speed = 0x100;
            super::flingy::set_speeds(f, 0x100);
        }
        // state 3
        let un = &self.units[u as usize];
        if un.fl.move_target != self.unit_sprite_pos(u) || un.order_signal & 0x10 == 0 {
            return;
        }
        self.units[u as usize].order_signal &= !0x10;
        if d.dat.units.flags[t] & 1 == 0 {
            let top = self.unit_top_speed(d, u);
            self.units[u as usize].fl.top_speed = top as i32;
        }
        let tail = self.sprites[s as usize].images.tail;
        if tail != NIL && self.images[tail as usize].modifier == 10 {
            let img = &mut self.images[tail as usize];
            if img.y != height as i8 {
                img.flags |= super::sprite::iflag::REDRAW;
                img.y = height as i8;
            }
            img.flags &= !super::sprite::iflag::FREEZE_Y;
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x499820 (lift-off, overlay kind 4) / 0x4997e0 (landing, kind 3): dust sprites
    /// 0x143.. at overlay points 0..=8 and, mirrored, 0x10..=0x18 (`dust_row`).
    fn dust(&mut self, d: &Data, s: u32, kind: usize) {
        let main = self.sprites[s as usize].main_image;
        if main == NIL || d.lo.get(kind).and_then(|v| v.get(self.images[main as usize].image_id as usize)).map_or(true, |l| l.is_none()) {
            return;
        }
        self.dust_row(d, s, kind, 0, 8, 0x143, false);
        self.dust_row(d, s, kind, 0x10, 0x18, 0x143, true);
    }

    /// 0x499660 (EAX = `first`, EBX = sprite `s`): for overlay points `first..=last` of
    /// the main image's lo file `kind`, a thingy of sprite `sprite_id` (+1 per point) one
    /// elevation above `s`; flipped when `mirror`.
    #[allow(clippy::too_many_arguments)]
    fn dust_row(&mut self, d: &Data, s: u32, kind: usize, first: i8, last: i8, mut sprite_id: u16, mirror: bool) {
        let main = self.sprites[s as usize].main_image;
        for idx in first..=last {
            let img = &self.images[main as usize];
            let Some((mut lx, ly)) = d.lo_offset(kind, img.image_id, img.frame, idx) else {
                sprite_id += 1;
                continue;
            };
            if img.flags & super::sprite::iflag::FLIPPED != 0 {
                lx = lx.wrapping_neg();
            }
            if lx != 0x7f || ly != 0x7f {
                let sp = &self.sprites[s as usize];
                let (x, y, elev) = (sp.x + lx as i16, sp.y + ly as i16, sp.elevation);
                if let Some(th) = self.create_thingy(d, sprite_id, x, y, 0) {
                    let ts = self.thingies[th as usize].sprite;
                    self.sprites[ts as usize].elevation = elev.wrapping_add(1);
                    self.sprite_fog_visibility(ts);
                    if mirror {
                        self.sprite_set_flip(d, ts, true);
                    }
                }
            }
            sprite_id += 1;
        }
}

    /// 0x4878f0: a sprite's visibility from the fog at its tile.
    pub fn sprite_fog_visibility(&mut self, s: u32) {
        if s == NIL || self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return;
        }
        let sp = &self.sprites[s as usize];
        let i = (sp.y as i32 / 32) as usize * self.terrain.width as usize + (sp.x as i32 / 32) as usize;
        let v = !(self.terrain.tile_flags.get(i).copied().unwrap_or(0) as u8);
        if sp.visibility != v {
            self.sprite_set_visibility(s, v);
        }
    }

    /// 0x4eb9c0: move target and next target waypoint to (x, y).
    fn move_to_point(&mut self, d: &Data, u: u32, p: (i16, i16)) {
        self.set_move_target_xy(d, u, p);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// 0x4eb9f0: put `u` at (x, y) (clamped to the map by its box, 0x401fa0), keeping
    /// its order's movement there and the unit finder in step.
    /// 0x4ebae0 is an identical copy in 1.16.1.
    pub(super) fn place_unit_at(&mut self, d: &Data, u: u32, x: i16, y: i16) {
        let t = self.units[u as usize].unit_type as usize;
        let dim = d.dat.units.dimensions[t];
        let (l, top, r, b) = (dim as i16, (dim >> 16) as i16, (dim >> 32) as i16, (dim >> 48) as i16);
        let mut x = x;
        let mut y = y;
        if x < l {
            x = l;
        } else if x as i32 >= self.map_px.0 as i32 - r as i32 {
            x = (self.map_px.0 as i16).wrapping_sub(r).wrapping_sub(1);
        }
        if y < top {
            y = top;
        } else if y as i32 >= self.map_px.1 as i32 - b as i32 - 0x20 {
            y = (self.map_px.1 as i16).wrapping_sub(b).wrapping_sub(0x21);
        }
        let s = self.units[u as usize].sprite;
        let (ox, oy) = (self.sprites[s as usize].x, self.sprites[s as usize].y);
        let f = &mut self.units[u as usize].fl;
        f.pos = (x, y);
        f.exact = ((x as i32) << 8, (y as i32) << 8);
        self.move_sprite(s, x, y);
        if self.units[u as usize].order != 0 {
            self.move_to_point(d, u, (x, y));
        }
        if d.dat.units.flags[t] & 0x10 == 0 {
            self.finder_move(u, x as i32 - ox as i32, y as i32 - oy as i32);
            if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
                self.update_unit_overlaps(d, u);
            }
        }
    }

    /// 0x464360: BuildingLand: fly over the spot, check it, descend with the landing
    /// animation and become a grounded building again.
    pub fn order_building_land(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 2 != 0 {
            // Grounded: lift off first, then land at the same target.
            let un = &self.units[u as usize];
            let (pos, target) = (un.order_target_pos, un.order_target_unit);
            self.units[u as usize].order_flags |= 1;
            self.issue_order(d, u, 0x48, true, (0, 0), NIL, 0xe4);
            self.prepare_next_order(d, u);
            if self.units[u as usize].order != 0 {
                loop {
                    let t = self.units[u as usize].order_queue_tail;
                    if t == NIL || self.orders[t as usize].order != 0x47 {
                        break;
                    }
                    self.order_remove(d, u, t);
                }
                self.order_queue_append(d, u, 0x47, 0xe4, pos, target);
            }
            let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
            return;
        }
        let s = self.units[u as usize].sprite;
        let tgt = self.units[u as usize].order_target_pos;
        let tail_y = |st: &State| {
            let tail = st.sprites[s as usize].images.tail;
            if tail != NIL { st.images[tail as usize].y as i16 } else { 0 }
        };
        match self.units[u as usize].order_state {
            0 => {
                let p = (tgt.0, tgt.1 - tail_y(self));
                self.move_to_point(d, u, p);
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if self.unit_at_move_target(u) == 0 {
                    return;
                }
                let t = self.units[u as usize].unit_type;
                let (pw, ph) = super::build::placement(d, t);
                // C rounds toward zero: (v + (v >> 31 & 0x1f)) >> 5.
                let tz = |v: i32| ((v + ((v >> 31) & 0x1f)) >> 5) as i16;
                let (tx, ty) = (tz(tgt.0 as i32 - pw / 2), tz(tgt.1 as i32 - ph / 2));
                let p = self.units[u as usize].player;
                if self.placement_check(d, u, p, tx, ty, t, 0, false, true, false) != 0 {
                    self.land_blocked_sound(d, u);
                    let head = self.units[u as usize].order_queue_head;
                    if head != NIL && self.orders[head as usize].order == 0x24 {
                        self.order_remove(d, u, head);
                    }
                    self.order_done_or_next(d, u);
                    return;
                }
                self.mark_building_tiles(d, u, tgt, true);
                self.units[u as usize].set_u8_at(0xcb, 1);
                self.move_to_point(d, u, tgt);
                let tail = self.sprites[s as usize].images.tail;
                if tail != NIL && self.images[tail as usize].modifier == 10 {
                    self.images[tail as usize].flags |= super::sprite::iflag::FREEZE_Y;
                }
                self.units[u as usize].status |= 0x1000;
                let f = &mut self.units[u as usize].fl;
                f.top_speed = 0x100;
                super::flingy::set_speeds(f, 0x100);
                self.units[u as usize].order_state = 2;
            }
            2 | 3 => {
                if self.units[u as usize].order_state == 2 {
                    let sp = (self.sprites[s as usize].x, self.sprites[s as usize].y);
                    let f = &self.units[u as usize].fl;
                    let dir = crate::game::flingy::direction_to(sp.0 as i32, sp.1 as i32, f.next_target_waypoint.0 as i32, f.next_target_waypoint.1 as i32);
                    // At the next target waypoint, or facing it.
                    if !super::unit::at_target_waypoint(f) && f.velocity_dir_prev != dir {
                        return;
                    }
                    self.sprite_images_run_anim(d, s, 0x11);
                    self.units[u as usize].order_state = 3;
                }
                if self.unit_at_move_target(u) == 0 || self.units[u as usize].order_signal & 0x10 == 0 {
                    return;
                }
                self.units[u as usize].order_signal &= !0x10;
                self.land_finish(d, u);
            }
            _ => {}
        }
    }

    /// The landing's last step (0x464360 state 3).
    fn land_finish(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        let tgt = self.units[u as usize].order_target_pos;
        self.mark_building_tiles(d, u, tgt, false);
        self.unit_leave_map_grids(d, u);
        self.land_status(u);
        self.sprites[s as usize].elevation = 4;
        self.set_air(u, false);
        self.place_unit_at(d, u, tgt.0, tgt.1);
        self.unit_enter_map_grids(d, u);
        if self.sprites[s as usize].flags & 8 != 0 && self.selected.len() > 1 {
            self.selection_deselect(d, u);
        }
        // 0x498150(0): the shadow back on the ground.
        let tail = self.sprites[s as usize].images.tail;
        if tail != NIL && self.images[tail as usize].modifier == 10 {
            let img = &mut self.images[tail as usize];
            if img.y != 0 {
                img.flags |= super::sprite::iflag::REDRAW;
                img.y = 0;
            }
            img.flags &= !super::sprite::iflag::FREEZE_Y;
        }
        self.dust(d, s, 3);
        self.units[u as usize].set_u8_at(0xcb, 0);
        self.land_crush(d, u);
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            let o = self.orders[head as usize].order;
            if o == 6 || o == 0x31 {
                // Lift off again to carry out the move.
                self.queue_order_before(d, u, 0x48, head);
            } else {
                let keep = if o == 0x24 { head } else { NIL };
                while self.units[u as usize].order_queue_tail != keep {
                    let tail = self.units[u as usize].order_queue_tail;
                    self.order_remove(d, u, tail);
                }
                if self.units[u as usize].order_queue_head == NIL {
                    let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                    self.start_order_now(d, u, idle);
                }
            }
        }
        self.land_blocked_check(d, u);
        self.order_next_or_idle(d, u);
        if matches!(self.units[u as usize].unit_type, 0x6a | 0x71 | 0x72 | 0x74) {
            self.take_adjacent_addon(d, u);
        }
    }

    /// The units in the landed building's box (0x4e8280 over its box).
    fn units_under(&mut self, d: &Data, u: u32) -> Vec<u32> {
        let (x, y) = self.unit_sprite_pos(u);
        let t = self.units[u as usize].unit_type as usize;
        let dim = d.dat.units.dimensions[t];
        let (l, top, r, b) = (dim as i16, (dim >> 16) as i16, (dim >> 32) as i16, (dim >> 48) as i16);
        let bx = [x.wrapping_sub(l), y.wrapping_sub(top), x.wrapping_add(r), y.wrapping_add(b)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, bx);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
    }

    /// 0x464300 with callback 0x463fc0: units under the landed building that its owner
    /// is allied with (seen from them) die: burrowed ones, and armed ones that can't be
    /// hidden (0x47b810).
    fn land_crush(&mut self, d: &Data, u: u32) {
        let owner = self.units[u as usize].player;
        for c in self.units_under(d, u) {
            if c == u || !self.unit_alive(c) {
                continue;
            }
            let o = if owner == 0xb { self.sprites[self.units[u as usize].sprite as usize].owner } else { owner };
            let allied = self.alliances[self.units[c as usize].player as usize][o as usize] != 0;
            if !allied {
                continue;
            }
            if (!self.hides_under(d, c) && self.is_armed_pub(d, c)) || self.units[c as usize].status & 0x10 != 0 {
                self.kill_unit(d, c);
            }
        }
    }

    /// 0x47b810: a ground unit that hides (burrowed lurker or a units.dat
    /// right-click type) other than type 3 (0x4e5ea0).
    fn hides_under(&self, d: &Data, c: u32) -> bool {
        let un = &self.units[c as usize];
        if un.status & 2 != 0 {
            return false;
        }
        let t = un.unit_type as usize;
        let lurker = t == 0x67 && un.status & 0x10 != 0;
        if !lurker && d.dat.units.right_click_action[t] == 0 {
            return false;
        }
        // 0x4e5ea0
        let kind = if lurker { 3 } else { d.dat.units.right_click_action[t] };
        kind != 3
    }

    /// 0x463640 with callback 0x4635b0: an armed unit with an order under the building
    /// makes it lift off again.
    fn land_blocked_check(&mut self, d: &Data, u: u32) {
        let mut blocked = false;
        for c in self.units_under(d, u) {
            if c == u {
                continue;
            }
            if self.hides_under(d, c) {
                continue;
            }
            if self.is_armed_pub(d, c) && self.units[c as usize].order != 0 {
                blocked = true;
                break;
            }
        }
        if !blocked {
            return;
        }
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, 0x48, true, (0, 0), NIL, 0xe4);
        self.order_nothing(d, u);
        let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
    }
}
