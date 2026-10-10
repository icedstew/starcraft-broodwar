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

/// CPath (unit +0x100). Only the fields the movement states touch.
#[derive(Clone, Copy, Default, Debug)]
pub struct Path {
    /// +0x4: next waypoint.
    pub next_waypoint: (i16, i16),
    /// +0xc: frame the path was created.
    pub creation_frame: u32,
    /// +0x10: handle of the unit being avoided.
    pub collision_unit: u32,
    /// +0x14: speed saved while avoiding.
    pub speed: i32,
    /// +0x18: flags (1 = repath requested).
    pub flags: u8,
    /// +0x19: wait countdown.
    pub wait: u8,
    /// +0x1a: avoidance direction.
    pub direction: u8,
}

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
                0xd => self.mv_repath_wait(u),
                0xe => self.mv_no_path_move(d, u),
                0xf => {
                    self.units[u as usize].pathing_collision_interval = 10;
                    self.mv_stop_walking_anim(d, u);
                    self.units[u as usize].movement_state = 0x10;
                    return;
                }
                0x10 => self.mv_retry_path(d, u),
                0x11 => self.mv_request_path(u),
                0x12 => self.mv_wait_random(u),
                0x13 => self.mv_get_path_again(u),
                0x14 => self.mv_path_check(d, u),
                0x15 => self.mv_clear_repath(u),
                0x16 => {
                    let t = self.units[u as usize].fl.move_target;
                    let ok = self.stub_path_request(u, t);
                    self.units[u as usize].movement_state = if ok { 0x19 } else { 0xf };
                    return;
                }
                0x17 => self.mv_repath_after_collision(u),
                0x18 => self.mv_repath_ignoring_units(u),
                0x19 => self.mv_follow_path(d, u),
                0x1a => self.mv_check_blockers(u),
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
            todo_port!("0x4e5f30", "unit vision update after moving to a new tile");
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
        self.iscript_order_unit = Some(sub);
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
        self.iscript_order_unit = Some(u);
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
    fn subunit_face(&mut self, d: &Data, u: u32, dir: u8) {
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

    fn unit_sprite_pos(&self, u: u32) -> (i16, i16) {
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
        self.unit_halt(u);
        let p = self.unit_sprite_pos(u);
        self.set_move_target_xy(d, u, p);
        self.units[u as usize].status |= status::UNMOVABLE;
        self.units[u as usize].user_action_flags &= !2; // +0x93 bit 1
    }

    /// 0x4eb290.
    fn unit_halt(&mut self, u: u32) {
        let un = &mut self.units[u as usize];
        if un.pathing_flags & 2 != 0 {
            un.pathing_flags |= 4;
        }
        todo_port!("0x4957a0", "flingy halt (move target := position, clear flags 0x70)");
        todo_port!("0x401fa0", "clamp move target to map");
        self.units[u as usize].recent_order_timer = 0xf;
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
        let _ = q;
        todo_port!("0x4eb820", "order queue head: set flingy flag 0x20 unless orders.dat 0x665100[order] (needs order queue)");
        self.units[u as usize].fl.flags &= !0x20;
        true
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
    fn predict(&mut self, d: &Data, u: u32) {
        let mut m = self.mv;
        flingy::predict(&mut self.units[u as usize].fl, d, &mut m);
        self.mv = m;
    }

    /// 0x46ad90: apply the predicted move, updating the unit finder.
    fn apply_move_checked(&mut self, d: &Data, u: u32) -> bool {
        if !self.unit_apply_move(d, u) {
            return false;
        }
        let (old, new) = (self.mv.old_pos, self.mv.new_pos);
        self.stub_unit_finder_move(u, new.0 as i32 - old.0 as i32, new.1 as i32 - old.1 as i32);
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
        let t = self.units[u as usize].fl.move_target;
        let _ = t;
        todo_port!("0x46be80", "4eb820 call with the subunit's current target (EBX/EDI from caller context)");
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
        let repulsed = self.air_repulse(u);
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
        self.air_repulse_finish(u, repulsed);
        false
    }

    /// 0x46a740: clamp the move target into the map; sets UNMOVABLE.
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
        }
        un.status |= status::UNMOVABLE;
    }

    /// 0x453420: air unit repulsion. Adjusts the predicted position; returns whether applied.
    fn air_repulse(&mut self, u: u32) -> bool {
        todo_port!("0x453420", "air repulse (repulse grid 0x6d5cd8, table 0x515418)");
        let _ = u;
        false
    }

    /// 0x4535a0: update repulse grid cell; snap target if close.
    fn air_repulse_finish(&mut self, u: u32, repulsed: bool) {
        let _ = (u, repulsed);
        todo_port!("0x4535a0", "repulse grid update");
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
        self.stub_path_delete(u);
        let un = &mut self.units[u as usize];
        un.fl.next_waypoint = un.fl.move_target;
        let ns = if un.status & status::IS_NORMAL != 0 && un.fl.flags & mflags::BRAKING == 0 { 0xc } else { 9 };
        self.state(u, ns);
        true
    }

    /// 0x46bdf0: terrain-only unit: check whether still stuck in terrain.
    fn mv_repath_check(&mut self, _d: &Data, u: u32) -> bool {
        self.units[u as usize].status &= !status::IGNORE_TILE_COLLISION;
        if self.stub_4f2de0(u) {
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
    fn mv_repath_wait(&mut self, u: u32) -> bool {
        let path = self.units[u as usize].path;
        if path == NIL {
            self.state(u, 0xc);
            return true;
        }
        if self.units[u as usize].pathing_flags & 4 != 0 {
            let un = &mut self.units[u as usize];
            un.pathing_flags &= 0xfb;
            un.status |= status::IGNORE_TILE_COLLISION;
            self.stub_path_delete(u);
            self.state(u, 9);
            return true;
        }
        let p = self.paths[path as usize];
        if p.flags & 1 != 0 || p.next_waypoint != self.units[u as usize].fl.next_waypoint {
            self.units[u as usize].fl.next_waypoint = p.next_waypoint;
        }
        if !self.stub_4f2160(u) {
            return false;
        }
        if self.units[u as usize].path != NIL {
            self.stub_path_delete(u);
        }
        todo_port!("0x42d9a0", "terrain escape finished");
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
        let blocked = (self.units[u as usize].status & status::IS_NORMAL != 0 && self.stub_collides_terrain(u))
            || self.stub_collides_units(u);
        if !blocked {
            return self.apply_move_checked(d, u);
        }
        self.apply_without_moving(d, u);
        if self.units[u as usize].move_target_unit != NIL {
            todo_port!("0x430f10", "notify move target unit of block");
        }
        set_speeds(&mut self.units[u as usize].fl, 0);
        let sp = self.unit_sprite_pos(u);
        let un = &mut self.units[u as usize];
        if sp != un.fl.move_target {
            un.fl.move_target = sp;
            un.move_target_unit = NIL;
            un.fl.flags = (un.fl.flags & !mflags::BRAKING) | mflags::TURNING;
            un.fl.next_waypoint = sp;
        }
        self.unit_halt(u);
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
        if !self.stub_path_request(u, t) {
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
            self.stub_path_delete(u);
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
    fn mv_request_path(&mut self, u: u32) -> bool {
        let t = self.units[u as usize].fl.move_target;
        if !self.stub_path_request(u, t) {
            self.state(u, 0xf);
            return false;
        }
        let un = &mut self.units[u as usize];
        if un.user_action_flags & 2 != 0 {
            un.user_action_flags &= !2;
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
    fn mv_get_path_again(&mut self, u: u32) -> bool {
        let p = self.units[u as usize].path;
        if p != NIL {
            let _avoid = self.unit_from_handle(self.paths[p as usize].collision_unit);
            self.stub_path_delete(u);
        }
        if self.needs_move(u) {
            let t = self.units[u as usize].fl.move_target;
            let ok = self.stub_path_request(u, t);
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
        if !self.stub_402a90(u) {
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
            self.stub_path_delete(u);
            self.state(u, 0x17);
        }
        true
    }

    /// 0x46bc30.
    fn mv_repath_after_collision(&mut self, u: u32) -> bool {
        if self.units[u as usize].path != NIL {
            self.stub_path_delete(u);
        }
        let t = self.units[u as usize].fl.move_target;
        let ok = self.stub_path_request(u, t);
        self.state(u, if ok { 0x19 } else { 0xf });
        true
    }

    /// 0x46bbc0: repath ignoring units (0x6bee80 = 1).
    fn mv_repath_ignoring_units(&mut self, u: u32) -> bool {
        self.stub_path_delete(u);
        let (t, tu) = (self.units[u as usize].fl.move_target, self.units[u as usize].move_target_unit);
        let ok = self.stub_path_request(u, t);
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
        if !self.stub_path_request(u, t) {
            self.state(u, 0x16);
            return true;
        }
        let mut speed = self.units[u as usize].fl.next_speed;
        self.predict(d, u);
        if self.units[u as usize].fl.movement_type == 0 {
            speed = self.units[u as usize].fl.speed;
        }
        let terrain = self.units[u as usize].status & status::IS_NORMAL != 0 && self.stub_collides_terrain(u);
        let units = self.stub_collides_units(u);
        if !terrain {
            if units {
                let p = self.units[u as usize].path;
                self.paths[p as usize].speed = speed;
                self.state(u, 0x22);
                return true;
            }
        } else {
            todo_port!("0x46b950", "terrain-collision half-step retry (0x4f2240) -> state 0x1c");
            let p = self.units[u as usize].path;
            self.paths[p as usize].collision_unit = 0;
            self.paths[p as usize].speed = speed;
            self.state(u, 0x1c);
            return true;
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
        if self.stub_42f700(u) {
            todo_port!("0x42f720", "path wait 0x1e");
            self.state(u, 0x1a);
        }
        false
    }

    /// 0x46ab10: count units blocking the path's next region; 0x1b if any, else 0x19.
    fn mv_check_blockers(&mut self, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        todo_port!("0x46ab10", "unit finder search (0x42e000/0x42db50/0x402b10)");
        self.state(u, 0x19);
        true
    }

    /// 0x46bf60: decide how to resolve a collision.
    fn mv_collision_resolve(&mut self, _d: &Data, u: u32) -> bool {
        if self.mv_check_path_state(u, true) {
            return true;
        }
        todo_port!("0x46bf60", "collision resolution (needs pathing 0x42da00/0x42d6b0/0x4f1a20/0x4f2a70)");
        self.state(u, 0x19);
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
        if !self.stub_collides_units_strict(u) {
            if self.stub_collides_units(u) {
                self.state(u, 0x17);
                return false;
            }
            self.apply_move_checked(d, u);
            self.state(u, 0x19);
            return false;
        }
        self.mv_stop_walking_anim(d, u);
        if !repath {
            set_speeds(&mut self.units[u as usize].fl, 0);
            if !self.stub_402a90(u) {
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
        if !self.stub_402a90(u) {
            self.unit_turn_in_place(d, u);
            return false;
        }
        self.stub_path_delete(u);
        let dir = self.stub_4f2a70(u);
        if dir < 0 {
            self.state(u, 0x18);
            return false;
        }
        let p = self.units[u as usize].path;
        self.paths[p as usize].collision_unit = 0;
        self.paths[p as usize].direction = dir as u8;
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
    fn mv_collision_sidestep(&mut self, _d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        todo_port!("0x46b000", "sidestep loop (needs 0x4f20d0/0x4f1980)");
        self.state(u, 0x13);
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
            self.stub_path_delete(u);
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
        if !self.stub_collides_units(u) && !self.stub_collides_units_strict(u) {
            self.apply_move_checked(d, u);
            self.units[u as usize].fl.flags |= mflags::TURNING;
            self.state(u, 0x19);
            return false;
        }
        self.mv_stop_walking_anim(d, u);
        if !self.stub_402a90(u) {
            self.unit_turn_in_place(d, u);
            return false;
        }
        todo_port!("0x4f1b70", "avoidance direction");
        self.paths[p as usize].direction = self.units[u as usize].fl.heading;
        self.state(u, 0x23);
        false
    }

    /// 0x46ae20.
    fn mv_collision_turn(&mut self, _d: &Data, u: u32) -> bool {
        let un = &mut self.units[u as usize];
        if un.pathing_collision_interval != 0xff {
            un.pathing_collision_interval += 1;
        }
        if self.mv_check_path_state(u, true) {
            return true;
        }
        todo_port!("0x46ae20", "turn-away step (needs 0x4f20d0/0x4f1980)");
        self.state(u, 0x21);
        false
    }

    // ---- Unported callees. Each returns the "no obstruction / no work" value. ----

    /// 0x42fe00: request a path to `t`. Without the pathfinder, use a straight line.
    fn stub_path_request(&mut self, u: u32, t: (i16, i16)) -> bool {
        todo_port!("0x42fe00", "pathfinder request");
        let un = &mut self.units[u as usize];
        un.fl.next_waypoint = t;
        if un.path == NIL {
            un.path = 0;
            if self.paths.is_empty() {
                self.paths.push(Path::default());
            }
        }
        true
    }

    /// 0x42f740: delete the unit's path.
    fn stub_path_delete(&mut self, u: u32) {
        todo_port!("0x42f740", "delete path");
        let _ = u;
    }

    /// 0x4f20d0: predicted position collides with terrain.
    fn stub_collides_terrain(&mut self, _u: u32) -> bool {
        todo_port!("0x4f20d0", "terrain collision check");
        false
    }

    /// 0x4f1980: predicted position collides with a unit.
    fn stub_collides_units(&mut self, _u: u32) -> bool {
        todo_port!("0x4f1980", "unit collision check");
        false
    }

    /// 0x4f20d0 variant used by the waiting states.
    fn stub_collides_units_strict(&mut self, _u: u32) -> bool {
        false
    }

    fn stub_4f2de0(&mut self, _u: u32) -> bool {
        todo_port!("0x4f2de0", "unit stuck in terrain");
        false
    }

    fn stub_4f2160(&mut self, _u: u32) -> bool {
        todo_port!("0x4f2160", "terrain escape step");
        true
    }

    fn stub_402a90(&mut self, _u: u32) -> bool {
        todo_port!("0x402a90", "facing next waypoint");
        true
    }

    fn stub_4f2a70(&mut self, _u: u32) -> i32 {
        todo_port!("0x4f2a70", "free direction search");
        -1
    }

    fn stub_42f700(&mut self, _u: u32) -> bool {
        false
    }

    /// 0x46a000: move the unit's unit-finder entries by (dx, dy).
    fn stub_unit_finder_move(&mut self, _u: u32, _dx: i32, _dy: i32) {
        todo_port!("0x46a000", "unit finder update");
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
