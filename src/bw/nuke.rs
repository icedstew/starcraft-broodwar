//! Nukes: NukePaint (0x463610), NukeUnit (0x479410), CastNuclearStrike (0x479200) and
//! NukeTrack (0x464d10) for ghosts, NukeTrain (0x4e6700) for silos, NukeLaunch
//! (0x464730) for the missile.
//!
//! Silo: +0xd0 the nuke, +0xd4 1 when it is ready. Ghost: +0xd0 the target dot
//! (thingy), +0x80 the missile. Missile: +0x80 the silo, then the ghost.

use super::list::NIL;
use super::{Data, State};

const NUKE: u16 = 0xe;
const SILO: u16 = 0x6c;

impl State {
    /// 0x463610: NukePaint: nuke a unit (0x7f) or a spot (0x80).
    pub(super) fn order_nuke_paint(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.order_on(d, u, 0x7f, t);
        } else {
            let p = self.units[u as usize].order_target_pos;
            self.order_issue_now(d, u, 0x80, p, NIL, 0xe4);
        }
    }

    /// 0x479410: NukeUnit: nuke the target where it stands.
    pub(super) fn order_nuke_unit(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
            self.order_cast_nuclear_strike(d, u);
            return;
        }
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
    }

    /// 0x43b970: a cloak-capable unit (ghosts, wraiths and their heroes) cloaks if its
    /// tech is available (Cloaking Field 9 for wraiths, Personal Cloaking 10 otherwise).
    /// The tail call 0x491b30 (pay the energy, secondary order 0x6d) is repeated here:
    /// the original is private to cloak.rs.
    pub(super) fn ai_cloak_before_nuke(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x100 != 0 {
            return;
        }
        let tech = match self.units[u as usize].unit_type {
            1 | 0x10 | 0x64 | 0x63 | 0x68 | 0x33 => 10,
            8 | 0x15 => 9,
            _ => return,
        };
        let player = self.units[u as usize].player;
        if self.can_use_tech(d, u, tech, player) != Ok(1) {
            return;
        }
        self.start_cloak_order(d, u);
    }

    /// 0x43db50: an AI unit not standing in an own base region walks (order 6, 0xb1 for
    /// medics, 0x70 if it was already moving there) to the center of the nearest region
    /// holding an own base unit; it joins that region's military. Returns whether it did.
    pub(super) fn ai_ghost_to_enemy_region(&mut self, d: &Data, u: u32) -> bool {
        use super::ai_town::{ai_kind, AI_MILITARY};
        let ai = self.units[u as usize].ai;
        if ai == NIL {
            return false;
        }
        let p = self.units[u as usize].player;
        let (ux, uy) = self.unit_sprite_pos(u);
        let r = super::path::region_at(&self.sai, ux as i32, uy as i32);
        let st = self.ai.regions[p as usize][r as usize].state;
        if (st == 5 || st == 4) && self.player_has_base_unit_in(d, r, p) {
            return false;
        }
        let mut best: Option<(i32, i32)> = None;
        let mut best_dist = 9999999u32;
        for i in 1..self.sai.region_count as i16 as u32 {
            let rs = self.ai.regions[p as usize][i as usize].state;
            if (rs == 5 || rs == 4) && self.player_has_base_unit_in(d, i as u16, p) {
                let c = self.sai.regions[i as usize].center;
                let (cx, cy) = (c.0 >> 8, c.1 >> 8);
                let dist = super::path::approx_distance(cx, ux as i32, cy, uy as i32);
                if dist < best_dist {
                    best_dist = dist;
                    best = Some((cx, cy));
                }
            }
        }
        let Some((cx, cy)) = best else { return false };
        if ai_kind(ai) == AI_MILITARY {
            self.ai_release_unit(d, u, false);
            let tr = super::path::region_at(&self.sai, cx, cy);
            self.region_add_military(d, p, tr, u, true);
        }
        let un = &self.units[u as usize];
        let mut order = if un.unit_type != 0x22 { 6 } else { 0xb1 };
        if un.order == 0x6f || un.order == 0x70 {
            order = 0x70;
        }
        self.order_at(d, u, order, (cx as i16, cy as i16));
        true
    }

    /// 0x479200: CastNuclearStrike: get within sight range, face the spot and launch
    /// a ready nuke from one of the player's silos.
    pub(super) fn order_cast_nuclear_strike(&mut self, d: &Data, u: u32) {
        let p = self.units[u as usize].order_target_pos;
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        let sight = self.unit_sight_range(d, u, false) as u32;
        let f = &self.units[u as usize].fl;
        let dist = super::path::approx_distance(f.exact.0, (p.0 as i32) << 8, f.exact.1, (p.1 as i32) << 8) >> 8;
        if self.units[u as usize].ai != NIL && dist <= sight * 0x60 {
            self.ai_cloak_before_nuke(d, u);
        }
        if dist > sight * 0x20 {
            if self.units[u as usize].recent_order_timer == 0 {
                self.set_move_target_xy(d, u, p);
            }
            return;
        }
        self.unit_halt(d, u);
        let f = &self.units[u as usize].fl;
        if f.next_target_waypoint != f.pos {
            let s = self.unit_sprite_pos(u);
            let w = f.next_target_waypoint;
            let dir = crate::game::flingy::direction_to(s.0 as i32, s.1 as i32, w.0 as i32, w.1 as i32) as i32;
            let diff = self.units[u as usize].fl.heading as i32 - dir;
            if !(-1..=1).contains(&diff) {
                return;
            }
        }
        let mut silo = self.player_units[self.units[u as usize].player as usize];
        while silo != NIL {
            if self.units[silo as usize].unit_type == SILO && self.units[silo as usize].u32_at(0xd4) != 0 {
                break;
            }
            silo = self.units[silo as usize].player_next;
        }
        if silo == NIL {
            self.order_done_or_next(d, u);
            return;
        }
        let n = self.units[silo as usize].ptr_at(0xd0);
        self.units[silo as usize].set_ptr_at(0xd0, NIL);
        self.units[silo as usize].set_u32_at(0xd4, 0);
        // 0x48ed50 (EBX = 0xef, ESI = the ghost): the launch sound.
        self.unit_sound(0xef, u, 0);
        self.order_issue_now(d, n, 0x7d, p, NIL, 0xe4);
        self.units[n as usize].connected_unit = u;
        // 0x401140(0): the missile's images face up.
        let s = self.units[n as usize].sprite;
        self.sprite_images_set_direction32(d, s, 0);
        self.show_unit(d, n);
        self.units[u as usize].connected_unit = n;
        let gp = self.unit_sprite_pos(u);
        self.order_issue_now(d, u, 0x81, gp, NIL, 0xe4);
        let un = &self.units[u as usize];
        if !self.disabled(u) || d.dat.units.flags[un.unit_type as usize] & 1 != 0 {
            self.units[u as usize].button_set = 0xed;
        }
        // 0x68c1b0 etc.: UI refresh.
    }

    /// 0x464d10: NukeTrack: the ghost paints the target until the missile lands.
    pub(super) fn order_nuke_track(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xd);
                let n = self.units[u as usize].connected_unit;
                let p = self.units[n as usize].order_target_pos;
                let player = self.units[u as usize].player;
                let th = self.create_thingy(d, 0xe7, p.0, p.1, player);
                match th {
                    Some(th) => {
                        self.units[u as usize].set_ptr_at(0xd0, th);
                        let ts = self.thingies[th as usize].sprite;
                        self.sprites[ts as usize].elevation = self.sprites[s as usize].elevation.wrapping_add(1);
                        self.sprite_fog_visibility(ts);
                    }
                    None => self.units[u as usize].set_ptr_at(0xd0, NIL),
                }
                self.units[u as usize].order_state = 6;
            }
            6 => {
                let n = self.units[u as usize].connected_unit;
                if n != NIL && self.units[n as usize].order_state != 5 {
                    return;
                }
                if self.units[u as usize].order_queue_head == NIL {
                    let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                    self.queue_order(d, u, o, true);
                }
                self.order_next_or_idle(d, u);
                // 0x4e5d60: the button set goes back to the unit's own.
                let t = self.units[u as usize].unit_type;
                if !self.disabled(u) || d.dat.units.flags[t as usize] & 1 != 0 || t == 0xe4 {
                    self.units[u as usize].button_set = t;
                }
                // 0x68c1b0 etc.: UI refresh.
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xc);
                let th = self.units[u as usize].ptr_at(0xd0);
                if th != NIL {
                    let ts = self.thingies[th as usize].sprite;
                    self.sprite_images_run_anim(d, ts, 1);
                    self.units[u as usize].set_ptr_at(0xd0, NIL);
                }
                if self.units[u as usize].ai != NIL {
                    self.ai_ghost_to_enemy_region(d, u);
                }
            }
            _ => {}
        }
    }

    /// 0x49f4a0 (ghosts): a dying ghost's target dot fades.
    pub(super) fn ghost_drop_nuke_dot(&mut self, d: &Data, u: u32) {
        let th = self.units[u as usize].ptr_at(0xd0);
        if th != NIL {
            let ts = self.thingies[th as usize].sprite;
            self.sprite_images_run_anim(d, ts, 1);
            self.units[u as usize].set_ptr_at(0xd0, NIL);
        }
    }

    /// 0x4e6700: NukeTrain: the silo keeps the nuke it builds and marks it ready.
    pub(super) fn order_nuke_train(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let n = self.units[u as usize].current_build_unit;
                if n != NIL {
                    self.units[u as usize].set_ptr_at(0xd0, n);
                    self.units[n as usize].connected_unit = u;
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                let n = self.units[u as usize].ptr_at(0xd0);
                if n == NIL {
                    self.units[u as usize].order_state = 0;
                    return;
                }
                if self.units[n as usize].status & 1 != 0 {
                    self.units[u as usize].set_u32_at(0xd4, 1);
                    // 0x68c1b0 etc.: UI refresh.
                    self.units[u as usize].order_state = 0;
                    if self.units[u as usize].ai != NIL {
                        self.order_now(d, u, 0x9c);
                    }
                }
            }
            _ => {}
        }
    }

    /// 0x464730: NukeLaunch: the missile rises off the top of the screen, drops on the
    /// target and blows up.
    pub(super) fn order_nuke_launch(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state < 5 && self.units[u as usize].connected_unit == NIL {
            self.kill_unit(d, u);
            return;
        }
        let dim = d.dat.units.dimensions[NUKE as usize];
        let (left, up) = (dim as i16, (dim >> 16) as i16);
        match self.units[u as usize].order_state {
            0 => {
                // 0x48ed50 (EBX = 0x54, ESI = the missile): the launch sound.
                self.unit_sound(0x54, u, 0);
                let x = self.unit_sprite_pos(u).0;
                self.move_and_face(d, u, (x, up));
                self.units[u as usize].main_order_timer = 0x5a;
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if self.units[u as usize].main_order_timer < 0x2e || self.unit_at_move_target(u) != 0 {
                    // 0x48ed50: the local race's advisor (0x7f + race); 0x48cf00:
                    // "Nuclear launch detected." for everyone.
                    // ponytail: 0x48ed50's volume gate is not ported (as in 0x48f230).
                    let me = self.local_player;
                    self.ui_sounds.push(0x7f + self.player_races[me as usize] as u16);
                    self.local_error_line(me, 0x417);
                    self.units[u as usize].order_state = 2;
                }
            }
            2 => {
                if self.units[u as usize].main_order_timer == 0 || self.unit_at_move_target(u) != 0 {
                    self.hide_unit(d, u);
                    let g = self.units[u as usize].connected_unit;
                    self.units[g as usize].connected_unit = u;
                    self.unit_halt(d, u);
                    self.units[u as usize].order_state = 3;
                }
            }
            3 => {
                if self.units[u as usize].fl.flags & 2 == 0 {
                    let s = self.units[u as usize].sprite;
                    self.sprite_images_run_anim(d, s, 0x15);
                    self.units[u as usize].order_state = 4;
                }
            }
            4 => {
                if self.units[u as usize].order_signal & 2 != 0 {
                    self.units[u as usize].order_signal &= !2;
                    let p = self.units[u as usize].order_target_pos;
                    let y = up.max(p.1.wrapping_sub(0x140));
                    let x = left.max(p.0);
                    self.place_unit_at(d, u, x, y);
                    self.set_heading_now(d, u, 0x80);
                    self.move_and_face(d, u, p);
                    self.show_unit(d, u);
                    self.units[u as usize].order_state = 5;
                }
            }
            5 => {
                let mt = self.units[u as usize].fl.move_target;
                if self.near_point(u, 10, mt) {
                    self.units[u as usize].order_target_unit = u;
                    let s = self.units[u as usize].sprite;
                    self.sprite_images_run_anim(d, s, 0xd);
                    self.units[u as usize].order_state = 6;
                }
            }
            6 => {
                if self.units[u as usize].order_signal & 1 != 0 {
                    self.units[u as usize].order_signal &= !1;
                    self.units[u as usize].order_flags |= 4;
                    self.kill_unit(d, u);
                }
            }
            _ => {}
        }
    }
}
