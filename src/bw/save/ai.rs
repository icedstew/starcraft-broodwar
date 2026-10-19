//! sai_loadsave.cpp: the pathing regions and unit paths (0x46ec40 / 0x46ea70), then the
//! AI (0x4a10f0 / 0x4a1180): military, guards, workers, buildings, towns, scripts, the
//! players' banker data and the resource areas.
//!
//! Every pool is written as 1.16.1 writes it: entries as bytes in the native layout, links
//! and pointers as 1-based indices (0 = null), unit pointers as unit handles, free entries
//! with the fields 1.16.1 clears.

use super::{In, Out};
use crate::bw::ai::{ai_region_ref, AiLink, AiNode, AiPool, AiRegion, AiScript, AiTown, BuildingAi, GuardAi, MilitaryAi, PlayerAi, SpendingRequest, WorkerAi};
use crate::bw::ai_areas::{ResourceArea, AREA_SLOTS};
use crate::bw::list::NIL;
use crate::bw::path_req::{Path, PATH_COUNT};
use crate::bw::sai::{self, ContourEntry, Contours, NeighborPtr, Region, Sai, Split};
use crate::bw::{Data, State};

// Save version 3 only (see gamedata.rs); the older formats' fix-ups (0x447e20, script
// positions + 0xb0 of 0x45b310) are not ported.

/// 0x46ec40's value for a null split cursor: native computes (0 - base - 0x2000c) / 6 from a
/// heap base; the null never happens after the regions are made, so use base 0.
const SPLIT_NONE: u32 = ((0i32 - 0x2000c) / 6) as u32;

fn p16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn p32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn g16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn g32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
/// A Rust index (NIL = null) as the native 1-based index.
fn to1(v: u32) -> u32 {
    if v == NIL {
        0
    } else {
        v + 1
    }
}
fn from1(v: u32) -> u32 {
    if v == 0 {
        NIL
    } else {
        v - 1
    }
}

/// A pool saved as 0x4036d0 / 0x402d10 / 0x403e50 / 0x404350 / 0x404620 / 0x403230 write it:
/// `n` entries of `stride` bytes (`enc` writes the fields after the links), the fields of
/// free entries in `clear` zeroed, then the free list head as a 1-based index.
fn pool_bytes<T: AiNode>(p: &AiPool<T>, n: usize, stride: usize, enc: impl Fn(&T, &mut [u8]), clear: &[(usize, usize)]) -> Vec<u8> {
    let link = |t: &T| -> AiLink { *t.clone().ai_link() };
    let mut free = vec![false; n];
    let mut f = p.free;
    while (f as usize) < n && !free[f as usize] {
        free[f as usize] = true;
        f = link(&p.items[f as usize]).next;
    }
    let mut b = vec![0u8; n * stride + 4];
    for i in 0..n {
        let Some(t) = p.items.get(i) else { continue };
        let e = &mut b[i * stride..(i + 1) * stride];
        enc(t, e);
        let l = link(t);
        p32(e, 0, to1(l.next));
        p32(e, 4, to1(l.prev));
        if free[i] {
            for &(a, z) in clear {
                e[a..z].fill(0);
            }
        }
    }
    p32(&mut b, n * stride, to1(p.free));
    b
}

/// The reverse (0x403780 / 0x402fb0 / 0x4041f0 / 0x404410 / 0x4046d0 / 0x403310 ...).
fn pool_load<T: AiNode>(b: &[u8], n: usize, stride: usize, dec: impl Fn(&[u8]) -> T) -> Option<AiPool<T>> {
    let mut items = Vec::with_capacity(n);
    for i in 0..n {
        let e = &b[i * stride..(i + 1) * stride];
        let (next, prev) = (g32(e, 0), g32(e, 4));
        if next as usize > n || prev as usize > n {
            return None;
        }
        let mut t = dec(e);
        *t.ai_link() = AiLink { next: from1(next), prev: from1(prev) };
        items.push(t);
    }
    let free = g32(b, n * stride);
    if free as usize > n {
        return None;
    }
    Some(AiPool { items, free: from1(free) })
}

/// 0x437290 inverse (the AI region pointer 0x438240 / 0x4328e0 store as `player * 0x9c4 + 1 +
/// index`): None for a null or bad value.
fn region_dec(v: u32, count: u32) -> Option<(u8, u16)> {
    if v == 0 {
        return None;
    }
    let p = (v / 0x9c4) & 0xff;
    let r = v.wrapping_sub(p * 0x9c4);
    (p < 8 && r != 0 && r <= count).then(|| (p as u8, (r - 1) as u16))
}

