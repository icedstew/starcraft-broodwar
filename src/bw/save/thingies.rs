//! CThingy.cpp: the thingies (0x6509d8) and the fog thingies (0x652928), 500 entries each.

use super::{In, Out};
use crate::bw::list::{Link, NIL};
use crate::bw::sprite::Thingy;
use crate::bw::{Data, List, State};

/// 1-based index as the savers (0x487540) store a pool pointer, 0 for null.
fn idx1(i: u32) -> u32 {
    if i == NIL { 0 } else { i + 1 }
}

/// 0x4875f0 on one link value: the 1-based index back to a slot (NIL for 0).
fn slot(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

/// 0x487ec0: the pool's count (u16), then its entries with a sprite as 0x12-byte records
/// (u16 index, prev, next, hp, sprite; links and sprite as 1-based indices), packed.
/// `active` is the pool's active list: the free entries have no sprite.
fn save_pool(pool: &[Thingy], active: List, o: &mut Out) -> bool {
    let mut live = vec![false; pool.len()];
    let mut t = active.head;
    while t != NIL {
        live[t as usize] = true;
        t = pool[t as usize].link.next;
    }
    let n = live.iter().filter(|&&l| l).count();
    o.u16(n as u16);
    if n == 0 {
        return true;
    }
    let mut b = Vec::with_capacity(n * 0x12);
    for (i, t) in pool.iter().enumerate().filter(|(i, _)| live[*i]) {
        b.extend_from_slice(&(i as u16).to_le_bytes());
        b.extend_from_slice(&idx1(t.link.prev).to_le_bytes());
        b.extend_from_slice(&idx1(t.link.next).to_le_bytes());
        b.extend_from_slice(&t.hp.to_le_bytes());
        b.extend_from_slice(&idx1(t.sprite).to_le_bytes());
    }
    o.packed(&b)
}

/// 0x487db0: the pool zeroed, then the saved entries put back at their indices (the links
/// are rebuilt by 0x487990 afterwards).
fn load_pool(pool: &mut Vec<Thingy>, i: &mut In) -> bool {
    let Some(n) = i.u16() else { return false };
    pool.iter_mut().for_each(|t| *t = Thingy { sprite: NIL, ..Default::default() });
    if n == 0 {
        return true;
    }
    let Some(b) = i.packed(n as usize * 0x12) else { return false };
    for r in b.chunks_exact(0x12) {
        let idx = u16::from_le_bytes([r[0], r[1]]) as usize;
        let Some(t) = pool.get_mut(idx) else { return false };
        let w = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        t.link = Link { prev: slot(w(2)), next: slot(w(6)) };
        t.hp = w(10) as i32;
        t.sprite = slot(w(14));
    }
    true
}

/// 0x487990 / 0x487470 for one pool: the free list (entries without a sprite) and the
/// active list (with one) are rebuilt in index order, relinking every entry.
fn relink(pool: &mut [Thingy]) -> (List, List) {
    let mut lists = [List::default(), List::default()];
    for i in 0..pool.len() {
        // 0x4872e0 selects the free list (sprite null), 0x4872d0 the active one.
        let l = &mut lists[(pool[i].sprite != NIL) as usize];
        pool[i].link = Link { prev: l.tail, next: NIL };
        if l.tail == NIL {
            l.head = i as u32;
        } else {
            pool[l.tail as usize].link.next = i as u32;
        }
        l.tail = i as u32;
    }
    let [free, active] = lists;
    (free, active)
}

impl State {
    /// 0x487540, then 0x487ec0 on 0x6509d8 and 0x652928 and 0x4875f0: the pointer to index
    /// conversion and back are not needed here (the links are converted while writing).
    pub(super) fn save_fog_thingies(&mut self, _d: &Data, o: &mut Out) -> bool {
        let a = save_pool(&self.thingies, self.active_thingies, o);
        let b = save_pool(&self.fog_thingies, self.active_fog_thingies, o);
        a && b
    }

    /// 0x488100: 0x487db0 on both pools, 0x4875f0 (index to pointer) and 0x487990 (the four
    /// lists rebuilt).
    pub(super) fn load_fog_thingies(&mut self, _d: &Data, i: &mut In) -> bool {
        if !load_pool(&mut self.thingies, i) || !load_pool(&mut self.fog_thingies, i) {
            return false;
        }
        (self.free_thingies, self.active_thingies) = relink(&mut self.thingies);
        (self.free_fog_thingies, self.active_fog_thingies) = relink(&mut self.fog_thingies);
        true
    }
}
