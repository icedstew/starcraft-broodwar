//! Flingy movement physics: direction, speed, turning, position stepping.
//!
//! Port of the x64 SC:R routines (addresses in comments). Field offsets are
//! the x64 CUnit/CFlingy layout, kept for cross-referencing the decomp.

use crate::dat::Flingy as FlingyDat;
use std::sync::OnceLock;

/// 256 directions, 0 = up, clockwise. (x, y) unit vectors * 256.
/// Equals BW's table exactly: (round(sin * 256), round(-cos * 256)).
pub fn direction_table() -> &'static [(i32, i32); 256] {
    static T: OnceLock<[(i32, i32); 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [(0, 0); 256];
        for (i, v) in t.iter_mut().enumerate() {
            let a = i as f64 * std::f64::consts::TAU / 256.0;
            *v = ((a.sin() * 256.0).round() as i32, (-a.cos() * 256.0).round() as i32);
        }
        t
    })
}

/// tan thresholds: ceil(tan((i + 1) * pi / 128) * 256), last = u32::MAX. (0x140dac430)
fn tan_table() -> &'static [u32; 64] {
    static T: OnceLock<[u32; 64]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [u32::MAX; 64];
        for (i, v) in t.iter_mut().enumerate().take(63) {
            *v = ((i as f64 + 1.0) * std::f64::consts::PI / 128.0).tan().mul_add(256.0, 0.0).ceil() as u32;
        }
        t
    })
}

/// 0x494c10 / SC:R 0x1401881b0: signed slope (fp8) -> direction offset within a quadrant
/// (binary search over 0x513528).
fn atan_dir(slope: i32) -> i32 {
    let t = tan_table();
    let a = slope.unsigned_abs();
    let (mut lo, mut hi) = (0u32, 0x40u32);
    let mut mid = 0x20u32;
    loop {
        let next_lo = mid + 1;
        if a <= t[mid as usize] {
            hi = mid;
        } else {
            lo = next_lo;
        }
        mid = (hi + lo) >> 1;
        if hi == lo {
            break;
        }
    }
    if slope < 0 { -(mid as i32) } else { mid as i32 }
}

/// 0x495300: direction from (x, y) to (tx, ty) (as in SC:R 0x140189e50).
pub fn direction_to(x: i32, y: i32, tx: i32, ty: i32) -> u8 {
    let dx = tx - x;
    let dy = ty - y;
    if dx == 0 {
        return if dy < 1 { 0 } else { 0x80 };
    }
    let r = atan_dir((dy * 0x100) / dx);
    if dx < 0 {
        let v = r + 0xC0;
        if v == 0x100 { 0 } else { v as u8 }
    } else {
        (r + 0x40) as u8
    }
}

/// 0x14021d820: BW's approximate distance.
pub fn xy_length(x1: i32, x2: i32, y1: i32, y2: i32) -> u32 {
    let (dx, dy) = ((x2 - x1).abs(), (y2 - y1).abs());
    let (a, b) = if dx >= dy { (dx, dy) } else { (dy, dx) };
    if a >> 2 >= b {
        return a as u32;
    }
    let t = (b >> 1) + b;
    let e = (t >> 7) - (a >> 6) - (a >> 4);
    ((t >> 2) + a + e) as u32
}

pub mod mflags {
    /// Turning toward the desired heading.
    pub const TURNING: u8 = 0x1;
    /// Moving toward next_movement_waypoint.
    pub const MOVING: u8 = 0x2;
    pub const BRAKING: u8 = 0x4;
    pub const STARTING: u8 = 0x8;
}

