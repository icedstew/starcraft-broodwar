//! Flingy movement, x86 1.16.1 split form (0x494xxx-0x496xxx).
//!
//! BW predicts a step into globals (0x495ee0 and friends), lets the caller veto it
//! (unit collision, 0x46ad90), then applies it (0x496030). The prediction side
//! outputs live in `MoveScratch`. Field offsets are x86 CUnit/CBullet offsets.

use super::list::NIL;
use super::{Data, State};
use crate::game::flingy::{direction_table, direction_to, mflags, xy_length, Flingy};

/// Globals written by the predict functions.
#[derive(Clone, Copy, Default, Debug)]
pub struct MoveScratch {
    /// 0x63ff30: started moving -> anim Walking (0xb).
    pub starting: bool,
    /// 0x63fec1: stopped -> anim WalkingToIdle (0xc).
    pub stopping: bool,
    /// 0x63fec2: flags saved across a predict.
    pub saved_flags: u8,
    /// 0x63fec0: flags produced by the predict (restored by apply).
    pub new_flags: u8,
    /// 0x63fed4 / 0x63fecc: predicted position.
    pub new_pos: (i16, i16),
    /// 0x63fed8 / 0x63ff40: predicted exact position (fp8).
    pub new_exact: (i32, i32),
    /// 0x63ff2c: distance moved by the predicted step.
    pub moved: i32,
    /// 0x63fec4 / 0x63fed0: position before apply.
    pub old_pos: (i16, i16),
}

fn recalc_velocity(f: &mut Flingy) {
    let (dx, dy) = direction_table()[f.velocity_dir as usize];
    f.velocity = ((dx * f.speed) >> 8, (dy * f.speed) >> 8);
}

/// 0x494f60: set current speed (+0x38).
pub fn set_speed(f: &mut Flingy, s: i32) {
    if f.speed != s {
        f.speed = s;
        recalc_velocity(f);
    }
}

/// 0x4951c0: set next speed (+0x3c) and current speed.
pub fn set_speeds(f: &mut Flingy, s: i32) {
    f.next_speed = s;
    set_speed(f, s);
}

/// Set velocity direction (+0x4a), recomputing velocity (inlined in many places).
fn set_velocity_dir(f: &mut Flingy, d: u8) {
    if f.velocity_dir != d {
        f.velocity_dir = d;
        recalc_velocity(f);
    }
}

/// 0x402180: accelerate by delta, clamped to [0, top_speed].
pub fn accelerate(f: &mut Flingy, delta: i32) {
    let n = f.speed + delta;
    if n > f.top_speed {
        set_speed(f, f.top_speed);
    } else if n < 0 {
        if f.speed != 0 {
            f.speed = 0;
            f.velocity = (0, 0);
        }
    } else {
        set_speed(f, n);
    }
}

/// 0x494f90: distance needed to stop from next speed.
pub fn halt_distance(f: &Flingy, d: &Data) -> i32 {
    if f.next_speed == 0 || f.movement_type != 0 {
        return 0;
    }
    let i = f.flingy_id as usize;
    let fl = &d.dat.flingy;
    if f.next_speed == fl.top_speed[i] as i32 && f.acceleration == fl.acceleration[i] {
        return fl.halt_distance[i] as i32;
    }
    (f.next_speed * f.next_speed) / ((f.acceleration as i32) << 1)
}

/// 0x495100: turn velocity direction toward desired (+0x4b), limited by turn rate.
fn turn_velocity(f: &mut Flingy) {
    let v = f.velocity_dir_prev;
    if f.desired_dir == v {
        set_velocity_dir(f, v);
        return;
    }
    let mut lim = f.turn_rate;
    if f.movement_type != 2 {
        lim >>= 1;
    }
    let diff = f.desired_dir.wrapping_sub(v);
    let step = if (diff as i8 as i32) < lim as i32 {
        if (diff as i8 as i32) <= -(lim as i32) {
            lim.wrapping_neg()
        } else {
            diff
        }
    } else {
        lim
    };
    set_velocity_dir(f, v.wrapping_add(step));
}