impl State {
    /// The unit handle 0x438240 stores for a unit pointer (0 = none).
    fn ai_handle(&self, u: u32) -> u32 {
        if (u as usize) < self.units.len() {
            self.unit_handle(u)
        } else {
            0
        }
    }

    /// The unit slot of a handle as the loaders compute it (`0x59cb58 + (h & 0x7ff) * 0x150`).
    fn ai_unit(&self, h: u32) -> u32 {
        let s = h & 0x7ff;
        if s == 0 || s as usize > self.units.len() {
            NIL
        } else {
            s - 1
        }
    }

    /// The AI region value of 0x438240: `player * 0x9c4 + 1 + index`, 0 when the index is past
    /// the pathing regions.
    fn region_enc(&self, (p, r): (u8, u16)) -> u32 {
        if p < 8 && (r as i16) >= 0 && r < self.sai.region_count {
            p as u32 * 0x9c4 + 1 + r as u32
        } else {
            0
        }
    }

    /// 0x46ec40's buffer: 0xe (plus 1 << 16 with contours), the path free list head, the
    /// SAI as 0x46e990 leaves it, the contours (0x38 bytes and four lists), the 1024 paths.
    fn path_buffer(&self) -> Vec<u8> {
        let s = &self.sai;
        let mut sb = sai::layout(s);
        // 0x46e990: pointers become offsets.
        p32(&mut sb, 4, s.neighbor_cursor as u32);
        p32(&mut sb, 8, s.split_cursor.map_or(SPLIT_NONE, |n| n as u32));
        for r in 0..s.regions.len() {
            let o = 0x449fc + r * 0x40 + 0xc;
            let p = g32(&sb, o);
            if (r as i16) < (s.region_count as i16) {
                let v = if p == 0x449fc + r as u32 * 0x40 + 0x2c { -1 } else { (p as i32).wrapping_sub(0x92bfc) >> 1 };
                p32(&mut sb, o, v as u32);
            }
        }
        let has = s.contours.is_some();
        let mut b = Vec::new();
        b.extend_from_slice(&(0xeu32 | (has as u32) << 16).to_le_bytes());
        b.extend_from_slice(&self.free_path.to_le_bytes());
        b.extend_from_slice(&sb);
        if let Some(c) = &s.contours {
            let mut h = [0u8; 0x38];
            // +0x0..0x10: the lists' heap pointers; not kept: 0.
            for k in 0..4 {
                p16(&mut h, 0x10 + k * 2, c.lists[k].len() as u16);
                p16(&mut h, 0x18 + k * 2, c.caps[k] as u16);
                p16(&mut h, 0x20 + k * 2, c.dims_a[k] as u16);
                p16(&mut h, 0x28 + k * 2, c.unk28[k]);
                p16(&mut h, 0x30 + k * 2, c.dims_b[k] as u16);
            }
            b.extend_from_slice(&h);
            for l in &c.lists {
                for e in l {
                    for k in 0..3 {
                        b.extend_from_slice(&e.v[k].to_le_bytes());
                    }
                    b.push(e.dir);
                    b.push(e.flags);
                }
            }
        }
        for i in 0..PATH_COUNT {
            b.extend_from_slice(&self.paths.get(i).map_or([0; 0x80], |p| p.raw()));
        }
        b
    }

