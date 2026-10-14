//! Unit movement state machine (1.16.1 0x46c480) and its state handlers.
//!
//! State byte is CUnit +0x97. Each handler returns whether to re-dispatch this frame.
//! Pathfinding (0x42xxxx), unit collision (0x4f1xxx/0x4f2xxx) and the unit finder
//! are not ported yet; their call sites go through the `stub_*` functions below,
//! which return the value that means "nothing in the way / no path work".

use super::flingy::{self, set_speeds};
use super::list::NIL;
use super::unit::status;
use super::{Data, State};
use crate::game::flingy::{direction_to, mflags};
use crate::todo_port;


impl State {
    /// 0x46c480: run the movement state machine for one unit.
    pub fn movement_step(&mut self, d: &Data, u: u32) {
        self.moved_flag = self.is_100th_frame;
        loop {
            let st = self.units[u as usize].movement_state;
            let again = match st {
                0 => self.mv_init(d, u),
                1 => {
                    if self.units[u as usize].status & status::NO_BRK_CODE_START != 0 {
                        return;
                    }
                    self.units[u as usize].movement_state = 0;
                    true
                }
                3 => self.mv_subunit(d, u),
                4 => self.mv_in_bunker(d, u),
                5 => self.mv_turn_only(d, u),
                6 => {
                    self.moved_flag = false;
                    return;
                }
                7 => self.mv_flyer(d, u),
                9 => self.mv_ground_follow(d, u),
                10 => self.mv_ground_idle(u),
                0xb => self.mv_path_done(u),
                0xc => self.mv_repath_check(d, u),
                0xd => self.mv_repath_wait(d, u),
                0xe => self.mv_no_path_move(d, u),
                0xf => {
                    self.units[u as usize].pathing_collision_interval = 10;
                    self.mv_stop_walking_anim(d, u);
                    self.units[u as usize].movement_state = 0x10;
                    return;
                }
                0x10 => self.mv_retry_path(d, u),
                0x11 => self.mv_request_path(d, u),
                0x12 => self.mv_wait_random(u),
                0x13 => self.mv_get_path_again(d, u),
                0x14 => self.mv_path_check(d, u),
                0x15 => self.mv_clear_repath(u),
                0x16 => {
                    let t = self.units[u as usize].fl.move_target;
                    let ok = self.path_request(d, u, t);
                    self.units[u as usize].movement_state = if ok { 0x19 } else { 0xf };
                    return;
                }
                0x17 => self.mv_repath_after_collision(d, u),
                0x18 => self.mv_repath_ignoring_units(d, u),
                0x19 => self.mv_follow_path(d, u),
                0x1a => self.mv_check_blockers(d, u),
                0x1b => {
                    self.units[u as usize].movement_state = 0x19;
                    true
                }
                0x1c => self.mv_collision_resolve(d, u),
                0x1d => self.mv_collision_wait(d, u),
                0x1e => self.mv_collision_wait_repath(d, u),
                0x1f => self.mv_collision_slow(u),
                0x20 => self.mv_collision_sidestep(d, u),
                0x21 => self.mv_collision_stop(u),
                0x22 => self.mv_collision_push(d, u),
                0x23 => self.mv_collision_turn(d, u),
                _ => return,
            };
            if !again {
                return;
            }
        }
    }

    /// 0x4ebc30: movement for a unit, then drag its subunit (turret) along.
    pub fn unit_movement(&mut self, d: &Data, u: u32) {
        let h0 = self.units[u as usize].fl.velocity_dir_prev;
        self.movement_step(d, u);
        if self.moved_flag {
            self.unit_reveal(d, u);
        }
        let sub = self.units[u as usize].subunit;
        let ut = self.units[u as usize].unit_type as usize;
        if self.units[u as usize].status & status::COMPLETED == 0 || sub == NIL || d.dat.units.flags[ut] & 0x10 != 0 {
            return;
        }
        let delta = self.units[u as usize].fl.velocity_dir_prev.wrapping_sub(h0);
        self.subunit_turn_with_parent(d, sub, delta);
        // 0x401df0(2): special overlay (LO kind 2) offset of the parent's main image.
        let ps = self.units[u as usize].sprite;
        let mi = self.sprites[ps as usize].main_image;
        let im = &self.images[mi as usize];
        let (mut ox, oy) = d.lo_offset(2, im.image_id, im.frame, 0).unwrap_or((0, 0));
        if im.flags & super::sprite::iflag::FLIPPED != 0 {
            ox = ox.wrapping_neg();
        }
        let ex = self.units[u as usize].fl.exact;
        let p = ((ex.0 >> 8) as i16, (ex.1 >> 8) as i16);
        let sf = &mut self.units[sub as usize].fl;
        sf.exact = ex;
        sf.pos = p;
        let ss = self.units[sub as usize].sprite;
        self.move_sprite(ss, p.0, p.1);
        let smi = self.sprites[ss as usize].main_image;
        let si = &mut self.images[smi as usize];
        if si.x != ox || si.y != oy {
            si.y = oy;
            si.flags |= super::sprite::iflag::REDRAW;
            si.x = ox;
        }
        self.iscript_unit = Some(sub);
        let moving = self.units[u as usize].fl.flags & mflags::MOVING != 0;
        let sub_walking = self.units[sub as usize].status & status::PASSIVE_REVEAL != 0;
        if !moving && sub_walking {
            self.units[sub as usize].status &= !status::PASSIVE_REVEAL;
            if self.units[u as usize].status & 0x20000 != 0 && self.units[sub as usize].fl.flags & mflags::STARTING == 0 {
                self.sprite_images_run_anim(d, ss, 0xc);
            }
        } else if moving && !sub_walking {
            self.units[sub as usize].status |= status::PASSIVE_REVEAL;
            if self.units[sub as usize].fl.flags & mflags::STARTING == 0 {
                self.sprite_images_run_anim(d, ss, 0xb);
            }
        }
        self.unit_movement(d, sub);
        self.iscript_unit = Some(u);
    }

    /// 0x4eb660: rotate a subunit along with its parent (ESI = subunit).
    fn subunit_turn_with_parent(&mut self, d: &Data, sub: u32, delta: u8) {
        let parent = self.units[sub as usize].subunit;
        let ph = self.units[parent as usize].fl.heading;
        let un = &mut self.units[sub as usize];
        if un.order_target_unit != NIL {
            un.status &= !0x2000000;
        } else if un.fl.heading == ph {
            un.status |= 0x2000000;
        }
        if un.status & 0x2000000 == 0 {
            let h = un.fl.velocity_dir_prev.wrapping_add(delta);
            un.fl.velocity_dir_prev = h;
            un.fl.heading = h;
        } else {
            self.subunit_face(d, sub, ph);
        }
        let un = &mut self.units[sub as usize];
        // Goliath (4) / Alan Schezar (0x12) turrets: stay within 0x20 of the body.
        if un.unit_type == 4 || un.unit_type == 0x12 {
            let h = un.fl.heading as i8;
            let diff = (ph as i8).wrapping_sub(h);
            if diff == -0x80 {
                un.fl.heading = h.wrapping_sub(0x60) as u8;
            } else if diff < -0x20 {
                un.fl.heading = h.wrapping_add(diff).wrapping_add(0x20) as u8;
            } else if diff > 0x20 {
                un.fl.heading = h.wrapping_add(diff).wrapping_sub(0x20) as u8;
            }
        }
    }