/// 0x495840: desired direction toward next waypoint (else next target waypoint, else heading).
pub fn update_desired_dir(f: &mut Flingy) {
    let (x, y) = (f.pos.0 as i32, f.pos.1 as i32);
    f.desired_dir = if f.next_waypoint != f.pos {
        direction_to(x, y, f.next_waypoint.0 as i32, f.next_waypoint.1 as i32)
    } else if f.next_target_waypoint == f.pos {
        f.heading
    } else {
        direction_to(x, y, f.next_target_waypoint.0 as i32, f.next_target_waypoint.1 as i32)
    };
    turn_velocity(f);
}

/// 0x495cb0: speed control.
pub fn update_speed(f: &mut Flingy, d: &Data) {
    let at_target = f.move_target == f.pos;
    match f.movement_type {
        0 => {
            if at_target && f.next_speed < 0xc0 && f.flags & 0x30 == 0 {
                f.flags &= !mflags::BRAKING;
                set_speed(f, 0);
                return;
            }
        }
        1 => {
            if at_target {
                f.flags &= !mflags::BRAKING;
                set_speed(f, 0);
                return;
            }
        }
        2 => {
            if at_target {
                f.flags &= !mflags::BRAKING;
                set_speed(f, 0);
                return;
            }
            if f.flags & mflags::TURNING != 0 {
                let mut a = f.heading.wrapping_sub(f.desired_dir) as u32;
                if a > 0x80 {
                    a = 0x100 - a;
                }
                if a >= 0x20 {
                    if f.flags & mflags::MOVING != 0 {
                        f.flags &= !(mflags::MOVING | mflags::BRAKING);
                        set_speed(f, 0);
                    }
                    return;
                }
            }
            set_speed(f, f.next_speed);
            return;
        }
        _ => {}
    }
    set_speed(f, f.next_speed);
    let dist = xy_length(f.pos.0 as i32, f.next_waypoint.0 as i32, f.pos.1 as i32, f.next_waypoint.1 as i32);
    if f.velocity_dir != f.desired_dir && dist < 0x20 {
        let mut a = f.desired_dir.wrapping_sub(f.velocity_dir) as i32;
        if a > 0x80 {
            a = 0x100 - a;
        }
        let tr = f.turn_rate as i32;
        let frames = (tr + a * 2 - 1) / tr;
        let need = ((frames.wrapping_mul(f.next_speed) as u32) >> 8).wrapping_mul(3) >> 1;
        if need > dist {
            accelerate(f, -(f.acceleration as i32));
            return;
        }
    }
    if f.movement_type == 0
        && (f.flags & 0x10 == 0 || f.flags & 0x40 != 0)
        && (f.flags & 0x20 == 0 || f.next_waypoint != f.move_target)
    {
        let dist = xy_length(f.exact.0, (f.next_waypoint.0 as i32) << 8, f.exact.1, (f.next_waypoint.1 as i32) << 8);
        if (halt_distance(f, d) as u32) >= dist {
            accelerate(f, -(f.acceleration as i32));
            return;
        }
    }
    accelerate(f, f.acceleration as i32);
}

/// 0x495080: moving-flag bookkeeping; sets the starting/stopping globals.
pub fn update_moving_flag(f: &mut Flingy, m: &mut MoveScratch) {
    let flags = f.flags;
    let moving = flags & mflags::MOVING != 0;
    m.starting = false;
    m.stopping = false;
    let at_target = f.move_target == f.pos;
    if moving && at_target {
        f.flags = flags & !mflags::MOVING;
        if flags & mflags::STARTING == 0 {
            m.stopping = true;
        }
    } else if !moving && !at_target {
        if f.movement_type != 2 || flags & mflags::STARTING == 0 {
            f.flags |= mflags::MOVING;
        }
        if f.flags & mflags::STARTING == 0 {
            m.starting = true;
        }
    }
}