    /// 0x46ea70: the buffer back into the SAI and the path pool.
    fn path_load(&mut self, b: &[u8]) -> bool {
        if b.len() < 8 + 0x97a20 || g16(b, 0) != 0xe {
            return false;
        }
        let has = b[2] != 0;
        let sb = &b[8..8 + 0x97a20];
        let mut pos = 8 + 0x97a20;
        let mut s = Sai::new();
        s.region_count = g16(sb, 0);
        s.neighbor_cursor = g32(sb, 4) as u16;
        let sc = g32(sb, 8);
        s.split_cursor = (sc != SPLIT_NONE).then_some(sc as u16);
        for (i, t) in s.tiles.iter_mut().enumerate() {
            *t = g16(sb, 0xc + i * 2);
        }
        for (i, sp) in s.splits.iter_mut().enumerate() {
            let o = 0x2000c + i * 6;
            *sp = Split { mask: g16(sb, o), rgn1: g16(sb, o + 2), rgn2: g16(sb, o + 4) };
        }
        let count = s.region_count as i16;
        for (r, rg) in s.regions.iter_mut().enumerate() {
            let o = 0x449fc + r * 0x40;
            let v = g32(sb, o + 0xc) as i32;
            // 0x46e920: -1 is the region's own inline list.
            let p = if (r as i16) < count {
                if v == -1 {
                    o as i32 + 0x2c
                } else {
                    0x92bfci32.wrapping_add(v.wrapping_mul(2))
                }
            } else {
                v
            };
            let neighbors = if p == 0 {
                NeighborPtr::Null
            } else if p >= 0x449fc + 0x2c && (p - 0x449fc - 0x2c) % 0x40 == 0 && ((p - 0x449fc - 0x2c) / 0x40) < 5000 {
                NeighborPtr::Inline(((p - 0x449fc - 0x2c) / 0x40) as u16)
            } else if p >= 0x92bfc && (p - 0x92bfc) % 2 == 0 && ((p - 0x92bfc) / 2) < 10000 {
                NeighborPtr::Shared(((p - 0x92bfc) / 2) as u16)
            } else {
                NeighborPtr::Null
            };
            let mut inline_neighbors = [0u16; 10];
            for (k, n) in inline_neighbors.iter_mut().enumerate() {
                *n = g16(sb, o + 0x2c + k * 2);
            }
            let mut unk22 = [0u8; 10];
            unk22.copy_from_slice(&sb[o + 0x22..o + 0x2c]);
            *rg = Region {
                flags: g16(sb, o),
                group: g16(sb, o + 2),
                tile_count: g16(sb, o + 4),
                walkable_neighbors: sb[o + 6],
                neighbor_count: sb[o + 7],
                unk8: g32(sb, o + 8),
                neighbors,
                center: (g32(sb, o + 0x10) as i32, g32(sb, o + 0x14) as i32),
                rect: [g16(sb, o + 0x18) as i16, g16(sb, o + 0x1a) as i16, g16(sb, o + 0x1c) as i16, g16(sb, o + 0x1e) as i16],
                properties: sb[o + 0x20],
                zero_neighbors: sb[o + 0x21],
                unk22,
                inline_neighbors,
            };
        }
        for (i, n) in s.neighbor_buf.iter_mut().enumerate().take(10000) {
            *n = g16(sb, 0x92bfc + i * 2);
        }
        if has {
            if b.len() < pos + 0x38 {
                return false;
            }
            let h = &b[pos..pos + 0x38];
            pos += 0x38;
            let mut c = Contours::default();
            let mut counts = [0usize; 4];
            for k in 0..4 {
                counts[k] = g16(h, 0x10 + k * 2) as usize;
                c.caps[k] = g16(h, 0x18 + k * 2) as i16;
                c.dims_a[k] = g16(h, 0x20 + k * 2) as i16;
                c.unk28[k] = g16(h, 0x28 + k * 2);
                c.dims_b[k] = g16(h, 0x30 + k * 2) as i16;
            }
            for k in 0..4 {
                if b.len() < pos + counts[k] * 8 {
                    return false;
                }
                c.lists[k] = b[pos..pos + counts[k] * 8]
                    .chunks_exact(8)
                    .map(|e| ContourEntry { v: [g16(e, 0) as i16, g16(e, 2) as i16, g16(e, 4) as i16], dir: e[6], flags: e[7] })
                    .collect();
                pos += counts[k] * 8;
            }
            s.contours = Some(c);
        }
        if b.len() < pos + PATH_COUNT * 0x80 {
            return false;
        }
        let free_path = g32(b, 4);
        if free_path as usize > PATH_COUNT {
            return false;
        }
        self.free_path = free_path;
        self.paths = (0..PATH_COUNT).map(|i| Path::from_raw(&b[pos + i * 0x80..pos + (i + 1) * 0x80])).collect();
        self.sai = s;
        true
    }

    /// 0x46ec40, as written by 0x4d02d0: the buffer's size, then the buffer packed.
    fn save_paths(&self, o: &mut Out) -> bool {
        let b = self.path_buffer();
        o.u32(b.len() as u32);
        o.packed(&b)
    }

    // ---- 0x4392f0: military AIs and the AI regions

    /// 0x438240: a military AI as its 0x14 bytes.
    fn military_enc(&self, m: &MilitaryAi, e: &mut [u8]) {
        e[8] = m.ai_type;
        e[9] = m.unk9;
        p16(e, 0xa, m.unka);
        p32(e, 0xc, self.ai_handle(m.parent));
        p32(e, 0x10, self.region_enc(m.region));
    }

    fn military_dec(&self, e: &[u8], count: u32) -> MilitaryAi {
        MilitaryAi {
            link: AiLink::default(),
            ai_type: e[8],
            unk9: e[9],
            unka: g16(e, 0xa),
            parent: self.ai_unit(g32(e, 0xc)),
            region: region_dec(g32(e, 0x10), count).unwrap_or((0, 0)),
        }
    }