#[derive(Clone, Default, Debug)]
pub struct Flingy {
    pub flingy_id: u16,              // +0x3c
    pub move_target: (i16, i16),     // +0x20
    pub next_waypoint: (i16, i16),   // +0x30 next_movement_waypoint
    pub next_target_waypoint: (i16, i16), // +0x34
    pub flags: u8,                   // +0x38
    pub heading: u8,                 // +0x39
    pub turn_rate: u8,               // +0x3a
    pub velocity_dir_prev: u8,       // +0x3b
    pub movement_type: u8,           // +0x3f
    pub pos: (i16, i16),             // +0x40
    pub exact: (i32, i32),           // +0x44, +0x48 (fp8)
    pub top_speed: i32,              // +0x4c
    pub speed: i32,                  // +0x50
    pub next_speed: i32,             // +0x54
    pub velocity: (i32, i32),        // +0x58, +0x5c
    pub acceleration: u16,           // +0x60
    pub velocity_dir: u8,            // +0x62
    pub desired_dir: u8,             // +0x63
}

/// Side outputs of one movement step (BW keeps these in globals 0x140e616d0..ef).
#[derive(Default, Debug)]
pub struct StepResult {
    pub new_pos: (i16, i16),
    pub new_exact: (i32, i32),
    pub moved_distance: i32,
    /// 0x140e616ec: unit just started moving -> iscript Walking.
    pub starting: bool,
    /// 0x140e616ed: unit just stopped -> iscript WalkingToIdle.
    pub stopping: bool,
}

impl Flingy {
    pub fn new(dat: &FlingyDat, id: u16, x: i16, y: i16, heading: u8) -> Flingy {
        let i = id as usize;
        Flingy {
            flingy_id: id,
            move_target: (x, y),
            next_waypoint: (x, y),
            next_target_waypoint: (x, y),
            heading,
            velocity_dir_prev: heading,
            velocity_dir: heading,
            desired_dir: heading,
            turn_rate: dat.turn_rate[i],
            movement_type: dat.movement_control[i],
            pos: (x, y),
            exact: ((x as i32) << 8, (y as i32) << 8),
            top_speed: dat.top_speed[i] as i32,
            acceleration: dat.acceleration[i],
            ..Default::default()
        }
    }

    fn set_velocity_dir(&mut self, d: u8) {
        if self.velocity_dir != d {
            self.velocity_dir = d;
            self.recalc_velocity();
        }
    }

    fn recalc_velocity(&mut self) {
        let (dx, dy) = direction_table()[self.velocity_dir as usize];
        self.velocity = ((dx * self.speed) >> 8, (dy * self.speed) >> 8);
    }

    /// 0x1401899c0
    pub fn set_speed(&mut self, s: i32) {
        if self.speed != s {
            self.speed = s;
            self.recalc_velocity();
        }
    }

    /// 0x140188680: accelerate by delta, clamped to [0, top_speed].
    fn accelerate(&mut self, delta: i32) {
        let n = self.speed + delta;
        if n > self.top_speed {
            if self.speed != self.top_speed {
                self.speed = self.top_speed;
                self.recalc_velocity();
            }
        } else if n < 0 {
            if self.speed != 0 {
                self.speed = 0;
                self.velocity = (0, 0);
            }
        } else if self.speed != n {
            self.speed = n;
            self.recalc_velocity();
        }
    }

    /// 0x1401888b0: distance needed to stop from next_speed.
    fn halt_distance(&self, dat: &FlingyDat) -> i32 {
        if self.next_speed == 0 || self.movement_type != 0 {
            return 0;
        }
        let i = self.flingy_id as usize;
        if self.next_speed == dat.top_speed[i] as i32 && self.acceleration == dat.acceleration[i] {
            return dat.halt_distance[i] as i32;
        }
        (self.next_speed * self.next_speed) / (self.acceleration as i32 * 2)
    }

    /// 0x140189e50: desired direction toward the waypoint, then turn velocity by at most turn rate.
    fn update_desired_direction(&mut self) {
        let (px, py) = (self.pos.0 as i32, self.pos.1 as i32);
        let d = if self.next_waypoint != self.pos {
            direction_to(px, py, self.next_waypoint.0 as i32, self.next_waypoint.1 as i32)
        } else if self.next_target_waypoint != self.pos {
            direction_to(px, py, self.next_target_waypoint.0 as i32, self.next_target_waypoint.1 as i32)
        } else {
            self.heading
        };
        self.desired_dir = d;
        let mut v = self.velocity_dir_prev;
        if d != v {
            let mut limit = self.turn_rate;
            if self.movement_type != 2 {
                limit >>= 1;
            }
            let diff = d.wrapping_sub(v) as i8 as i32;
            let step = if diff >= limit as i32 {
                limit
            } else if diff <= -(limit as i32) {
                limit.wrapping_neg()
            } else {
                diff as u8
            };
            v = v.wrapping_add(step);
        }
        self.set_velocity_dir(v);
    }

