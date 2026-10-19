//! CSprite.cpp: the sprites.

use super::{In, Out};
use crate::bw::list::{List, NIL};
use crate::bw::sprite::{Sprite, MAX_SPRITES};
use crate::bw::{Data, State};

/// 0x497380 / 0x497310: a sprite or image slot as the file stores it (1-based, 0 = null).
fn idx(v: u32) -> u32 {
    if v == NIL { 0 } else { v + 1 }
}

/// The reverse: a stored 1-based index to a slot, None when it is outside `n` slots.
fn slot(v: u32, n: usize) -> Option<u32> {
    if v == 0 {
        Some(NIL)
    } else {
        ((v as usize) <= n).then(|| v - 1)
    }
}

impl State {
    /// 0x498740: writes the count of sprites in use (those with an image list), per such
    /// sprite its index (u16) and its 0x24 native bytes with sprite / image pointers as
    /// 1-based indices (0x4935f0 / 0x497380; images are slot + 1 from 0x52f568), packed
    /// (0x4c3450); then the per-row sprite lists, 0x400 bytes of heads (0x629688) and 0x400
    /// of tails (0x629288), as sprite indices + 1.
    pub(super) fn save_sprites(&mut self, _d: &Data, o: &mut Out) -> bool {
        let used: Vec<usize> = (0..MAX_SPRITES).filter(|&i| self.sprites[i].images.head != NIL).collect();
        o.u16(used.len() as u16);
        // 0x4c3450 fails for no data: with a count of 0 the game skips it (uVar5 stays 1).
        if used.len() as u16 != 0 {
            let mut b = Vec::with_capacity(used.len() * 0x26);
            for &i in &used {
                let s = &self.sprites[i];
                b.extend_from_slice(&(i as u16).to_le_bytes());
                b.extend_from_slice(&idx(s.link.prev).to_le_bytes()); // 0x0
                b.extend_from_slice(&idx(s.link.next).to_le_bytes()); // 0x4
                b.extend_from_slice(&s.sprite_id.to_le_bytes()); // 0x8
                b.extend_from_slice(&[s.owner, s.selection_index, s.visibility, s.elevation, s.flags, s.selection_timer]);
                b.extend_from_slice(&(i as u16).to_le_bytes()); // 0x10: index
                b.extend_from_slice(&[s.width, s.height]);
                b.extend_from_slice(&s.x.to_le_bytes()); // 0x14
                b.extend_from_slice(&s.y.to_le_bytes());
                b.extend_from_slice(&idx(s.main_image).to_le_bytes()); // 0x18
                b.extend_from_slice(&idx(s.images.head).to_le_bytes()); // 0x1c
                b.extend_from_slice(&idx(s.images.tail).to_le_bytes()); // 0x20
            }
            if !o.packed(&b) {
                return false;
            }
        }
        for heads in [true, false] {
            let mut a = [0u32; 0x100];
            for (k, r) in self.sprite_rows.iter().take(0x100).enumerate() {
                a[k] = idx(if heads { r.head } else { r.tail });
            }
            for v in a {
                o.u32(v);
            }
        }
        true
    }

    /// 0x498570: reads what `save_sprites` wrote: clears the pool, puts each record at its
    /// index (pointers back from 1-based indices, 0x497310), rebuilds the free list of the
    /// sprites without images in ascending order (0x497230) and reads the row lists.
    pub(super) fn load_sprites(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(n) = i.u16() else { return false };
        let nimg = self.images.len();
        for s in self.sprites.iter_mut() {
            *s = Sprite::default();
        }
        if n != 0 {
            let Some(b) = i.packed(n as usize * 0x26) else { return false };
            for r in b.chunks_exact(0x26) {
                let u16_at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let u32_at = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                let k = u16_at(0) as usize;
                if k >= MAX_SPRITES {
                    return false;
                }
                let (Some(prev), Some(next), Some(main), Some(head), Some(tail)) = (
                    slot(u32_at(2), MAX_SPRITES),
                    slot(u32_at(6), MAX_SPRITES),
                    slot(u32_at(0x1a), nimg),
                    slot(u32_at(0x1e), nimg),
                    slot(u32_at(0x22), nimg),
                ) else {
                    return false;
                };
                let s = &mut self.sprites[k];
                s.link.prev = prev;
                s.link.next = next;
                s.sprite_id = u16_at(0xa);
                s.owner = r[0xc];
                s.selection_index = r[0xd];
                s.visibility = r[0xe];
                s.elevation = r[0xf];
                s.flags = r[0x10];
                s.selection_timer = r[0x11];
                // 0x10 (index, u16 at r[0x12]) is the slot itself: set for all by 0x498570's end.
                s.width = r[0x14];
                s.height = r[0x15];
                s.x = u16_at(0x16) as i16;
                s.y = u16_at(0x18) as i16;
                s.main_image = main;
                s.images = List { head, tail };
            }
        }
        // 0x497230: the sprites without images, linked in ascending order.
        let mut free = List::default();
        let mut last = NIL;
        for k in 0..MAX_SPRITES as u32 {
            if self.sprites[k as usize].images.head != NIL {
                continue;
            }
            if last == NIL {
                free.head = k;
            } else {
                self.sprites[last as usize].link.next = k;
                self.sprites[k as usize].link.prev = last;
            }
            last = k;
        }
        free.tail = last;
        self.free_sprites = free;
        for heads in [true, false] {
            let Some(b) = i.bytes(0x400) else { return false };
            for k in 0..self.sprite_rows.len().min(0x100) {
                let v = u32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap());
                let Some(v) = slot(v, MAX_SPRITES) else { return false };
                if heads {
                    self.sprite_rows[k].head = v;
                } else {
                    self.sprite_rows[k].tail = v;
                }
            }
        }
        true
    }
}