/// 0x4956c0: predict the next position into the scratch globals.
pub fn predict_position(f: &mut Flingy, m: &mut MoveScratch) {
    m.moved = f.speed;
    if f.flags & mflags::MOVING == 0 || f.speed == 0 {
        m.new_pos = f.pos;
        m.new_exact = f.exact;
        return;
    }
    let dist = xy_length(f.exact.0, (f.next_waypoint.0 as i32) << 8, f.exact.1, (f.next_waypoint.1 as i32) << 8) as i32;
    if f.speed < dist {
        m.new_exact = (f.velocity.0 + f.exact.0, f.velocity.1 + f.exact.1);
        m.new_pos = ((m.new_exact.0 as u32 >> 8) as i16, (m.new_exact.1 as u32 >> 8) as i16);
    } else {
        m.new_pos = f.next_waypoint;
        m.new_exact = ((m.new_pos.0 as i32) << 8, (m.new_pos.1 as i32) << 8);
        m.moved = dist;
    }
    if f.movement_type == 2 {
        set_speed(f, 0);
    }
}

/// 0x495ee0: full predict step; flags are restored, the predicted flags kept in scratch.
pub fn predict(f: &mut Flingy, d: &Data, m: &mut MoveScratch) {
    m.saved_flags = f.flags;
    update_desired_dir(f);
    update_speed(f, d);
    update_moving_flag(f, m);
    predict_position(f, m);
    m.new_flags = f.flags;
    f.flags = m.saved_flags;
}

/// 0x495e80: predict with a forced velocity direction (no desired-direction update).
pub fn predict_with_dir(f: &mut Flingy, d: &Data, m: &mut MoveScratch, dir: u8) {
    m.saved_flags = f.flags;
    set_velocity_dir(f, dir);
    update_speed(f, d);
    update_moving_flag(f, m);
    predict_position(f, m);
    m.new_flags = f.flags;
    f.flags = m.saved_flags;
}

/// 0x495980: predict with a forced velocity direction and speed.
pub fn predict_with_dir_speed(f: &mut Flingy, m: &mut MoveScratch, dir: u8, speed: i32) {
    m.saved_flags = f.flags;
    set_velocity_dir(f, dir);
    set_speeds(f, speed);
    update_moving_flag(f, m);
    predict_position(f, m);
    m.new_flags = f.flags;
    f.flags = m.saved_flags;
}

/// 0x494fe0: turn heading (+0x21) toward desired direction. Returns false when not turning.
pub fn turn_heading(f: &mut Flingy) -> bool {
    let flags = f.flags;
    if flags & mflags::MOVING != 0 && flags & mflags::TURNING == 0 {
        return false;
    }
    let h = f.heading as i8;
    let mut diff = (f.desired_dir as i8).wrapping_sub(h);
    if diff < -0x7f {
        diff = -0x80;
    }
    let tr = f.turn_rate as i8;
    f.heading = if tr < diff {
        tr.wrapping_add(h) as u8
    } else if (diff as i32) < -(tr as i32) {
        h.wrapping_sub(tr) as u8
    } else {
        h.wrapping_add(diff) as u8
    };
    let id = f.flingy_id;
    if (0x8d..0xac).contains(&id) || (0xc9..0xcf).contains(&id) {
        f.turn_rate = f.turn_rate.wrapping_add(1);
    }
    if f.velocity_dir_prev == f.desired_dir && f.heading == f.desired_dir {
        f.flags = flags & !mflags::TURNING;
    }
    true
}

impl State {
    /// 0x497a10: move a sprite, relinking it into its new row list and marking images for redraw.
    pub fn move_sprite(&mut self, s: u32, x: i16, y: i16) {
        let sp = &self.sprites[s as usize];
        if sp.x == x && sp.y == y {
            return;
        }
        let old = self.sprite_row(s);
        self.sprites[s as usize].x = x;
        self.sprites[s as usize].y = y;
        let new = self.sprite_row(s);
        if old != new {
            let mut l = self.sprite_rows[old];
            super::list::remove(&mut self.sprites, s, &mut l);
            self.sprite_rows[old] = l;
            let mut l = self.sprite_rows[new];
            super::list::insert_after_head(&mut self.sprites, s, &mut l);
            self.sprite_rows[new] = l;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            self.images[i as usize].flags |= super::sprite::iflag::REDRAW;
            i = self.images[i as usize].link.next;
        }
    }