    /// 0x14018a1b0: speed control.
    fn update_speed(&mut self, dat: &FlingyDat) {
        let at_target = self.move_target == self.pos;
        match self.movement_type {
            0 => {
                if at_target && self.next_speed < 0xC0 && self.flags & 0x30 == 0 {
                    self.flags &= !mflags::BRAKING;
                    self.set_speed(0);
                    return;
                }
            }
            1 => {
                if at_target {
                    self.stop_hard();
                    return;
                }
            }
            2 => {
                if at_target {
                    self.stop_hard();
                    return;
                }
                if self.flags & mflags::TURNING != 0 {
                    let d = self.heading.wrapping_sub(self.desired_dir);
                    let d = if d <= 0x80 { d as u32 } else { 0x100 - d as u32 };
                    if d > 0x1F {
                        if self.flags & mflags::MOVING != 0 {
                            self.flags &= !(mflags::MOVING | mflags::BRAKING);
                            self.set_speed(0);
                        }
                        return;
                    }
                }
                let s = self.next_speed;
                if self.speed != s {
                    self.speed = s;
                    self.recalc_velocity();
                }
                return;
            }
            _ => {}
        }
        let s = self.next_speed;
        if self.speed != s {
            self.speed = s;
            self.recalc_velocity();
        }
        let dist = xy_length(self.pos.0 as i32, self.next_waypoint.0 as i32, self.pos.1 as i32, self.next_waypoint.1 as i32);
        let desired = self.desired_dir;
        if self.velocity_dir != desired && dist < 0x20 {
            let tr = self.turn_rate as i32;
            let d = desired.wrapping_sub(self.velocity_dir);
            let d = if d <= 0x80 { d as i32 } else { 0x100 - d as i32 };
            let turn_frames = (tr - 1 + d * 2) / tr;
            if (dist as i32) < ((((turn_frames * self.next_speed) as u32) >> 8) * 3 >> 1) as i32 {
                self.accelerate(-(self.acceleration as i32));
                return;
            }
        }
        if self.movement_type == 0
            && (self.flags & 0x10 == 0 || self.flags & 0x40 != 0)
            && (self.flags & 0x20 == 0 || self.next_waypoint != self.move_target)
        {
            let halt = self.halt_distance(dat);
            let d = xy_length(self.exact.0, (self.next_waypoint.0 as i32) << 8, self.exact.1, (self.next_waypoint.1 as i32) << 8);
            if d <= halt as u32 {
                self.accelerate(-(self.acceleration as i32));
                return;
            }
        }
        self.accelerate(self.acceleration as i32);
    }

    fn stop_hard(&mut self) {
        self.flags &= !mflags::BRAKING;
        if self.speed != 0 {
            self.speed = 0;
            self.velocity = (0, 0);
        }
    }

    /// 0x14018a060: start/stop bookkeeping. Returns (starting, stopping).
    fn update_moving_flag(&mut self) -> (bool, bool) {
        let at_target = self.move_target == self.pos;
        if self.flags & mflags::MOVING != 0 {
            if at_target {
                self.flags &= !mflags::MOVING;
                return (false, self.flags & mflags::STARTING == 0);
            }
            return (false, false);
        }
        if at_target {
            return (false, false);
        }
        if self.movement_type == 2 && self.flags & mflags::STARTING != 0 {
            return (false, false);
        }
        self.flags |= mflags::MOVING;
        (self.flags & mflags::STARTING == 0, false)
    }