    /// 0x403480: an AI region as its 0x34 bytes (+0x2c, the military pool's base, is 0).
    fn region_bytes(&self, r: Option<&AiRegion>) -> [u8; 0x34] {
        let mut e = [0u8; 0x34];
        let Some(r) = r else { return e };
        p16(&mut e, 0, r.id);
        p16(&mut e, 2, r.target_region_id);
        e[4] = r.player;
        e[5] = r.state;
        e[6] = r.defense_priority_add;
        e[7] = r.unk_count;
        e[8] = r.flags;
        e[9] = r.unk;
        for (k, v) in [
            r.ground_unit_count,
            r.needed_ground_strength,
            r.needed_air_strength,
            r.local_military_ground_strength,
            r.local_military_air_strength,
            r.all_military_ground_strength,
            r.all_military_air_strength,
            r.enemy_air_strength,
            r.enemy_ground_strength,
        ]
        .into_iter()
        .enumerate()
        {
            p16(&mut e, 0xa + k * 2, v);
        }
        p32(&mut e, 0x1c, self.ai_handle(r.air_target));
        p32(&mut e, 0x20, self.ai_handle(r.ground_target));
        p32(&mut e, 0x24, self.ai_handle(r.slowest_military));
        p32(&mut e, 0x28, self.ai_handle(r.detector));
        p32(&mut e, 0x30, to1(r.military));
        e
    }

    /// 0x4035b0 inverse.
    fn region_from(&self, e: &[u8]) -> AiRegion {
        AiRegion {
            id: g16(e, 0),
            target_region_id: g16(e, 2),
            player: e[4],
            state: e[5],
            defense_priority_add: e[6],
            unk_count: e[7],
            flags: e[8],
            unk: e[9],
            ground_unit_count: g16(e, 0xa),
            needed_ground_strength: g16(e, 0xc),
            needed_air_strength: g16(e, 0xe),
            local_military_ground_strength: g16(e, 0x10),
            local_military_air_strength: g16(e, 0x12),
            all_military_ground_strength: g16(e, 0x14),
            all_military_air_strength: g16(e, 0x16),
            enemy_air_strength: g16(e, 0x18),
            enemy_ground_strength: g16(e, 0x1a),
            air_target: self.ai_unit(g32(e, 0x1c)),
            ground_target: self.ai_unit(g32(e, 0x20)),
            slowest_military: self.ai_unit(g32(e, 0x24)),
            detector: self.ai_unit(g32(e, 0x28)),
            military: from1(g32(e, 0x30)),
        }
    }

    /// 0x4392f0: the region count, the military pool, the regions of players 7 .. 0.
    fn save_ai_regions(&self, o: &mut Out) -> bool {
        let n = self.sai.region_count as i16 as i32;
        let mut ok = o.packed(&n.to_le_bytes());
        let pool = pool_bytes(&self.ai.military, 1000, 0x14, |m, e| self.military_enc(m, e), &[(8, 0x14)]);
        ok &= o.packed(&pool);
        for p in (0..8).rev() {
            let mut b = Vec::new();
            for r in 0..n.max(0) as usize {
                b.extend_from_slice(&self.region_bytes(self.ai.regions[p].get(r)));
            }
            ok &= o.packed(&b);
        }
        ok
    }

    /// 0x439220
    fn load_ai_regions(&mut self, i: &mut In) -> bool {
        let Some(h) = i.packed(4) else { return false };
        let n = g32(&h, 0);
        if n as i32 > 5000 {
            return false;
        }
        let Some(pool) = i.packed(0x4e24) else { return false };
        let Some(military) = pool_load(&pool, 1000, 0x14, |e| self.military_dec(e, n)) else { return false };
        let mut regions = vec![Vec::new(); 8];
        for p in (0..8).rev() {
            if n == 0 {
                // 0x4c3280 refuses a zero length.
                return false;
            }
            let Some(b) = i.packed(n as usize * 0x34) else { return false };
            regions[p] = b.chunks_exact(0x34).map(|e| self.region_from(e)).collect();
        }
        self.ai.military = military;
        self.ai.regions = regions;
        self.ai.region_step = 0;
        self.ai.region_player = 0;
        true
    }

    // ---- 0x462bf0: guards

    fn guard_enc(&self, g: &GuardAi, e: &mut [u8]) {
        e[8] = g.ai_type;
        e[9] = g.times_died;
        p32(e, 0xc, self.ai_handle(g.parent));
        p16(e, 0x10, g.unit_id);
        p16(e, 0x12, g.home.0 as u16);
        p16(e, 0x14, g.home.1 as u16);
        p16(e, 0x16, g.other_home.0 as u16);
        p16(e, 0x18, g.other_home.1 as u16);
        // +0x1a: not kept: 0.
        p32(e, 0x1c, g.previous_update);
    }