    /// 0x495f20: snap a unit's heading/velocity to `dir`; next target waypoint := position.
    pub(crate) fn subunit_face(&mut self, d: &Data, u: u32, dir: u8) {
        let f = &mut self.units[u as usize].fl;
        f.velocity_dir_prev = dir;
        f.heading = dir;
        f.velocity_dir = dir;
        let (dx, dy) = crate::game::flingy::direction_table()[dir as usize];
        f.velocity = ((dx * f.speed) >> 8, (dy * f.speed) >> 8);
        let s = self.units[u as usize].sprite;
        let sp = (self.sprites[s as usize].x, self.sprites[s as usize].y);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != sp {
            f.next_target_waypoint = sp;
        }
        self.sprite_images_set_direction(d, s, dir);
    }

    pub(super) fn unit_sprite_pos(&self, u: u32) -> (i16, i16) {
        let s = &self.sprites[self.units[u as usize].sprite as usize];
        (s.x, s.y)
    }

    /// Inlined `401dc0() == 0 && !(flags & BRAKING)`: still has somewhere to go.
    fn needs_move(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        un.fl.move_target != self.unit_sprite_pos(u) && un.fl.flags & mflags::BRAKING == 0
    }

    fn state(&mut self, u: u32, s: u8) {
        self.units[u as usize].movement_state = s;
    }

    /// 0x46bf00: stop: zero speed, clear move target, mark unmovable-at-target.
    pub fn unit_stop_moving(&mut self, d: &Data, u: u32) {
        set_speeds(&mut self.units[u as usize].fl, 0);
        self.unit_halt(d, u);
        let p = self.unit_sprite_pos(u);
        self.set_move_target_xy(d, u, p);
        self.units[u as usize].status |= status::UNMOVABLE;
        self.units[u as usize].order_flags &= !2; // +0x93 bit 1
    }

    /// 0x4eb290: stop at the current position (or the braking point), clamped to the map.
    pub fn unit_halt(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        if un.pathing_flags & 2 != 0 {
            un.pathing_flags |= 4;
        }
        self.flingy_halt(d, u);
        let mut t = self.units[u as usize].fl.move_target;
        let old = t;
        let ut = self.units[u as usize].unit_type;
        self.clamp_unit_pos(d, ut, &mut t);
        let un = &mut self.units[u as usize];
        un.recent_order_timer = 0xf;
        if t != old {
            un.fl.move_target = t;
            un.fl.next_waypoint = t;
        }
    }

    /// 0x4957a0: move target := position, or the point where braking ends if moving.
    fn flingy_halt(&mut self, d: &Data, u: u32) {
        let sp = self.unit_sprite_pos(u);
        let halt = flingy::halt_distance(&self.units[u as usize].fl, d);
        let un = &mut self.units[u as usize];
        let flags = un.fl.flags;
        if flags & mflags::BRAKING != 0 {
            return;
        }
        let f = &mut un.fl;
        f.move_target = f.pos;
        f.flags = flags & 0x8f;
        un.move_target_unit = NIL;
        let f = &mut un.fl;
        f.next_waypoint = f.move_target;
        if flags & mflags::MOVING != 0 && f.movement_type == 0 {
            let (dx, dy) = crate::game::flingy::direction_table()[f.velocity_dir_prev as usize];
            let x = (((dx * halt) >> 8) + f.exact.0) as u32 >> 8;
            let y = (f.exact.1 + ((dy * halt) >> 8)) as u32 >> 8;
            f.move_target = (x as i16, y as i16);
            f.next_waypoint = f.move_target;
            if f.move_target != sp {
                f.flags = (flags & 0x8f) | mflags::BRAKING;
            }
        }
    }

    /// 0x4eb820: set move target to (x, y) (EBX/EDI in the original).
    pub fn set_move_target_xy(&mut self, d: &Data, u: u32, p: (i16, i16)) -> bool {
        if self.units[u as usize].fl.move_target == p {
            return true;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x4e7420", "AI move-target veto");
        }
        let path = self.units[u as usize].path;
        if path != NIL {
            self.paths[path as usize].flags |= 1;
        }
        let mut p = p;
        let ut = self.units[u as usize].unit_type;
        self.clamp_unit_pos(d, ut, &mut p);
        let un = &mut self.units[u as usize];
        if p != un.fl.move_target {
            un.fl.move_target = p;
            un.move_target_unit = NIL;
            un.fl.flags = (un.fl.flags & !mflags::BRAKING) | mflags::TURNING;
            un.fl.next_waypoint = p;
        }
        un.recent_order_timer = 0xf;
        un.status &= !status::UNMOVABLE;
        let q = un.order_queue_head;
        if q != NIL && d.dat.orders.stop_moving[self.orders[q as usize].order as usize] == 0 {
            self.units[u as usize].fl.flags |= 0x20;
            return true;
        }
        self.units[u as usize].fl.flags &= !0x20;
        true
    }

    /// The path's collision unit if it is still alive (0x46bc30 / 0x46bcc0 checks).
    fn path_collision_unit(&self, u: u32) -> Option<u32> {
        let h = self.paths[self.units[u as usize].path as usize].collision_unit;
        if h == 0 {
            return None;
        }
        let i = (h & 0x7ff) as usize;
        let o = (i as u32).wrapping_sub(1);
        let ou = self.units.get(o as usize)?;
        if ou.sprite == NIL || (ou.order == 0 && ou.order_state == 1) || ou.unique_id as u32 != h >> 11 {
            return None;
        }
        Some(o)
    }