    /// Set the direction of every image of a sprite (loop over 0x4d5f80).
    pub fn sprite_images_set_direction(&mut self, d: &Data, s: u32, dir: u8) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.image_set_direction(d, i, dir);
            i = next;
        }
    }

    /// 0x496030: apply a predicted unit step. Returns whether the position changed.
    pub fn unit_apply_move(&mut self, d: &Data, u: u32) -> bool {
        let m = self.mv;
        let f = &mut self.units[u as usize].fl;
        self.mv.old_pos = f.pos;
        let changed = f.pos != m.new_pos;
        f.velocity_dir_prev = f.velocity_dir;
        f.pos = m.new_pos;
        f.flags = m.new_flags;
        f.exact = m.new_exact;
        f.next_speed = f.speed;
        let s = self.units[u as usize].sprite;
        self.move_sprite(s, m.new_pos.0, m.new_pos.1);
        turn_heading(&mut self.units[u as usize].fl);
        let h = self.units[u as usize].fl.heading;
        self.sprite_images_set_direction(d, s, h);
        if m.stopping {
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                let next = self.images[i as usize].link.next;
                if self.sprites[s as usize].flags & 0x80 == 0 {
                    self.image_run_anim(d, i, 0xc);
                }
                i = next;
            }
        } else if m.starting {
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                let next = self.images[i as usize].link.next;
                self.image_run_anim(d, i, 0xb);
                i = next;
            }
        }
        changed
    }

    /// 0x496140: turn in place: copy velocity direction, turn heading, update image directions.
    pub fn unit_turn_in_place(&mut self, d: &Data, u: u32) {
        let f = &mut self.units[u as usize].fl;
        f.velocity_dir_prev = f.velocity_dir;
        turn_heading(f);
        let (s, h) = (self.units[u as usize].sprite, self.units[u as usize].fl.heading);
        self.sprite_images_set_direction(d, s, h);
    }

    /// 0x401fa0: clamp a position so the unit's box stays inside the map.
    pub fn clamp_unit_pos(&self, d: &Data, unit_type: u16, p: &mut (i16, i16)) {
        let dim = d.dat.units.dimensions[unit_type as usize];
        let (l, u, r, dn) = (dim as i16, (dim >> 16) as i16, (dim >> 32) as i16, (dim >> 48) as i16);
        let (w, h) = (self.map_px.0 as i16, self.map_px.1 as i16);
        if p.0 < l {
            p.0 = l;
        } else if p.0 as i32 >= w as i32 - r as i32 {
            p.0 = w - r - 1;
        }
        if p.1 < u {
            p.1 = u;
        } else if p.1 as i32 >= h as i32 - dn as i32 - 0x20 {
            p.1 = h - dn - 0x21;
        }
    }

    /// 0x401dc0: 0 = not at move target, 1 = at it, 2 = at it and 0x80000 (unmovable) status set.
    pub fn unit_at_move_target(&self, u: u32) -> u8 {
        let un = &self.units[u as usize];
        let s = &self.sprites[un.sprite as usize];
        if un.fl.move_target == (s.x, s.y) {
            1 + (un.status & super::unit::status::UNMOVABLE != 0) as u8
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerate_clamps() {
        let mut f = Flingy { top_speed: 100, velocity_dir: 0x40, ..Default::default() };
        accelerate(&mut f, 150);
        assert_eq!((f.speed, f.velocity), (100, (100, 0)));
        accelerate(&mut f, -150);
        assert_eq!((f.speed, f.velocity), (0, (0, 0)));
    }

    #[test]
    fn turn_rate_limited() {
        let mut f = Flingy { heading: 0, desired_dir: 0x40, turn_rate: 0x10, flags: mflags::TURNING, ..Default::default() };
        assert!(turn_heading(&mut f));
        assert_eq!(f.heading, 0x10);
        f.heading = 0x38;
        f.velocity_dir_prev = 0x40;
        turn_heading(&mut f);
        assert_eq!(f.heading, 0x40);
        assert_eq!(f.flags & mflags::TURNING, 0);
    }
}