    fn guard_dec(&self, e: &[u8]) -> GuardAi {
        GuardAi {
            link: AiLink::default(),
            ai_type: e[8],
            times_died: e[9],
            parent: self.ai_unit(g32(e, 0xc)),
            unit_id: g16(e, 0x10),
            home: (g16(e, 0x12) as i16, g16(e, 0x14) as i16),
            other_home: (g16(e, 0x16) as i16, g16(e, 0x18) as i16),
            previous_update: g32(e, 0x1c),
        }
    }

    /// The 8 list heads as 0x40 bytes (base pointer 0, head 1-based), for the guards, towns.
    fn heads_bytes(heads: &[u32; 8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x40];
        for (p, &h) in heads.iter().enumerate() {
            p32(&mut b, p * 8 + 4, to1(h));
        }
        b
    }

    fn heads_load(b: &[u8], n: usize) -> Option<[u32; 8]> {
        let mut h = [NIL; 8];
        for (p, v) in h.iter_mut().enumerate() {
            let x = g32(b, p * 8 + 4);
            if x as usize > n {
                return None;
            }
            *v = from1(x);
        }
        Some(h)
    }

    /// 0x462bf0: the guard pool and the players' guard lists.
    fn save_ai_guards(&self, o: &mut Out) -> bool {
        let mut ok = o.packed(&pool_bytes(&self.ai.guards, 1000, 0x20, |g, e| self.guard_enc(g, e), &[(8, 9), (0xc, 0x1a), (0x1c, 0x20)]));
        ok &= o.packed(&Self::heads_bytes(&self.ai.guard_lists));
        ok
    }

    /// 0x462ac0
    fn load_ai_guards(&mut self, i: &mut In) -> bool {
        let Some(b) = i.packed(0x7d04) else { return false };
        let Some(pool) = pool_load(&b, 1000, 0x20, |e| self.guard_dec(e)) else { return false };
        let Some(l) = i.packed(0x40) else { return false };
        let Some(lists) = Self::heads_load(&l, 1000) else { return false };
        self.ai.guards = pool;
        self.ai.guard_lists = lists;
        true
    }

    // ---- 0x432da0: workers, buildings, towns

    fn worker_enc(&self, w: &WorkerAi, e: &mut [u8]) {
        e[8] = w.ai_type;
        e[9] = w.target_resource;
        e[0xa] = w.reassign_count;
        e[0xb] = w.wait_timer;
        p32(e, 0xc, w.last_update_second);
        p32(e, 0x10, self.ai_handle(w.parent));
        p32(e, 0x14, to1(w.town));
    }

    fn worker_dec(&self, e: &[u8]) -> WorkerAi {
        WorkerAi {
            link: AiLink::default(),
            ai_type: e[8],
            target_resource: e[9],
            reassign_count: e[0xa],
            wait_timer: e[0xb],
            last_update_second: g32(e, 0xc),
            parent: self.ai_unit(g32(e, 0x10)),
            town: from1(g32(e, 0x14)),
        }
    }

    /// 0x4328e0: a building AI; each queue value is converted by its type (1 an AI region,
    /// 2 a guard, 3 a town, others as is).
    fn building_enc(&self, b: &BuildingAi, e: &mut [u8]) {
        e[8] = b.ai_type;
        e[9..14].copy_from_slice(&b.train_queue_types);
        // +0xe, +0xf: not kept: 0.
        p32(e, 0x10, self.ai_handle(b.parent));
        p32(e, 0x14, to1(b.town));
        for k in 0..5 {
            let v = b.train_queue_values[k];
            let n = match b.train_queue_types[k] {
                1 => self.region_enc(((v >> 16) as u8, v as u16)),
                2 => {
                    if v < 1000 {
                        v + 1
                    } else {
                        0
                    }
                }
                3 => to1(v),
                _ => v,
            };
            p32(e, 0x18 + k * 4, n);
        }
    }

    /// 0x432810
    fn building_dec(&self, e: &[u8], count: u32) -> BuildingAi {
        let mut types = [0u8; 5];
        types.copy_from_slice(&e[9..14]);
        let mut values = [0u32; 5];
        for k in 0..5 {
            let v = g32(e, 0x18 + k * 4);
            values[k] = match types[k] {
                0 => v,
                1 => region_dec(v, count).map_or(0, |(p, r)| ai_region_ref(p, r)),
                2 => from1(v),
                3 => from1(v),
                _ => 0,
            };
        }
        BuildingAi {
            link: AiLink::default(),
            ai_type: e[8],
            train_queue_types: types,
            parent: self.ai_unit(g32(e, 0x10)),
            town: from1(g32(e, 0x14)),
            train_queue_values: values,
        }
    }