    /// 0x46a6b0: stop the walking animation if stuck for a few frames.
    fn mv_stop_walking_anim(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.fl.flags & mflags::MOVING != 0 && un.pathing_collision_interval > 2 && un.status & 0x3000 == 0 {
            self.units[u as usize].fl.flags &= !mflags::MOVING;
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xc);
        }
    }

    pub fn sprite_images_run_anim(&mut self, d: &Data, s: u32, anim: u8) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.image_run_anim(d, i, anim);
            i = next;
        }
    }

    /// 0x495ee0 on a unit.
    pub(super) fn predict(&mut self, d: &Data, u: u32) {
        let mut m = self.mv;
        flingy::predict(&mut self.units[u as usize].fl, d, &mut m);
        self.mv = m;
    }

    /// 0x46ad90: apply the predicted move, updating the unit finder.
    pub(super) fn apply_move_checked(&mut self, d: &Data, u: u32) -> bool {
        if !self.unit_apply_move(d, u) {
            return false;
        }
        let (old, new) = (self.mv.old_pos, self.mv.new_pos);
        self.finder_move(u, new.0 as i32 - old.0 as i32, new.1 as i32 - old.1 as i32);
        if old.0 / 32 != new.0 / 32 || old.1 / 32 != new.1 / 32 {
            self.moved_flag = true;
        }
        true
    }

    /// 0x46c320: pick the movement mode.
    fn mv_init(&mut self, d: &Data, u: u32) -> bool {
        let s = self.units[u as usize].sprite;
        let un = &mut self.units[u as usize];
        un.contour_bounds = [0; 4];
        let ground = s != NIL && self.sprites[s as usize].elevation < 0xc;
        let un = &mut self.units[u as usize];
        un.pathing_flags = (un.pathing_flags & 0xfc) | ground as u8;
        let ut = un.unit_type as usize;
        let subunit = d.dat.units.flags[ut] & 0x10 != 0;
        if !subunit && un.status & status::NO_BRK_CODE_START != 0 {
            un.movement_state = 1;
            return true;
        }
        if s == NIL || (un.order == 0 && un.order_state == 1) {
            un.movement_state = 2;
            return true;
        }
        let st = un.status;
        if st & status::IN_BUILDING != 0 {
            un.movement_state = 4;
            return true;
        }
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            if self.units[u as usize].fl.flags & mflags::MOVING != 0 || self.unit_at_move_target(u) == 0 {
                self.unit_stop_moving(d, u);
                self.predict(d, u);
                self.apply_move_checked(d, u);
            }
            self.state(u, 6);
            return true;
        }
        if st & status::BURROWED != 0 {
            self.state(u, 2);
            return true;
        }
        let movable = st & 0x20000 != 0;
        if movable || st & 0x10000 != 0 {
            let ns = if movable {
                if ground { 9 } else { 7 }
            } else if subunit {
                3
            } else {
                5
            };
            self.state(u, ns);
            return true;
        }
        if ground && (self.units[u as usize].fl.flags & mflags::MOVING != 0 || self.unit_at_move_target(u) == 0) {
            self.state(u, 0xe);
            return true;
        }
        self.state(u, 2);
        true
    }

    /// 0x46be80: subunit (turret) follows its parent's direction.
    fn mv_subunit(&mut self, d: &Data, u: u32) -> bool {
        self.moved_flag = false;
        let p = self.unit_sprite_pos(u);
        self.set_move_target_xy(d, u, p);
        let f = &mut self.units[u as usize].fl;
        let diff = f.desired_dir.wrapping_sub(f.heading) as i8;
        if diff > -0xb && diff < 0xb {
            f.flags &= !mflags::TURNING;
        }
        if self.units[u as usize].status & 0x2000000 != 0 {
            let mut m = self.mv;
            flingy::update_moving_flag(&mut self.units[u as usize].fl, &mut m);
            self.mv = m;
            return false;
        }
        let mut m = self.mv;
        flingy::update_desired_dir(&mut self.units[u as usize].fl);
        flingy::update_moving_flag(&mut self.units[u as usize].fl, &mut m);
        self.mv = m;
        self.unit_turn_in_place(d, u);
        false
    }

    /// 0x46acf0: in a bunker: turn only.
    fn mv_in_bunker(&mut self, d: &Data, u: u32) -> bool {
        self.moved_flag = false;
        self.mv_turn_only(d, u)
    }

    /// 0x46ad40: turn only.
    fn mv_turn_only(&mut self, d: &Data, u: u32) -> bool {
        let mut m = self.mv;
        flingy::update_desired_dir(&mut self.units[u as usize].fl);
        flingy::update_moving_flag(&mut self.units[u as usize].fl, &mut m);
        self.mv = m;
        self.unit_turn_in_place(d, u);
        false
    }

    /// 0x46b400: flyers: clamp target, predict, air repulse, clamp, apply.
    fn mv_flyer(&mut self, d: &Data, u: u32) -> bool {
        self.clamp_move_target(d, u);
        self.predict(d, u);
        let repulsed = self.air_repulse(d, u);
        let ut = self.units[u as usize].unit_type as usize;
        let dim = d.dat.units.dimensions[ut];
        let (l, t, r, b) = (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32);
        let (w, h) = (self.map_px.0 as i32, self.map_px.1 as i32);
        let m = &mut self.mv;
        let mut x = m.new_pos.0 as i32;
        let mut y = m.new_pos.1 as i32;
        if x - l < 0 {
            x -= x - l;
        }
        if w <= r + x {
            x += w - (r + x) - 1;
        }
        if y - t < 0 {
            y -= y - t;
        }
        if h <= b + y {
            y += h - (b + y) - 1;
        }
        m.new_pos = (x as i16, y as i16);
        self.apply_move_checked(d, u);
        self.air_repulse_finish(d, u, repulsed);
        false
    }

    /// 0x46a740: clamp the move target into the map; UNMOVABLE when it was clamped.
    fn clamp_move_target(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].fl.move_target;
        if t == self.unit_sprite_pos(u) {
            return;
        }
        let ut = self.units[u as usize].unit_type as usize;
        let dim = d.dat.units.dimensions[ut];
        let (l, tp, r, b) = (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32);
        let maxx = self.map_px.0 as i32 - r - 1;
        let maxy = self.map_px.1 as i32 - b - 1;
        let mut x = (t.0 as i32).max(l);
        let mut y = if tp <= t.1 as i32 { t.1 as i32 } else { tp };
        x = x.min(maxx);
        y = y.min(maxy);
        let un = &mut self.units[u as usize];
        if (x, y) != (t.0 as i32, t.1 as i32) {
            un.fl.move_target = (x as i16, y as i16);
            un.move_target_unit = NIL;
            un.fl.flags = (un.fl.flags & !mflags::BRAKING) | mflags::TURNING;
            un.fl.next_waypoint = un.fl.move_target;
            // Only a target that had to be moved marks the unit (the other path to
            // the flag in 0x46a740 is unreachable).
            un.status |= status::UNMOVABLE;
        }
    }

    /// 0x453420: air unit repulsion. Adjusts the predicted position; returns whether applied.
    fn air_repulse(&mut self, d: &Data, u: u32) -> bool {
        if !self.repulse_eligible(d, u) || self.units[u as usize].status & 0x3000 != 0 {
            return false;
        }
        let un = &self.units[u as usize];
        let n = self.repulse[un.repulse_x as usize + un.repulse_y as usize * 0xab];
        if n < 2 {
            self.units[u as usize].repulse_unknown &= 0xf8;
            return false;
        }
        let r = self.units[u as usize].repulse_unknown;
        if r & 7 == 0 {
            self.units[u as usize].repulse_unknown = r | 7;
            self.repulse_new_angle(d, u);
        } else {
            self.units[u as usize].repulse_unknown = r - 1;
        }
        let (dx, dy) = crate::game::flingy::direction_table()[self.units[u as usize].repulse_angle as usize];
        let (mut x, mut y) = (dx / 2, dy / 2);
        if self.unit_at_move_target(u) != 0 {
            let k = ((n >> 2) + 1) as i32;
            x = k * x / 2;
            y = k * y / 2;
        }
        const JITTER: [(i32, i32); 16] = [
            (-5, -5), (-5, 5), (5, -5), (5, 5), (5, -5), (5, 5), (-5, -5), (-5, 5),
            (-5, 5), (5, -5), (-5, -5), (5, 5), (-5, 5), (5, -5), (5, 5), (-5, -5),
        ];
        let (jx, jy) = JITTER[(self.units[u as usize].repulse_unknown >> 4) as usize];
        if jx * 4 < x || (x < 0 && x >= jx * -4) {
            x -= jx;
        } else if x > 0 || x < jx * -4 {
            x += jx;
        }
        if jy * 4 < y || (y < 0 && y >= jy * -4) {
            y -= jy;
        } else if y > 0 || y < jy * -4 {
            y += jy;
        }
        let m = &mut self.mv;
        m.new_exact.0 += x;
        m.new_exact.1 += y;
        m.new_pos = ((m.new_exact.0 as u32 >> 8) as i16, (m.new_exact.1 as u32 >> 8) as i16);
        true
    }

    /// 0x4531f0: pick a new repulse angle, steered towards the move target and away from map edges.
    fn repulse_new_angle(&mut self, d: &Data, u: u32) {
        let (sx, sy) = { let p = self.unit_sprite_pos(u); (p.0 as i32, p.1 as i32) };
        let un = &self.units[u as usize];
        let mut b = un.repulse_angle;
        let mt = un.fl.move_target;
        if (sx, sy) != (mt.0 as i32, mt.1 as i32) {
            let c = direction_to(sx, sy, mt.0 as i32, mt.1 as i32);
            // 0x68c348 (filled by 0x4531a0)
            let k = b.wrapping_sub(c);
            let t = if k < 0x80 { (k & 0x1f) + 0x20 } else { (k & 0x1f).wrapping_sub(0x40) };
            b = t.wrapping_add(c);
        }
        let dim = d.dat.units.dimensions[un.unit_type as usize];
        let (l, t, r, bt) = (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32);
        let (w, h) = (self.map_px.0 as i32, self.map_px.1 as i32);
        let mut ru = un.repulse_unknown;
        if sx < l + 0x20 {
            if (b as i8) < 0 {
                b = b.wrapping_neg();
            }
            ru &= 0xf;
        } else if w - r - 0x21 < sx {
            if (b as i8) >= 0 {
                b = b.wrapping_neg();
            }
            ru &= 0xf;
        }
        let edge = if sy < t + 0x20 { Some(true) } else if h - bt - 0x21 < sy { Some(false) } else { None };
        if let Some(top) = edge {
            let mut c = b.wrapping_add(0x40);
            if ((c as i8) >= 0) == top {
                c = c.wrapping_neg();
            }
            b = c.wrapping_sub(0x40);
            ru &= 0xf;
        }
        let un = &mut self.units[u as usize];
        un.repulse_unknown = ru;
        un.repulse_angle = b;
    }

    /// 0x4535a0: update repulse grid cell; snap target if close.
    fn air_repulse_finish(&mut self, d: &Data, u: u32, repulsed: bool) {
        if !self.repulse_eligible(d, u) {
            return;
        }
        let (sx, sy) = self.unit_sprite_pos(u);
        let un = &self.units[u as usize];
        if un.repulse_x as i32 != sx as i32 / 0x30 || un.repulse_y as i32 != sy as i32 / 0x30 {
            self.repulse_remove(d, u);
            self.repulse_init(d, u);
        }
        let np = self.mv.new_pos;
        let f = &mut self.units[u as usize].fl;
        if repulsed && (f.move_target.0 as i32 - np.0 as i32).abs() < 0x18 && (f.move_target.1 as i32 - np.1 as i32).abs() < 0x18 {
            f.move_target = np;
            f.next_waypoint = np;
        }
    }

    /// 0x46b790: ground unit, checks whether it needs a path.
    fn mv_ground_follow(&mut self, d: &Data, u: u32) -> bool {
        let at = self.units[u as usize].fl.move_target == self.unit_sprite_pos(u);
        let un = &mut self.units[u as usize];
        if at {
            un.pathing_collision_interval = 0;
        } else if un.pathing_collision_interval < 3 {
            if un.pathing_collision_interval != 0 {
                un.pathing_collision_interval -= 1;
            }
        } else {
            un.pathing_collision_interval = 2;
        }
        let flags = self.units[u as usize].fl.flags;
        let mut idle = true;
        let need_step = flags & mflags::BRAKING != 0
            || (at
                && (flags & mflags::MOVING != 0 || {
                    let f = &self.units[u as usize].fl;
                    let sp = self.unit_sprite_pos(u);
                    f.next_target_waypoint != f.pos && {
                        let dir = direction_to(sp.0 as i32, sp.1 as i32, f.next_target_waypoint.0 as i32, f.next_target_waypoint.1 as i32);
                        dir != f.heading || dir != f.velocity_dir_prev
                    }
                }));
        if need_step {
            idle = false;
            self.predict(d, u);
            self.collision_step(d, u);
        }
        let st = self.units[u as usize].status;
        if st & status::IGNORE_TILE_COLLISION != 0 && st & status::IS_NORMAL != 0 {
            self.state(u, 0xc);
            return false;
        }
        if self.needs_move(u) {
            self.state(u, 0x11);
            return true;
        }
        if idle {
            let sp = self.unit_sprite_pos(u);
            let f = &mut self.units[u as usize].fl;
            set_speeds(f, 0);
            if f.next_target_waypoint != sp {
                f.next_target_waypoint = sp;
            }
            self.state(u, 10);
        }
        false
    }

    /// 0x46a8d0: idle ground unit; wake when there is somewhere to go.
    fn mv_ground_idle(&mut self, u: u32) -> bool {
        let un = &self.units[u as usize];
        let st = un.status;
        let still = (st & status::IGNORE_TILE_COLLISION == 0 || st & status::IS_NORMAL == 0)
            && un.fl.move_target == self.unit_sprite_pos(u)
            && un.fl.next_target_waypoint == un.fl.pos;
        if still {
            return false;
        }
        self.state(u, 9);
        true
    }

    /// 0x46a640: path finished.
    fn mv_path_done(&mut self, u: u32) -> bool {
        self.path_delete(u);
        let un = &mut self.units[u as usize];
        un.fl.next_waypoint = un.fl.move_target;
        let ns = if un.status & status::IS_NORMAL != 0 && un.fl.flags & mflags::BRAKING == 0 { 0xc } else { 9 };
        self.state(u, ns);
        true
    }

    /// 0x46bdf0: terrain-only unit: check whether still stuck in terrain.
    fn mv_repath_check(&mut self, d: &Data, u: u32) -> bool {
        self.units[u as usize].status &= !status::IGNORE_TILE_COLLISION;
        if self.unstack_unit(d, u) {
            self.units[u as usize].pathing_flags |= 2;
            self.state(u, 0xd);
            return false;
        }
        self.units[u as usize].pathing_flags &= 0xf9;
        let ns = if self.needs_move(u) { 0x16 } else { 9 };
        self.state(u, ns);
        true
    }

    /// 0x46b360: escaping terrain.
    fn mv_repath_wait(&mut self, d: &Data, u: u32) -> bool {
        let path = self.units[u as usize].path;
        if path == NIL {
            self.state(u, 0xc);
            return true;
        }
        if self.units[u as usize].pathing_flags & 4 != 0 {
            let un = &mut self.units[u as usize];
            un.pathing_flags &= 0xfb;
            un.status |= status::IGNORE_TILE_COLLISION;
            self.path_delete(u);
            self.state(u, 9);
            return true;
        }
        let p = self.paths[path as usize];
        if p.flags & 1 != 0 || p.next_waypoint != self.units[u as usize].fl.next_waypoint {
            self.units[u as usize].fl.next_waypoint = p.next_waypoint;
        }
        if !self.terrain_escape_step(d, u) {
            return false;
        }
        if self.units[u as usize].path != NIL {
            self.path_delete(u);
        }
        self.update_unit_overlaps(d, u);
        self.units[u as usize].status |= status::IGNORE_TILE_COLLISION;
        self.state(u, 0xc);
        false
    }

    /// 0x46b700: ground non-unit flingy movement without pathing.
    fn mv_no_path_move(&mut self, d: &Data, u: u32) -> bool {
        if self.units[u as usize].fl.move_target == self.unit_sprite_pos(u)
            && self.units[u as usize].fl.flags & mflags::MOVING == 0
        {
            self.state(u, 2);
            return true;
        }
        self.predict(d, u);
        self.collision_step(d, u);
        false
    }

    /// 0x46b500: check unit collision for the predicted step; apply or stop.
    fn collision_step(&mut self, d: &Data, u: u32) -> bool {
        let blocked = (self.units[u as usize].status & status::IS_NORMAL != 0 && self.unit_blocking_move(u).is_some())
            || self.predicted_move_blocked(d, u);
        if !blocked {
            return self.apply_move_checked(d, u);
        }
        self.apply_without_moving(d, u);
        // 0x430f10(1, move target unit) is called here but its result is unused.
        set_speeds(&mut self.units[u as usize].fl, 0);
        let sp = self.unit_sprite_pos(u);
        let un = &mut self.units[u as usize];
        if sp != un.fl.move_target {
            un.fl.move_target = sp;
            un.move_target_unit = NIL;
            un.fl.flags = (un.fl.flags & !mflags::BRAKING) | mflags::TURNING;
            un.fl.next_waypoint = sp;
        }
        self.unit_halt(d, u);
        false
    }

    /// 0x495f90: apply flags/heading of the prediction but not the position.
    fn apply_without_moving(&mut self, d: &Data, u: u32) {
        let m = self.mv;
        let f = &mut self.units[u as usize].fl;
        f.flags = m.new_flags;
        f.velocity_dir_prev = f.velocity_dir;
        f.next_speed = 0;
        flingy::turn_heading(f);
        let (s, h) = (self.units[u as usize].sprite, self.units[u as usize].fl.heading);
        self.sprite_images_set_direction(d, s, h);
        if m.stopping {
            if self.sprites[s as usize].flags & 0x80 == 0 {
                self.sprite_images_run_anim(d, s, 0xc);
            }
        } else if m.starting {
            self.sprite_images_run_anim(d, s, 0xb);
        }
    }

    /// 0x46c2d0: retry the path every frame up to 20 times.
    fn mv_retry_path(&mut self, d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, false) {
            return true;
        }
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval < 0x14 {
            un.pathing_collision_interval += 1;
            return false;
        }
        let t = un.fl.move_target;
        if !self.path_request(d, u, t) {
            self.unit_stop_moving(d, u);
            self.state(u, 0xb);
            return false;
        }
        self.state(u, 0x19);
        true
    }

    /// 0x46a940: common transitions: arrived -> 0xb, path invalid -> 0x15/0x16/0x13.
    fn mv_check_path_state(&mut self, u: u32, need_path: bool) -> bool {
        if !self.needs_move(u) {
            self.state(u, 0xb);
            return true;
        }
        let st = self.units[u as usize].status;
        if st & status::IGNORE_TILE_COLLISION != 0 && st & status::IS_NORMAL != 0 {
            self.path_delete(u);
            self.state(u, 0xc);
            return true;
        }
        let path = self.units[u as usize].path;
        let mut ns = if need_path && path == NIL {
            0x16
        } else if path != NIL && self.paths[path as usize].flags & 1 != 0 {
            0x15
        } else {
            return false;
        };
        let cur = self.units[u as usize].movement_state;
        if cur > 0x1b && cur < 0x24 {
            ns = 0x13;
        }
        self.state(u, ns);
        true
    }

    /// 0x46bd90: request a path.
    fn mv_request_path(&mut self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].fl.move_target;
        if !self.path_request(d, u, t) {
            self.state(u, 0xf);
            return false;
        }
        let un = &mut self.units[u as usize];
        if un.order_flags & 2 != 0 {
            un.order_flags &= !2;
            if un.fl.movement_type == 2 {
                let w = self.rng.rand_between(0x33, 0, 2) as u8;
                let p = self.units[u as usize].path;
                self.paths[p as usize].wait = w;
                self.state(u, 0x12);
                return true;
            }
        }
        self.state(u, 0x19);
        true
    }

    /// 0x46aae0: wait a random number of frames before following the path.
    fn mv_wait_random(&mut self, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let p = self.units[u as usize].path;
        if self.paths[p as usize].wait != 0 {
            self.paths[p as usize].wait -= 1;
            return false;
        }
        self.state(u, 0x19);
        true
    }

    /// Resolve a unit handle (0x47b1d0 inverse, inlined as `(h & 0x7ff) * 0x150 + 0x59cb58`).
    pub fn unit_from_handle(&self, h: u32) -> Option<u32> {
        if h == 0 {
            return None;
        }
        let idx = (h & 0x7ff) - 1;
        let un = self.units.get(idx as usize)?;
        if un.sprite == NIL || (un.order == 0 && un.order_state == 1) || un.unique_id as u32 != h >> 11 {
            return None;
        }
        Some(idx)
    }

    /// 0x47b1d0: unit -> handle.
    pub fn unit_handle(&self, u: u32) -> u32 {
        (self.units[u as usize].unique_id as u32) << 11 | (u + 1)
    }

    /// 0x46bcc0.
    fn mv_get_path_again(&mut self, d: &Data, u: u32) -> bool {
        let p = self.units[u as usize].path;
        if p != NIL {
            self.path_avoid_unit = self.path_collision_unit(u);
            self.path_delete(u);
        }
        if self.needs_move(u) {
            let t = self.units[u as usize].fl.move_target;
            let ok = self.path_request(d, u, t);
            self.path_avoid_unit = None;
            if !ok {
                self.state(u, 0xf);
                return true;
            }
            self.state(u, 0x14);
            return false;
        }
        self.state(u, 0xb);
        true
    }

    /// 0x46ac60.
    fn mv_path_check(&mut self, d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        if !self.needs_move(u) {
            self.state(u, 0xb);
            return true;
        }
        if !self.facing_next_waypoint(u) {
            self.predict(d, u);
            self.unit_turn_in_place(d, u);
            return false;
        }
        let p = self.units[u as usize].path;
        let ns = if self.paths[p as usize].flags & 1 != 0 { 0x15 } else { 0x19 };
        self.state(u, ns);
        true
    }

    /// 0x46aab0.
    fn mv_clear_repath(&mut self, u: u32) -> bool {
        let p = self.units[u as usize].path;
        self.paths[p as usize].flags &= 0xfe;
        if !self.mv_check_path_state(u, true) {
            self.path_delete(u);
            self.state(u, 0x17);
        }
        true
    }

    /// 0x46bc30.
    fn mv_repath_after_collision(&mut self, d: &Data, u: u32) -> bool {
        if self.units[u as usize].path != NIL {
            self.path_avoid_unit = self.path_collision_unit(u);
            self.path_delete(u);
        }
        let t = self.units[u as usize].fl.move_target;
        let ok = self.path_request(d, u, t);
        self.path_avoid_unit = None;
        self.state(u, if ok { 0x19 } else { 0xf });
        true
    }

    /// 0x46bbc0: repath ignoring units (0x6bee80 = 1).
    fn mv_repath_ignoring_units(&mut self, d: &Data, u: u32) -> bool {
        self.path_delete(u);
        let (t, tu) = (self.units[u as usize].fl.move_target, self.units[u as usize].move_target_unit);
        self.path_all_units = true;
        let ok = self.path_request(d, u, t);
        self.path_all_units = false;
        if ok && self.units[u as usize].status & status::UNMOVABLE == 0 {
            self.state(u, 0x14);
            return true;
        }
        let un = &mut self.units[u as usize];
        un.fl.move_target = t;
        un.move_target_unit = tu;
        self.state(u, 0x13);
        false
    }

    /// 0x46b950: follow the path, detecting unit collisions.
    fn mv_follow_path(&mut self, d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let t = self.units[u as usize].fl.move_target;
        if !self.path_request(d, u, t) {
            self.state(u, 0x16);
            return true;
        }
        let mut speed = self.units[u as usize].fl.next_speed;
        self.predict(d, u);
        if self.units[u as usize].fl.movement_type == 0 {
            speed = self.units[u as usize].fl.speed;
        }
        let blocker = if self.units[u as usize].status & status::IS_NORMAL != 0 { self.unit_blocking_move(u) } else { None };
        let blocked = self.predicted_move_blocked(d, u);
        match blocker {
            None => {
                if blocked {
                    let p = self.units[u as usize].path;
                    self.paths[p as usize].speed = speed;
                    self.state(u, 0x22);
                    return true;
                }
            }
            Some(b) => {
                // Unit in the way: retry half and quarter steps before stopping.
                let mut stop = true;
                let f = &self.units[u as usize].fl;
                if !blocked && (f.velocity.0 >= 0x20 || f.velocity.1 >= 0x20) && self.mv.moved == f.speed {
                    let (vel, exact) = (f.velocity, f.exact);
                    let saved = (self.mv.new_pos, self.mv.new_exact);
                    for div in [2, 4] {
                        let ne = (vel.0 / div + exact.0, vel.1 / div + exact.1);
                        self.mv.new_exact = ne;
                        self.mv.new_pos = ((ne.0 >> 8) as i16, (ne.1 >> 8) as i16);
                        // 0x4f2240
                        let still = self.units[u as usize].status & status::IS_NORMAL != 0 && self.unit_blocking_move(u).is_some();
                        if !still {
                            stop = false;
                            break;
                        }
                    }
                    if stop {
                        (self.mv.new_pos, self.mv.new_exact) = saved;
                    }
                }
                if stop {
                    let h = self.unit_handle(b);
                    let p = self.units[u as usize].path;
                    self.paths[p as usize].collision_unit = h;
                    self.paths[p as usize].speed = speed;
                    self.state(u, 0x1c);
                    return true;
                }
            }
        }
        if self.mv.new_pos != self.unit_sprite_pos(u) {
            let un = &mut self.units[u as usize];
            if un.pathing_collision_interval < 3 {
                if un.pathing_collision_interval != 0 {
                    un.pathing_collision_interval -= 1;
                }
            } else {
                un.pathing_collision_interval = 2;
            }
        }
        self.apply_move_checked(d, u);
        if self.unit_at_move_target(u) != 0 {
            self.state(u, 0xb);
            return false;
        }
        if self.path_wait_expired(u) {
            self.set_path_wait(u, 0x1e);
            self.state(u, 0x1a);
        }
        false
    }

    /// 0x46ab10: count units blocking the path's next region; 0x1b if any, else 0x19.
    fn mv_check_blockers(&mut self, d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let mut blockers = 0;
        let (start, count) = self.moving_units_near(u, 0x20);
        if count != 0 {
            let me = self.predict_positions(d, u, 1, 0x1e)[0];
            let others: Vec<_> = self.search.results(start);
            let ahead: Vec<_> = others.iter().map(|&c| self.predict_positions(d, c, 1, 0x1e)[0]).collect();
            let ut = self.units[u as usize].unit_type;
            for (&c, &(x, y)) in others.iter().zip(&ahead) {
                // 0x402b10: c's box at its predicted spot, grown by u's box.
                let ct = self.units[c as usize].unit_type;
                let [cl, cu, cr, cd] = self.unit_box_at(d, ct, x, y);
                // Own box at (0, 0) is [-left, -up, right, down].
                let [nl, nu, r, dn] = self.unit_box_at(d, ut, 0, 0);
                let b = [cl.wrapping_sub(1).wrapping_sub(r), cu.wrapping_sub(1).wrapping_sub(dn), cr.wrapping_sub(nl).wrapping_add(1), cd.wrapping_sub(nu).wrapping_add(1)];
                if b[0] <= me.0 && me.0 <= b[2] && b[1] <= me.1 && me.1 <= b[3] {
                    blockers += 1;
                }
            }
        }
        self.search.pop();
        self.state(u, if blockers > 0 { 0x1b } else { 0x19 });
        true
    }

    /// 0x46bf60: decide how to resolve a collision.
    fn mv_collision_resolve(&mut self, d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        let p = self.units[u as usize].path;
        let h = self.paths[p as usize].collision_unit;
        let other = (h != 0)
            .then(|| (h & 0x7ff) as usize)
            .filter(|&i| i >= 1 && i <= self.units.len())
            .map(|i| (i - 1) as u32)
            .filter(|&o| {
                let ou = &self.units[o as usize];
                ou.sprite != NIL
                    && !(ou.order == 0 && ou.order_state == 1)
                    && ou.unique_id as u32 == h >> 11
                    && self.sprites[ou.sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
                    && ou.pathing_flags & 1 != 0
                    && ou.status & 0x20_0000 == 0
                    && o != u
                    && self.units_collide(o, u)
            });
        let Some(o) = other else {
            self.state(u, 0x19);
            return false;
        };
        let mtu = self.units[u as usize].move_target_unit;
        let action = if mtu != NIL && self.box_distance_within(d, u, 0, mtu) {
            1
        } else {
            let ut = self.units[u as usize].unit_type;
            let mt = self.units[u as usize].fl.move_target;
            let nw = self.units[u as usize].fl.next_waypoint;
            let ot = self.units[o as usize].fl.move_target;
            let in_box = |st: &State, p: (i16, i16), center: (i16, i16)| st.point_in_unit_box(d, o, center, ut, p);
            let os = self.units[o as usize].sprite as usize;
            let opos = (self.sprites[os].x, self.sprites[os].y);
            let facing_away = |st: &State| {
                let us = &st.sprites[st.units[u as usize].sprite as usize];
                let dir = crate::game::flingy::direction_to(us.x as i32, us.y as i32, opos.0 as i32, opos.1 as i32) as u32;
                let vd = st.units[o as usize].fl.velocity_dir_prev;
                let q: u32 = if vd < 0x20 { 0 } else if vd < 0x60 { 0x40 } else if vd < 0xa0 { 0x80 } else if vd < 0xe0 { 0xc0 } else { 0 };
                let mut diff = q.wrapping_sub(dir) & 0xff;
                if diff > 0x80 {
                    diff = 0x100 - diff;
                }
                diff > 0x40
            };
            if in_box(self, mt, opos) {
                if self.units[o as usize].pathing_collision_interval > 0x1d || in_box(self, mt, ot) {
                    (mtu != NIL) as u32 + 2
                } else if facing_away(self) {
                    4
                } else {
                    6
                }
            } else if in_box(self, nw, opos) {
                if self.units[o as usize].pathing_collision_interval > 0x1d || in_box(self, nw, ot) {
                    3
                } else if facing_away(self) {
                    4
                } else {
                    6
                }
            } else {
                let moving = self.units[o as usize].fl.flags & 2 != 0;
                let vd = self.units[o as usize].fl.velocity_dir_prev;
                let side_says_wait = moving
                    && match self.blocker_side(d, o, u) {
                        0 => vd <= 0x40 || vd >= 0xc0,
                        1 => vd <= 0x80,
                        2 => (0x40..=0xc0).contains(&vd),
                        _ => vd == 0 || vd >= 0x80,
                    };
                if side_says_wait {
                    6
                } else {
                    let ms = if self.units[o as usize].pathing_collision_interval > 2 { 0x1d } else { self.units[o as usize].movement_state };
                    if self.unit_at_move_target(o) == 0 && moving {
                        if ms < 0x19 {
                            4
                        } else if ms <= 0x1a {
                            6
                        } else if ms == 0x1d {
                            if self.sidestep_dir(d, o, u) >= 0 { 7 } else { 5 }
                        } else {
                            4
                        }
                    } else if self.sidestep_dir(d, o, u) >= 0 {
                        7
                    } else {
                        5
                    }
                }
            }
        };
        let created = self.paths[p as usize].creation_frame;
        if self.frame.wrapping_sub(created) < 7 && (3..=5).contains(&action) {
            self.mv_stop_walking_anim(d, u);
            return false;
        }
        match action {
            1 => {
                self.unit_stop_moving(d, u);
                self.units[u as usize].status &= !status::UNMOVABLE;
                self.state(u, 0xb);
            }
            2 => {
                self.unit_stop_moving(d, u);
                self.state(u, 0xb);
            }
            3 => self.state(u, 0x17),
            4 => self.state(u, 0x18),
            5 => {
                self.state(u, 0x13);
                set_speeds(&mut self.units[u as usize].fl, 0);
            }
            6 => self.state(u, 0x1d),
            7 => {
                let h = self.unit_handle(o);
                self.paths[p as usize].collision_unit = h;
                let dir = self.sidestep_dir(d, o, u);
                self.paths[p as usize].direction = dir as u8;
                self.state(u, 0x1f);
            }
            _ => self.state(u, 0x1e),
        }
        false
    }


    /// 0x46b220.
    fn mv_collision_wait(&mut self, d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        self.collision_wait_common(d, u, false)
    }

    /// 0x46b5b0.
    fn mv_collision_wait_repath(&mut self, d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        self.collision_wait_common(d, u, true)
    }

    fn collision_wait_common(&mut self, d: &Data, u: u32, repath: bool) -> bool {
        let p = self.units[u as usize].path;
        let sp = self.paths[p as usize].speed;
        set_speeds(&mut self.units[u as usize].fl, sp);
        self.predict(d, u);
        let blocker = self.unit_blocking_move(u);
        let Some(b) = blocker else {
            if self.predicted_move_blocked(d, u) {
                self.state(u, 0x17);
                return false;
            }
            self.apply_move_checked(d, u);
            self.state(u, 0x19);
            return false;
        };
        self.mv_stop_walking_anim(d, u);
        if !repath {
            set_speeds(&mut self.units[u as usize].fl, 0);
            if !self.facing_next_waypoint(u) {
                self.unit_turn_in_place(d, u);
            }
            if self.units[u as usize].pathing_collision_interval > 0x18 {
                if self.rng.rand(0x33) < 0x3fff {
                    self.state(u, 0x21);
                    return false;
                }
                set_speeds(&mut self.units[u as usize].fl, 0);
                self.state(u, 0x18);
            }
            return false;
        }
        if !self.facing_next_waypoint(u) {
            self.unit_turn_in_place(d, u);
            return false;
        }
        self.path_delete(u);
        let dir = self.sidestep_dir(d, b, u);
        if dir < 0 {
            self.state(u, 0x18);
            return false;
        }
        // 0x46b6d1: 0x42f740 always nulls unit +0x100, so 1.16.1 stores the blocker
        // handle and direction at address 0x10 / 0x1a (access violation).
        todo_port!("0x46b6d8", "1.16.1 writes the sidestep target through a null path");
        self.state(u, 0x1f);
        false
    }

    /// 0x46aa20.
    fn mv_collision_slow(&mut self, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let p = self.units[u as usize].path;
        if self.units[u as usize].fl.movement_type == 0 {
            let s = self.paths[p as usize].speed;
            if s < 0x200 {
                if s > 0x100 {
                    self.paths[p as usize].speed = 0x100;
                }
            } else {
                self.paths[p as usize].speed = s / 2;
            }
        }
        let s = self.paths[p as usize].speed;
        let f = &mut self.units[u as usize].fl;
        f.speed = s;
        f.next_speed = s;
        f.velocity_dir = f.velocity_dir_prev;
        let (dx, dy) = crate::game::flingy::direction_table()[f.velocity_dir as usize];
        f.velocity = ((dx * f.speed) >> 8, (dy * f.speed) >> 8);
        self.state(u, 0x20);
        true
    }

    /// 0x46b000.
    /// 0x46b000: slide sideways (path +0x1a direction) past the collision unit.
    fn mv_collision_sidestep(&mut self, d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let p = self.units[u as usize].path;
        let avoid = {
            let h = self.paths[p as usize].collision_unit;
            let o = (h & 0x7ff).wrapping_sub(1);
            (h != 0)
                .then_some(o)
                .filter(|&o| {
                    let ou = &self.units[o as usize];
                    ou.sprite != NIL && !(ou.order == 0 && ou.order_state == 1) && ou.unique_id as u32 == h >> 11
                })
        };
        let side_dir = self.paths[p as usize].direction;
        {
            let f = &mut self.units[u as usize].fl;
            f.flags |= 1;
            f.desired_dir = side_dir;
            flingy::turn_velocity(f);
            f.velocity_dir_prev = f.velocity_dir;
            flingy::turn_heading(f);
        }
        let s = self.units[u as usize].sprite;
        let h = self.units[u as usize].fl.heading;
        self.sprite_images_set_direction(d, s, h);
        if self.units[u as usize].fl.movement_type == 2 {
            let f = &mut self.units[u as usize].fl;
            f.speed = f.top_speed;
        } else {
            flingy::update_speed(&mut self.units[u as usize].fl, d);
        }
        let dir = self.units[u as usize].fl.velocity_dir_prev;
        let speed = self.units[u as usize].fl.speed;
        self.units[u as usize].fl.next_speed = speed;
        let mut next_state = None;
        let mut last = None;
        if speed > 0 {
            let mut left = speed;
            loop {
                let step = left.min(0x100);
                left -= 0x100;
                self.units[u as usize].fl.flags &= 0xfe;
                let mut m = self.mv;
                flingy::predict_with_dir_speed(&mut self.units[u as usize].fl, &mut m, side_dir, step);
                self.mv = m;
                if self.unit_blocking_move(u).is_some() || self.predicted_move_blocked(d, u) {
                    next_state = Some(0x21);
                    break;
                }
                self.apply_move_checked(d, u);
                self.units[u as usize].fl.velocity_dir_prev = dir;
                flingy::update_desired_dir(&mut self.units[u as usize].fl);
                let vd = self.units[u as usize].fl.velocity_dir;
                let mut m = self.mv;
                flingy::predict_with_dir_speed(&mut self.units[u as usize].fl, &mut m, vd, speed);
                self.mv = m;
                last = self.unit_blocking_move(u);
                if last != avoid || left <= 0 {
                    break;
                }
            }
        }
        let f = &mut self.units[u as usize].fl;
        f.velocity_dir = dir;
        f.velocity_dir_prev = dir;
        f.speed = speed;
        f.next_speed = speed;
        let (dx, dy) = crate::game::flingy::direction_table()[dir as usize];
        f.velocity = ((dx * speed) >> 8, (dy * speed) >> 8);
        if let Some(ns) = next_state {
            self.state(u, ns);
            return false;
        }
        if last.is_none() {
            self.units[u as usize].fl.flags |= 1;
            let p = self.units[u as usize].path;
            if p != NIL && self.frame.wrapping_sub(self.paths[p as usize].creation_frame) < 0x96 {
                self.state(u, 0x19);
                return false;
            }
            self.state(u, 0x13);
        }
        false
    }

    /// 0x46a850.
    fn mv_collision_stop(&mut self, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.needs_move(u) {
            set_speeds(&mut self.units[u as usize].fl, 0);
            self.path_delete(u);
            self.state(u, 0x13);
            return true;
        }
        self.state(u, 0xb);
        false
    }

    /// 0x46af00.
    fn mv_collision_push(&mut self, d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let p = self.units[u as usize].path;
        let sp = self.paths[p as usize].speed;
        set_speeds(&mut self.units[u as usize].fl, sp);
        self.predict(d, u);
        if !self.predicted_move_blocked(d, u) && !self.unit_blocking_move(u).is_some() {
            self.apply_move_checked(d, u);
            self.units[u as usize].fl.flags |= mflags::TURNING;
            self.state(u, 0x19);
            return false;
        }
        self.mv_stop_walking_anim(d, u);
        if !self.facing_next_waypoint(u) {
            self.unit_turn_in_place(d, u);
            return false;
        }
        let dir = self.slide_dir(d, u);
        let p = self.units[u as usize].path;
        self.paths[p as usize].direction = dir;
        self.state(u, 0x23);
        false
    }

    /// 0x46ae20.
    /// 0x46ae20: step in the path's avoidance direction, then turn back toward the waypoint.
    fn mv_collision_turn(&mut self, d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        let p = self.units[u as usize].path;
        let side_dir = self.paths[p as usize].direction;
        self.units[u as usize].fl.flags &= 0xfe;
        let saved = self.units[u as usize].fl.velocity_dir_prev;
        let mut m = self.mv;
        flingy::predict_with_dir(&mut self.units[u as usize].fl, d, &mut m, side_dir);
        self.mv = m;
        {
            let f = &mut self.units[u as usize].fl;
            f.flags |= 1;
            f.velocity_dir_prev = saved;
            f.desired_dir = side_dir;
            flingy::turn_velocity(f);
        }
        if self.unit_blocking_move(u).is_some() || self.predicted_move_blocked(d, u) {
            self.state(u, 0x21);
            return false;
        }
        self.apply_move_checked(d, u);
        let keep = self.units[u as usize].fl.next_speed;
        let ps = self.paths[p as usize].speed;
        set_speeds(&mut self.units[u as usize].fl, ps);
        let sp = self.unit_sprite_pos(u);
        let nw = self.units[u as usize].fl.next_waypoint;
        let dir = direction_to(sp.0 as i32, sp.1 as i32, nw.0 as i32, nw.1 as i32);
        let mut m = self.mv;
        flingy::predict_with_dir(&mut self.units[u as usize].fl, d, &mut m, dir);
        self.mv = m;
        set_speeds(&mut self.units[u as usize].fl, keep);
        if !self.predicted_move_blocked(d, u) {
            self.units[u as usize].fl.flags |= 1;
            self.state(u, 0x19);
        }
        false
    }

    /// 0x402a90: moving toward the next waypoint (+0x18) already.
    pub(super) fn facing_next_waypoint(&self, u: u32) -> bool {
        let f = &self.units[u as usize].fl;
        let sp = self.unit_sprite_pos(u);
        f.velocity_dir_prev == direction_to(sp.0 as i32, sp.1 as i32, f.next_waypoint.0 as i32, f.next_waypoint.1 as i32)
    }



}

#[cfg(test)]
mod tests {
    #[test]
    fn handle_roundtrip() {
        let mut st = super::State::new((4, 4), 0, 8);
        st.units[3].unique_id = 5;
        st.units[3].sprite = 0;
        let h = st.unit_handle(3);
        assert_eq!(h, 5 << 11 | 4);
        assert_eq!(st.unit_from_handle(h), Some(3));
        st.units[3].unique_id = 6;
        assert_eq!(st.unit_from_handle(h), None);
    }
}
