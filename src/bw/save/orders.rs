//! The order pool (0x48c910 / 0x48c870).

use super::{In, Out};
use crate::bw::list::{Link, List, NIL};
use crate::bw::orders::{Order, MAX_ORDERS};
use crate::bw::{Data, State};

/// A slot index as the savers store it: index + 1, 0 = null.
fn idx(v: u32) -> u32 {
    if v == NIL { 0 } else { v + 1 }
}
fn from_idx(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

impl State {
    /// 0x48c910 (with 0x48c7d0): the 2000 queued-order nodes (20 bytes: prev, next as
    /// index + 1, order, unit type, position, target unit as handle) packed, then the free
    /// list's head and tail as index + 1. 0x48c770 afterwards needs no counterpart.
    pub(super) fn save_orders(&mut self, _d: &Data, o: &mut Out) -> bool {
        let mut buf = Vec::with_capacity(MAX_ORDERS * 0x14);
        for n in &self.orders {
            buf.extend_from_slice(&idx(n.link.prev).to_le_bytes());
            buf.extend_from_slice(&idx(n.link.next).to_le_bytes());
            buf.push(n.order);
            buf.push(0); // not kept: +9
            buf.extend_from_slice(&n.unit_type.to_le_bytes());
            buf.extend_from_slice(&n.pos.0.to_le_bytes());
            buf.extend_from_slice(&n.pos.1.to_le_bytes());
            let t = n.target;
            let h = if t == NIL || t + 1 >= 0x6a5 { 0 } else { self.unit_handle(t) };
            buf.extend_from_slice(&h.to_le_bytes());
        }
        if !o.packed(&buf) {
            return false;
        }
        o.u32(idx(self.free_orders.head));
        o.u32(idx(self.free_orders.tail));
        true
    }

    /// 0x48c870 (with 0x48c770): the pool, then the free list's head and tail; the order
    /// count (0x641698) is 2000 minus the free list's length.
    pub(super) fn load_orders(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(buf) = i.packed(MAX_ORDERS * 0x14) else { return false };
        let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        self.orders = buf
            .chunks_exact(0x14)
            .map(|b| Order {
                link: Link { prev: from_idx(u32_at(b, 0)), next: from_idx(u32_at(b, 4)) },
                order: b[8],
                unit_type: u16::from_le_bytes([b[0xa], b[0xb]]),
                pos: (i16::from_le_bytes([b[0xc], b[0xd]]), i16::from_le_bytes([b[0xe], b[0xf]])),
                // 0x48c770: no generation check, the slot is the low 11 bits
                target: match u32_at(b, 0x10) & 0x7ff {
                    0 => NIL,
                    s => s - 1,
                },
            })
            .collect();
        let (Some(head), Some(tail)) = (i.u32(), i.u32()) else { return false };
        self.free_orders = List { head: from_idx(head), tail: from_idx(tail) };
        let (mut n, mut p) = (0, self.free_orders.head);
        while p != NIL {
            n += 1;
            p = self.orders[p as usize].link.next;
        }
        self.order_count = MAX_ORDERS as u32 - n;
        true
    }
}