    /// 0x403ae0: a town; the worker and building list heads are 1-based indices.
    fn town_enc(&self, t: &AiTown, e: &mut [u8]) {
        p32(e, 0xc, to1(t.workers));
        p32(e, 0x14, to1(t.buildings));
        e[0x18] = t.player;
        e[0x19] = t.inited;
        e[0x1a] = t.worker_count;
        e[0x1b] = t.worker_limit;
        e[0x1c] = t.resource_area;
        e[0x1d] = t.resource_units_not_set;
        e[0x1e] = t.in_battle;
        e[0x1f] = t.unk1f;
        p16(e, 0x20, t.position.0 as u16);
        p16(e, 0x22, t.position.1 as u16);
        p32(e, 0x24, self.ai_handle(t.main_building));
        p32(e, 0x28, self.ai_handle(t.building_scv));
        p32(e, 0x2c, self.ai_handle(t.mineral));
        for k in 0..3 {
            p32(e, 0x30 + k * 4, self.ai_handle(t.gas_buildings[k]));
        }
        for (k, u) in t.town_units.iter().enumerate() {
            e[0x3c + k * 4] = u.0;
            e[0x3d + k * 4] = u.1;
            p16(e, 0x3e + k * 4, u.2);
        }
    }

    /// 0x403cb0
    fn town_dec(&self, e: &[u8]) -> AiTown {
        let mut town_units = [(0u8, 0u8, 0u16); 0x64];
        for (k, u) in town_units.iter_mut().enumerate() {
            *u = (e[0x3c + k * 4], e[0x3d + k * 4], g16(e, 0x3e + k * 4));
        }
        AiTown {
            link: AiLink::default(),
            workers: from1(g32(e, 0xc)),
            buildings: from1(g32(e, 0x14)),
            player: e[0x18],
            inited: e[0x19],
            worker_count: e[0x1a],
            worker_limit: e[0x1b],
            resource_area: e[0x1c],
            resource_units_not_set: e[0x1d],
            in_battle: e[0x1e],
            unk1f: e[0x1f],
            position: (g16(e, 0x20) as i16, g16(e, 0x22) as i16),
            main_building: self.ai_unit(g32(e, 0x24)),
            building_scv: self.ai_unit(g32(e, 0x28)),
            mineral: self.ai_unit(g32(e, 0x2c)),
            gas_buildings: [self.ai_unit(g32(e, 0x30)), self.ai_unit(g32(e, 0x34)), self.ai_unit(g32(e, 0x38))],
            town_units,
        }
    }

    /// 0x432da0
    fn save_ai_towns(&self, o: &mut Out) -> bool {
        let ai = &self.ai;
        let mut ok = o.packed(&pool_bytes(&ai.workers, 1000, 0x18, |w, e| self.worker_enc(w, e), &[(8, 0x18)]));
        ok &= o.packed(&pool_bytes(&ai.buildings, 1000, 0x2c, |b, e| self.building_enc(b, e), &[(8, 0xe), (0x10, 0x2c)]));
        ok &= o.packed(&pool_bytes(&ai.towns, 100, 0x1cc, |t, e| self.town_enc(t, e), &[(0x18, 0x1cc)]));
        ok &= o.packed(&Self::heads_bytes(&ai.town_lists));
        ok
    }

    /// 0x432c10
    fn load_ai_towns(&mut self, i: &mut In) -> bool {
        let n = self.sai.region_count as u32;
        let Some(w) = i.packed(0x5dc4) else { return false };
        let Some(workers) = pool_load(&w, 1000, 0x18, |e| self.worker_dec(e)) else { return false };
        let Some(b) = i.packed(0xabe4) else { return false };
        let Some(buildings) = pool_load(&b, 1000, 0x2c, |e| self.building_dec(e, n)) else { return false };
        let Some(t) = i.packed(0xb3b4) else { return false };
        let Some(towns) = pool_load(&t, 100, 0x1cc, |e| self.town_dec(e)) else { return false };
        let Some(l) = i.packed(0x40) else { return false };
        let Some(lists) = Self::heads_load(&l, 100) else { return false };
        self.ai.workers = workers;
        self.ai.buildings = buildings;
        self.ai.towns = towns;
        self.ai.town_lists = lists;
        self.ai.town_update_player = 0;
        true
    }

    // ---- 0x447ed0: the players' banker data