    /// 0x140189f70: compute next position (does not apply it).
    fn next_position(&mut self) -> ((i16, i16), (i32, i32), i32) {
        let mut moved = self.speed;
        if self.flags & mflags::MOVING == 0 || self.speed == 0 {
            return (self.pos, self.exact, moved);
        }
        let wx = (self.next_waypoint.0 as i32) << 8;
        let wy = (self.next_waypoint.1 as i32) << 8;
        let dist = xy_length(self.exact.0, wx, self.exact.1, wy) as i32;
        let r = if self.speed < dist {
            let e = (self.exact.0 + self.velocity.0, self.exact.1 + self.velocity.1);
            (((e.0 >> 8) as i16, (e.1 >> 8) as i16), e)
        } else {
            moved = dist;
            (self.next_waypoint, (wx, wy))
        };
        if self.movement_type == 2 && self.speed != 0 {
            self.speed = 0;
            self.velocity = (0, 0);
        }
        (r.0, r.1, moved)
    }

    /// 0x14018a0f0: turn heading toward desired_dir. Returns whether the heading was updated.
    fn turn(&mut self) -> bool {
        if self.flags & mflags::MOVING != 0 && self.flags & mflags::TURNING == 0 {
            return false;
        }
        let tr = self.turn_rate as i8;
        let mut diff = self.desired_dir.wrapping_sub(self.heading) as i8;
        if diff < -0x7F {
            diff = -0x80;
        }
        self.heading = if diff > tr {
            self.heading.wrapping_add(tr as u8)
        } else if (diff as i32) < -(tr as i32) {
            self.heading.wrapping_sub(tr as u8)
        } else {
            self.heading.wrapping_add(diff as u8)
        };
        // Missiles/interceptor-like flingies (0x8d..=0xab, 0xc9..=0xce) turn faster over time.
        let id = self.flingy_id;
        if id.wrapping_sub(0x8D) < 0x1F || id.wrapping_sub(0xC9) < 6 {
            self.turn_rate = self.turn_rate.wrapping_add(1);
        }
        if self.velocity_dir_prev == self.desired_dir && self.heading == self.desired_dir {
            self.flags &= !mflags::TURNING;
        }
        true
    }

    /// 0x140189c10: one frame of movement. Caller moves the sprite to the new pos,
    /// sets its direction to `heading`, and plays Walking/WalkingToIdle per the result.
    pub fn step(&mut self, dat: &FlingyDat) -> StepResult {
        let saved_flags = self.flags;
        self.update_desired_direction();
        self.update_speed(dat);
        let (starting, stopping) = self.update_moving_flag();
        let (new_pos, new_exact, moved) = self.next_position();
        let flags_after = self.flags;
        let _ = saved_flags;
        self.velocity_dir_prev = self.velocity_dir;
        self.flags = flags_after;
        self.next_speed = self.speed;
        self.pos = new_pos;
        self.exact = new_exact;
        self.turn();
        StepResult { new_pos, new_exact, moved_distance: moved, starting, stopping }
    }

    /// Point the flingy at a new target (subset of set_unit_move_target: straight-line, no pathing yet).
    pub fn set_move_target(&mut self, x: i16, y: i16) {
        self.move_target = (x, y);
        self.next_waypoint = (x, y);
        self.next_target_waypoint = (x, y);
        self.flags |= mflags::TURNING;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        assert_eq!(direction_to(0, 0, 0, -10), 0);
        assert_eq!(direction_to(0, 0, 10, 0), 0x40);
        assert_eq!(direction_to(0, 0, 0, 10), 0x80);
        assert_eq!(direction_to(0, 0, -10, 0), 0xC0);
        assert_eq!(direction_table()[64], (256, 0));
    }

    #[test]
    fn distance() {
        assert_eq!(xy_length(0, 100, 0, 0), 100);
        // BW's approximation underestimates diagonals: a - a/16 - a/64 + 3b/8 + 3b/256.
        assert_eq!(xy_length(0, 100, 0, 100), 131);
        assert_eq!(xy_length(0, 10, 0, 1), 10);
    }
}
