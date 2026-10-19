//! CFlingy.cpp: the free flingies (0x63fee0, a pool of one; 0x495b50 / 0x495a50).

use super::{In, Out};
use crate::bw::list::{Link, NIL};
use crate::bw::{Data, List, State};
use crate::game::flingy::Flingy;

/// 1-based index as 0x494ce0 stores a pointer, 0 for null.
fn idx1(i: u32) -> u32 {
    if i == NIL { 0 } else { i + 1 }
}

fn slot(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

impl State {
    /// 0x495b50: the pointers become indices (0x494ce0), then the count (u16: 1 when the
    /// flingy has a sprite) and the 0x4e-byte record (u16 index 0, the 0x4c-byte flingy),
    /// packed; the pointers are restored (not needed here, we convert while writing).
    pub(super) fn save_flingies(&mut self, _d: &Data, o: &mut Out) -> bool {
        let t = &self.free_flingies[0];
        let n = (t.sprite != NIL) as u16;
        o.u16(n);
        if n == 0 {
            return true;
        }
        let f = &t.fl;
        let mut b = Vec::with_capacity(0x4e);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&idx1(t.link.prev).to_le_bytes()); // +0
        b.extend_from_slice(&idx1(t.link.next).to_le_bytes()); // +4
        b.extend_from_slice(&t.hp.to_le_bytes()); // +8
        b.extend_from_slice(&idx1(t.sprite).to_le_bytes()); // +0xc
        for v in [f.move_target.0, f.move_target.1] {
            b.extend_from_slice(&v.to_le_bytes()); // +0x10
        }
        b.extend_from_slice(&0u32.to_le_bytes()); // +0x14 move target unit; not kept: free flingies have none
        for v in [f.next_waypoint.0, f.next_waypoint.1, f.next_target_waypoint.0, f.next_target_waypoint.1] {
            b.extend_from_slice(&v.to_le_bytes()); // +0x18, +0x1c
        }
        b.extend_from_slice(&[f.flags, f.heading, f.turn_rate, f.velocity_dir_prev]); // +0x20
        b.extend_from_slice(&f.flingy_id.to_le_bytes()); // +0x24
        b.push(0); // +0x26 not kept: never written by 0x496360
        b.push(f.movement_type); // +0x27
        b.extend_from_slice(&f.pos.0.to_le_bytes()); // +0x28
        b.extend_from_slice(&f.pos.1.to_le_bytes());
        for v in [f.exact.0, f.exact.1, f.top_speed, f.speed, f.next_speed, f.velocity.0, f.velocity.1] {
            b.extend_from_slice(&v.to_le_bytes()); // +0x2c .. +0x44
        }
        b.extend_from_slice(&f.acceleration.to_le_bytes()); // +0x48
        b.extend_from_slice(&[f.velocity_dir, f.desired_dir]); // +0x4a
        debug_assert_eq!(b.len(), 0x4e);
        o.packed(&b)
    }

    /// 0x495a50: the flingy zeroed, the saved record put back, then 0x495370 (index to
    /// pointer) and 0x494da0 (the free list holds the flingy without a sprite, the active
    /// list the one with).
    pub(super) fn load_flingies(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(n) = i.u16() else { return false };
        let mut t = crate::bw::freeflingy::FreeFlingy { sprite: NIL, ..Default::default() };
        if n != 0 {
            let Some(b) = i.packed(n as usize * 0x4e) else { return false };
            for r in b.chunks_exact(0x4e) {
                // The index selects the pool entry; the pool has one.
                if u16::from_le_bytes([r[0], r[1]]) != 0 {
                    return false;
                }
                let r = &r[2..];
                let w = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                let h = |o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
                t.link = Link { prev: slot(w(0)), next: slot(w(4)) };
                t.hp = w(8) as i32;
                t.sprite = slot(w(0xc));
                t.fl = Flingy {
                    flingy_id: u16::from_le_bytes([r[0x24], r[0x25]]),
                    move_target: (h(0x10), h(0x12)),
                    next_waypoint: (h(0x18), h(0x1a)),
                    next_target_waypoint: (h(0x1c), h(0x1e)),
                    flags: r[0x20],
                    heading: r[0x21],
                    turn_rate: r[0x22],
                    velocity_dir_prev: r[0x23],
                    movement_type: r[0x27],
                    pos: (h(0x28), h(0x2a)),
                    exact: (w(0x2c) as i32, w(0x30) as i32),
                    top_speed: w(0x34) as i32,
                    speed: w(0x38) as i32,
                    next_speed: w(0x3c) as i32,
                    velocity: (w(0x40) as i32, w(0x44) as i32),
                    acceleration: u16::from_le_bytes([r[0x48], r[0x49]]),
                    velocity_dir: r[0x4a],
                    desired_dir: r[0x4b],
                };
            }
        }
        // 0x494da0
        if t.sprite != NIL {
            t.link = Link::default();
            (self.ff_free, self.ff_active) = (List::default(), List { head: 0, tail: 0 });
        } else {
            (self.ff_free, self.ff_active) = (List { head: 0, tail: 0 }, List::default());
        }
        self.free_flingies[0] = t;
        true
    }
}