    /// A player's 0x4e8 bytes; +0x4d4 the free medic as a unit handle.
    fn player_bytes(&self, a: &PlayerAi, e: &mut [u8]) {
        for (k, v) in [a.mineral_need, a.gas_need, a.supply_need, a.minerals_available, a.gas_available, a.supply_available].into_iter().enumerate() {
            p32(e, k * 4, v);
        }
        for (k, r) in a.requests.iter().enumerate() {
            let o = 0x18 + k * 8;
            e[o] = r.priority;
            e[o + 1] = r.ty;
            p16(e, o + 2, r.id);
            // A town request's value is a town pointer in 1.16.1; ours is not converted (the
            // loader clears the requests anyway).
            p32(e, o + 4, r.val);
        }
        e[0x210] = a.request_count;
        e[0x211] = a.build_cooldown;
        e[0x212] = a.nuke_rate_minutes;
        e[0x213] = a.unk_attack;
        p32(e, 0x214, a.previous_nuke_timer);
        p16(e, 0x218, a.flags);
        p16(e, 0x21a, a.panic_script_pos);
        p16(e, 0x21c, a.max_region_strength_need);
        p16(e, 0x21e, a.attack_grouping_region);
        p16(e, 0x220, a.train_unit_id);
        e[0x222] = a.default_min_strength_for_regions;
        e[0x223] = a.defense_priority_base;
        p32(e, 0x224, a.previous_building_hit_second);
        p32(e, 0x228, a.last_attack_second);
        e[0x22c] = a.strategic_suicide_mission_cooldown;
        e[0x22d] = a.spell_cooldown;
        e[0x22e] = a.attack_failed;
        e[0x22f] = a.difficulty;
        for (k, v) in a.attack_force.iter().enumerate() {
            p16(e, 0x230 + k * 2, *v);
        }
        for (k, v) in a.defense_units.iter().flatten().enumerate() {
            p16(e, 0x2b0 + k * 2, *v);
        }
        e[0x3f0..0x4d4].copy_from_slice(&a.build_limits);
        p32(e, 0x4d4, self.ai_handle(a.free_medic));
        for k in 0..4 {
            p32(e, 0x4d8 + k * 4, a.gencmd_area[k] as u32);
        }
    }

    /// 0x448230 (version 3): a player back; the requests are cleared, not read.
    fn player_dec(&self, e: &[u8]) -> PlayerAi {
        let mut a = PlayerAi {
            mineral_need: g32(e, 0),
            gas_need: g32(e, 4),
            supply_need: g32(e, 8),
            minerals_available: g32(e, 0xc),
            gas_available: g32(e, 0x10),
            supply_available: g32(e, 0x14),
            requests: [SpendingRequest::default(); 0x3f],
            request_count: 0,
            build_cooldown: e[0x211],
            nuke_rate_minutes: e[0x212],
            unk_attack: e[0x213],
            previous_nuke_timer: g32(e, 0x214),
            flags: g16(e, 0x218),
            panic_script_pos: g16(e, 0x21a),
            max_region_strength_need: g16(e, 0x21c),
            attack_grouping_region: g16(e, 0x21e),
            train_unit_id: g16(e, 0x220),
            default_min_strength_for_regions: e[0x222],
            defense_priority_base: e[0x223],
            previous_building_hit_second: g32(e, 0x224),
            last_attack_second: g32(e, 0x228),
            strategic_suicide_mission_cooldown: e[0x22c],
            spell_cooldown: e[0x22d],
            attack_failed: e[0x22e],
            difficulty: e[0x22f],
            attack_force: [0; 0x40],
            defense_units: [[0; 0x14]; 8],
            build_limits: [0; 0xe4],
            free_medic: self.ai_unit(g32(e, 0x4d4)),
            gencmd_area: [0; 4],
        };
        for (k, v) in a.attack_force.iter_mut().enumerate() {
            *v = g16(e, 0x230 + k * 2);
        }
        for (k, v) in a.defense_units.iter_mut().flatten().enumerate() {
            *v = g16(e, 0x2b0 + k * 2);
        }
        a.build_limits.copy_from_slice(&e[0x3f0..0x4d4]);
        for k in 0..4 {
            a.gencmd_area[k] = g32(e, 0x4d8 + k * 4) as i32;
        }
        a
    }

    // ---- 0x45b3a0: scripts; 0x4452d0: resource areas

    fn script_enc(&self, s: &AiScript, e: &mut [u8]) {
        p32(e, 8, s.pos);
        p32(e, 0xc, s.wait);
        p32(e, 0x10, s.player);
        for k in 0..4 {
            p32(e, 0x14 + k * 4, s.area[k] as u32);
        }
        p32(e, 0x24, s.center.0 as u32);
        p32(e, 0x28, s.center.1 as u32);
        p32(e, 0x2c, to1(s.town));
        p32(e, 0x30, s.flags);
    }

