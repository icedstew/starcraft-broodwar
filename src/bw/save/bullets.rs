//! CBullet.cpp: the bullets.

use super::{In, Out};
use crate::bw::bullet::{Bullet, MAX_BULLETS};
use crate::bw::list::{Link, List, NIL};
use crate::bw::{Data, State};

fn put32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn put16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
fn get32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
fn get16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}
/// A slot index as the savers store it: index + 1, 0 = null.
fn idx(v: u32) -> u32 {
    if v == NIL { 0 } else { v + 1 }
}
fn from_idx(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

impl State {
    /// The native 0x70-byte bullet with its pointers converted as 0x48a9a0 (via 0x479ec0)
    /// leaves them: bullet links and sprite as index + 1, units as handles.
    fn bullet_bytes(&self, b: &Bullet) -> [u8; 0x70] {
        let mut r = [0u8; 0x70];
        let unit = |u: u32| -> u32 { if u == NIL || u + 1 >= 0x6a5 { 0 } else { self.unit_handle(u) } };
        let f = &b.fl;
        put32(&mut r, 0, idx(b.link.prev));
        put32(&mut r, 4, idx(b.link.next));
        put32(&mut r, 8, b.hp as u32);
        put32(&mut r, 0xc, idx(b.sprite));
        put16(&mut r, 0x10, f.move_target.0 as u16);
        put16(&mut r, 0x12, f.move_target.1 as u16);
        put32(&mut r, 0x14, unit(b.move_target_unit));
        put16(&mut r, 0x18, f.next_waypoint.0 as u16);
        put16(&mut r, 0x1a, f.next_waypoint.1 as u16);
        put16(&mut r, 0x1c, f.next_target_waypoint.0 as u16);
        put16(&mut r, 0x1e, f.next_target_waypoint.1 as u16);
        r[0x20] = f.flags;
        r[0x21] = f.heading;
        r[0x22] = f.turn_rate;
        r[0x23] = f.velocity_dir_prev;
        put16(&mut r, 0x24, f.flingy_id);
        // not kept: +0x25, +0x26 (never set by the flingy init 0x496360)
        r[0x27] = f.movement_type;
        put16(&mut r, 0x28, f.pos.0 as u16);
        put16(&mut r, 0x2a, f.pos.1 as u16);
        put32(&mut r, 0x2c, f.exact.0 as u32);
        put32(&mut r, 0x30, f.exact.1 as u32);
        put32(&mut r, 0x34, f.top_speed as u32);
        put32(&mut r, 0x38, f.speed as u32);
        put32(&mut r, 0x3c, f.next_speed as u32);
        put32(&mut r, 0x40, f.velocity.0 as u32);
        put32(&mut r, 0x44, f.velocity.1 as u32);
        put16(&mut r, 0x48, f.acceleration);
        r[0x4a] = f.velocity_dir;
        r[0x4b] = f.desired_dir;
        r[0x4c] = b.player;
        r[0x4d] = b.state;
        r[0x4e] = b.order_state;
        r[0x4f] = b.order_signal;
        put16(&mut r, 0x50, b.unit_type);
        // not kept: +0x52, +0x54 (0 after the init 0x48bec0)
        put16(&mut r, 0x58, b.target_pos.0 as u16);
        put16(&mut r, 0x5a, b.target_pos.1 as u16);
        put32(&mut r, 0x5c, unit(b.target));
        r[0x60] = b.weapon;
        r[0x61] = b.remaining_time;
        r[0x62] = b.flags;
        r[0x63] = b.bounces;
        put32(&mut r, 0x64, unit(b.source));
        put32(&mut r, 0x68, unit(b.previous_bounce_target));
        r[0x6c] = b.spread_index;
        r
    }

    /// 0x48aeb0: the active bullets (sprite set) as u16 count, then packed records of
    /// u16 slot + the 0x70-byte bullet. (The pointer conversions 0x48a9a0 / 0x48a8d0 around
    /// it need no counterpart: Rust links are indices.)
    pub(super) fn save_bullets(&mut self, _d: &Data, o: &mut Out) -> bool {
        // Native tests sprite != 0; here a bullet is live when it is in the active list.
        let mut live = [false; MAX_BULLETS];
        let mut n = self.bullets.active.head;
        while n != NIL {
            live[n as usize] = true;
            n = self.bullets.pool[n as usize].link.next;
        }
        let count = live.iter().filter(|&&l| l).count() as u16;
        o.u16(count);
        if count == 0 {
            return true;
        }
        let mut buf = Vec::with_capacity(count as usize * 0x72);
        for (i, b) in self.bullets.pool.iter().enumerate().take(MAX_BULLETS) {
            if live[i] {
                buf.extend_from_slice(&(i as u16).to_le_bytes());
                buf.extend_from_slice(&self.bullet_bytes(b));
            }
        }
        o.packed(&buf)
    }

    /// 0x48ae40 (0x48ab00 for save version >= 3; 0x48abd0 below 3 reads u16 slot + 0x6c bytes
    /// = 0x6e-byte records, the bullet without the spread index at +0x6c, left 0): clear the bullets, read the records, resolve the
    /// pointers (0x48a8d0) and rebuild the free / active lists (0x48a720).
    /// 0x48a720: both lists relinked in slot order (inline here).
    pub(super) fn load_bullets(&mut self, _d: &Data, i: &mut In) -> bool {
        let zero = Bullet { sprite: NIL, move_target_unit: NIL, target: NIL, source: NIL, previous_bounce_target: NIL, ..Bullet::default() };
        self.bullets.pool = vec![zero.clone(); MAX_BULLETS];
        let Some(count) = i.u16() else { return false };
        let rec_len = if i.format < 3 { 0x6e } else { 0x72 }; // 0x48abd0 / 0x48ab00
        let mut recs = Vec::new();
        if count != 0 {
            let Some(buf) = i.packed(count as usize * rec_len) else { return false };
            recs = buf;
        }
        for r in recs.chunks_exact(rec_len) {
            let slot = get16(r, 0) as usize;
            if slot >= MAX_BULLETS {
                return false;
            }
            let r = &r[2..];
            let unit = |h: u32| -> u32 { self.unit_from_handle(h).unwrap_or(NIL) };
            let mut nb = zero.clone();
            nb.hp = get32(r, 8) as i32;
            nb.sprite = from_idx(get32(r, 0xc));
            let f = &mut nb.fl;
            f.move_target = (get16(r, 0x10) as i16, get16(r, 0x12) as i16);
            f.next_waypoint = (get16(r, 0x18) as i16, get16(r, 0x1a) as i16);
            f.next_target_waypoint = (get16(r, 0x1c) as i16, get16(r, 0x1e) as i16);
            f.flags = r[0x20];
            f.heading = r[0x21];
            f.turn_rate = r[0x22];
            f.velocity_dir_prev = r[0x23];
            f.flingy_id = get16(r, 0x24);
            f.movement_type = r[0x27];
            f.pos = (get16(r, 0x28) as i16, get16(r, 0x2a) as i16);
            f.exact = (get32(r, 0x2c) as i32, get32(r, 0x30) as i32);
            f.top_speed = get32(r, 0x34) as i32;
            f.speed = get32(r, 0x38) as i32;
            f.next_speed = get32(r, 0x3c) as i32;
            f.velocity = (get32(r, 0x40) as i32, get32(r, 0x44) as i32);
            f.acceleration = get16(r, 0x48);
            f.velocity_dir = r[0x4a];
            f.desired_dir = r[0x4b];
            nb.move_target_unit = unit(get32(r, 0x14));
            nb.player = r[0x4c];
            nb.state = r[0x4d];
            nb.order_state = r[0x4e];
            nb.order_signal = r[0x4f];
            nb.unit_type = get16(r, 0x50);
            nb.target_pos = (get16(r, 0x58) as i16, get16(r, 0x5a) as i16);
            nb.target = unit(get32(r, 0x5c));
            nb.weapon = r[0x60];
            nb.remaining_time = r[0x61];
            nb.flags = r[0x62];
            nb.bounces = r[0x63];
            nb.source = unit(get32(r, 0x64));
            nb.previous_bounce_target = unit(get32(r, 0x68));
            nb.spread_index = r.get(0x6c).copied().unwrap_or(0);
            self.bullets.pool[slot] = nb;
        }
        // 0x48a720: both lists in slot order (the saved links are not used).
        let pool = &mut self.bullets.pool;
        let build = |pool: &mut Vec<Bullet>, want_live: bool| -> (List, u32) {
            let mut l = List::default();
            let mut n = 0;
            let mut last = NIL;
            for s in 0..MAX_BULLETS as u32 {
                if (pool[s as usize].sprite != NIL) != want_live {
                    continue;
                }
                pool[s as usize].link = Link { prev: last, next: NIL };
                if last == NIL {
                    l.head = s;
                } else {
                    pool[last as usize].link.next = s;
                }
                l.tail = s;
                last = s;
                n += 1;
            }
            (l, n)
        };
        self.bullets.free = build(pool, false).0;
        let (active, n) = build(pool, true);
        self.bullets.active = active;
        self.bullets.count = n;
        true
    }
}
