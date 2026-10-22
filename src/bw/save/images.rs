//! CImage.cpp: the images (0x52f568 pool).

use super::{In, Out};
use crate::bw::list::{Link, List, NIL};
use crate::bw::sprite::{Image, IscriptState, MAX_IMAGES};
use crate::bw::{Data, State};

/// 1-based index of a pool slot, 0 for none (0x4d6330 on +0 / +4 / +0x3c).
fn idx1(v: u32) -> u32 {
    if v == NIL { 0 } else { v + 1 }
}

/// 0..=NIL from a 1-based index (0x4d5600).
fn from1(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

impl State {
    /// 0x4d6330: the file form of an image's +0x2c: the 1-based index of the first
    /// images.dat entry with the same GRP (0x51ced0 holds the loaded GRP pointers; equal
    /// files share one), 0 for no GRP.
    fn image_grp_index(&self, d: &Data, i: &Image) -> u32 {
        if !i.in_use {
            return 0;
        }
        let g = &d.dat.images.grp;
        let me = g.get(i.image_id as usize).copied();
        // 999 entries with Brood War (0x6d11e4), else 0x2f3.
        (0..g.len().min(999)).find(|&j| Some(g[j]) == me).map_or(0, |j| j as u32 + 1)
    }

    /// 0x4d64c0: the in-use images of the 5000 pool as u16 count, then packed
    /// (count * 0x42 bytes): u16 pool index + the 0x40-byte image with its pointers
    /// converted by 0x4d6330 (+0x34 / +0x38 zeroed).
    pub(super) fn save_images(&mut self, d: &Data, o: &mut Out) -> bool {
        let used: Vec<usize> = (0..MAX_IMAGES).filter(|&n| self.images[n].in_use).collect();
        o.u16(used.len() as u16);
        if used.is_empty() {
            return true;
        }
        let mut b = Vec::with_capacity(used.len() * 0x42);
        for n in used {
            let i = &self.images[n];
            b.extend_from_slice(&(n as u16).to_le_bytes());
            let mut r = [0u8; 0x40];
            r[0..4].copy_from_slice(&idx1(i.link.prev).to_le_bytes());
            r[4..8].copy_from_slice(&idx1(i.link.next).to_le_bytes());
            r[8..10].copy_from_slice(&i.image_id.to_le_bytes());
            r[0xa] = i.modifier;
            r[0xb] = i.direction;
            r[0xc..0xe].copy_from_slice(&i.flags.to_le_bytes());
            r[0xe] = i.x as u8;
            r[0xf] = i.y as u8;
            // not kept: +0x10 (GRP bounds, 4 x u16), +0x24 (map position) and +0x28
            // (screen position), render caches refreshed when drawn: 0.
            r[0x18..0x1a].copy_from_slice(&i.iscript.header.to_le_bytes());
            r[0x1a..0x1c].copy_from_slice(&i.iscript.pc.to_le_bytes());
            r[0x1c..0x1e].copy_from_slice(&i.iscript.ret.to_le_bytes());
            r[0x1e] = i.iscript.anim;
            r[0x1f] = i.iscript.wait;
            r[0x20..0x22].copy_from_slice(&i.frameset.to_le_bytes());
            r[0x22..0x24].copy_from_slice(&i.frame.to_le_bytes());
            r[0x2c..0x30].copy_from_slice(&self.image_grp_index(d, i).to_le_bytes());
            // +0x30: modifier 9 holds the index of the remap table (0x5128fc, 0 when not
            // found); 0xb a unit handle (not kept: 0); else the value as is.
            let c = match i.modifier {
                9 => if i.coloring < 8 { i.coloring as u32 } else { 0 },
                0xb => 0,
                _ => i.coloring as u32,
            };
            r[0x30..0x34].copy_from_slice(&c.to_le_bytes());
            // +0x34 / +0x38 (draw / update functions) are zeroed by 0x4d6330.
            r[0x3c..0x40].copy_from_slice(&idx1(i.sprite).to_le_bytes());
            b.extend_from_slice(&r);
        }
        o.packed(&b)
    }

    /// 0x4d6220: the images back into the pool (0x4d5600 turns the indices into pointers,
    /// +0x34 / +0x38 are rebuilt from the modifier), then 0x4d4ba0 links the unused slots
    /// (ascending) into the free list.
    /// 0x4d4ba0: the image pool's free slots linked in order (inline here).
    pub(super) fn load_images(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(n) = i.u16() else { return false };
        for im in &mut self.images[..MAX_IMAGES] {
            *im = Image::default();
        }
        if n != 0 {
            let Some(b) = i.packed(n as usize * 0x42) else { return false };
            for r in b.chunks_exact(0x42) {
                let slot = u16::from_le_bytes([r[0], r[1]]) as usize;
                if slot >= MAX_IMAGES {
                    return false; // the game would write outside its pool
                }
                let r = &r[2..];
                let u16_ = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let u32_ = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
                let modifier = r[0xa];
                self.images[slot] = Image {
                    link: Link { prev: from1(u32_(0)), next: from1(u32_(4)) },
                    image_id: u16_(8),
                    modifier,
                    direction: r[0xb],
                    flags: u16_(0xc),
                    x: r[0xe] as i8,
                    y: r[0xf] as i8,
                    iscript: IscriptState { header: u16_(0x18), pc: u16_(0x1a), ret: u16_(0x1c), anim: r[0x1e], wait: r[0x1f] },
                    frameset: u16_(0x20),
                    frame: u16_(0x22),
                    in_use: u32_(0x2c) != 0,
                    coloring: if modifier == 0xb { 0 } else { u32_(0x30) as u64 }, // 0xb: unit handle, not kept
                    sprite: from1(u32_(0x3c)),
                    // +0x24 (map position) is saved as 0.
                    pos_y: 0,
                };
            }
        }
        // 0x4d4ba0: the free slots in order.
        let mut l = List::default();
        let mut last = NIL;
        for s in 0..MAX_IMAGES {
            if self.images[s].in_use {
                continue;
            }
            self.images[s].link = Link { prev: last, next: NIL };
            if last == NIL {
                l.head = s as u32;
            } else {
                self.images[last as usize].link.next = s as u32;
            }
            last = s as u32;
        }
        l.tail = last;
        self.free_images = l;
        true
    }
}