    fn script_dec(e: &[u8]) -> AiScript {
        AiScript {
            link: AiLink::default(),
            pos: g32(e, 8),
            wait: g32(e, 0xc),
            player: g32(e, 0x10),
            area: [g32(e, 0x14) as i32, g32(e, 0x18) as i32, g32(e, 0x1c) as i32, g32(e, 0x20) as i32],
            center: (g32(e, 0x24) as i32, g32(e, 0x28) as i32),
            town: from1(g32(e, 0x2c)),
            flags: g32(e, 0x30),
        }
    }

    fn areas_bytes(&self) -> Vec<u8> {
        let mut b = vec![0u8; 0x2ee8];
        for (k, a) in self.areas.iter().enumerate().take(AREA_SLOTS) {
            let e = &mut b[k * 0x30..(k + 1) * 0x30];
            p16(e, 0, a.pos.0 as u16);
            p16(e, 2, a.pos.1 as u16);
            e[4] = a.mineral_fields;
            e[5] = a.geysers;
            e[6] = a.start;
            e[7] = a.flags;
            p32(e, 8, a.minerals);
            p32(e, 0xc, a.gas);
            // +0x10 .. +0x30: not kept: 0.
        }
        p32(&mut b, 0x2ee0, self.area_count);
        p32(&mut b, 0x2ee4, self.area_timer);
        b
    }

    /// 0x4a10f0: the AI after the paths.
    fn save_ai_rest(&self, o: &mut Out) -> bool {
        let mut ok = self.save_ai_regions(o);
        ok &= self.save_ai_guards(o);
        ok &= self.save_ai_towns(o);
        // 0x447ed0
        let mut pb = vec![0u8; 0x2740];
        for p in 0..8 {
            self.player_bytes(&self.ai.players[p], &mut pb[p * 0x4e8..(p + 1) * 0x4e8]);
        }
        ok &= o.packed(&pb);
        // 0x45b3a0
        let sc = pool_bytes(&self.ai.scripts, 100, 0x34, |s, e| self.script_enc(s, e), &[(8, 0x34)]);
        ok &= o.packed(&sc);
        let mut head = [0u8; 8];
        head[4..].copy_from_slice(&to1(self.ai.active_scripts).to_le_bytes());
        ok &= o.packed(&head);
        // 0x4452d0
        ok &= o.packed(&self.areas_bytes());
        ok
    }

    /// 0x4a1180: the AI after the paths (save version 3).
    fn load_ai_rest(&mut self, i: &mut In) -> bool {
        if !(self.load_ai_regions(i) && self.load_ai_guards(i) && self.load_ai_towns(i)) {
            return false;
        }
        // 0x448230
        let Some(pb) = i.packed(0x2740) else { return false };
        let players: Vec<PlayerAi> = (0..8).map(|p| self.player_dec(&pb[p * 0x4e8..(p + 1) * 0x4e8])).collect();
        // 0x45b310
        let Some(sb) = i.packed(0x1454) else { return false };
        let Some(scripts) = pool_load(&sb, 100, 0x34, Self::script_dec) else { return false };
        let Some(h) = i.packed(8) else { return false };
        let active = g32(&h, 4);
        if active > 100 {
            return false;
        }
        // 0x4452f0
        let Some(ab) = i.packed(0x2ee8) else { return false };
        self.ai.players = players;
        self.ai.scripts = scripts;
        self.ai.active_scripts = from1(active);
        self.areas = (0..AREA_SLOTS)
            .map(|k| {
                let e = &ab[k * 0x30..(k + 1) * 0x30];
                ResourceArea { pos: (g16(e, 0) as i16, g16(e, 2) as i16), mineral_fields: e[4], geysers: e[5], start: e[6], flags: e[7], minerals: g32(e, 8), gas: g32(e, 0xc) }
            })
            .collect();
        self.area_count = g32(&ab, 0x2ee0);
        self.area_timer = g32(&ab, 0x2ee4);
        true
    }

    /// 0x46ec40: the paths and SAI buffer (its size first), then 0x4a10f0.
    pub(super) fn save_ai(&mut self, _d: &Data, o: &mut Out) -> bool {
        let ok = self.save_paths(o);
        self.save_ai_rest(o) & ok
    }

    /// 0x46ea70: the paths and SAI buffer, then 0x4a1180.
    pub(super) fn load_ai(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(size) = i.u32() else { return false };
        let Some(b) = i.packed(size as usize) else { return false };
        self.path_load(&b) && self.load_ai_rest(i)
    }
}
