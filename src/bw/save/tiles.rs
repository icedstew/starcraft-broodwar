//! The map's tile, creep and fog arrays.

use super::{In, Out};
use crate::bw::list::NIL;
use crate::bw::{Data, State};

/// 0x47d460 per pointer: a node pointer as the saver stores it (1-based index, 0 = null).
fn node_idx(n: u32) -> u32 {
    if n == NIL { 0 } else { n + 1 }
}

/// 0x47d400 per pointer: the inverse.
fn node_ptr(i: u32) -> u32 {
    if i == 0 { NIL } else { i - 1 }
}

impl State {
    /// 0x4d02d0 from 0x4d0555: creep_saved (0x6d0c68, u32 size + packed w*h*2), creep_edges
    /// (0x6d0e80, w*h), the recession lists (0x47da80), then 0x5993c4 and 0x628494 (0x20000
    /// each) and the tile flags 0x6d1260 (0x40000).
    pub(super) fn save_tiles(&mut self, _d: &Data, o: &mut Out) -> bool {
        let m = &self.terrain;
        let n = m.width as usize * m.height as usize;
        let mut ok = true;
        let sv: Vec<u8> = m.creep_saved[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        o.u32(sv.len() as u32);
        ok &= o.packed(&sv);
        o.u32(n as u32);
        ok &= o.packed(&m.creep_edges[..n]);
        // 0x47da80: 0x659b10 nodes (0x400 x 0x14), 0x658b10 hash (0x400 u32), 0x658ae8 bucket
        // heads (10 u32), 0x65eb14 bucket counts (10 u16); pointers as 1-based indices.
        let r = &self.creep_rec;
        let mut nodes = Vec::with_capacity(0x5000);
        for nd in &r.nodes {
            for p in [nd.next, nd.prev, nd.hnext, nd.hprev] {
                nodes.extend_from_slice(&node_idx(p).to_le_bytes());
            }
            // +0x13 is padding, 0 (0x47d660 clears the pool)
            nodes.extend_from_slice(&[nd.x, nd.y, nd.bucket, 0]);
        }
        let hash: Vec<u8> = r.hash.iter().flat_map(|&p| node_idx(p).to_le_bytes()).collect();
        let heads: Vec<u8> = r.heads.iter().flat_map(|&p| node_idx(p).to_le_bytes()).collect();
        let counts: Vec<u8> = r.counts.iter().flat_map(|c| c.to_le_bytes()).collect();
        ok &= o.packed(&nodes) && o.packed(&hash) && o.packed(&heads) && o.packed(&counts);
        let u16s = |a: &[u16]| a.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        ok &= o.packed(&u16s(&m.mtxm[..0x10000]));
        ok &= o.packed(&u16s(&m.megatiles[..0x10000]));
        let tf: Vec<u8> = m.tile_flags[..0x10000].iter().flat_map(|v| v.to_le_bytes()).collect();
        ok &= o.packed(&tf);
        ok
    }

    /// 0x4cfef0 from the creep_saved block: the sizes must equal 0x413900 / 0x4138c0's
    /// (w*h*2, w*h); then 0x47d9d0 (recession lists) and the three maps.
    pub(super) fn load_tiles(&mut self, _d: &Data, i: &mut In) -> bool {
        let n = self.terrain.width as usize * self.terrain.height as usize;
        let Some(v) = i.u32().filter(|&s| s as usize == n * 2).and_then(|_| i.packed(n * 2)) else { return false };
        let Some(e) = i.u32().filter(|&s| s as usize == n).and_then(|_| i.packed(n)) else { return false };
        let (Some(nodes), Some(hash), Some(heads), Some(counts)) =
            (i.packed(0x5000), i.packed(0x1000), i.packed(0x28), i.packed(0x14))
        else {
            return false;
        };
        let (Some(mt), Some(mg), Some(tf)) = (i.packed(0x20000), i.packed(0x20000), i.packed(0x40000)) else {
            return false;
        };
        let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let m = &mut self.terrain;
        for (d, c) in m.creep_saved[..n].iter_mut().zip(v.chunks(2)) {
            *d = u16::from_le_bytes([c[0], c[1]]);
        }
        m.creep_edges[..n].copy_from_slice(&e);
        for (d, c) in m.mtxm[..0x10000].iter_mut().zip(mt.chunks(2)) {
            *d = u16::from_le_bytes([c[0], c[1]]);
        }
        for (d, c) in m.megatiles[..0x10000].iter_mut().zip(mg.chunks(2)) {
            *d = u16::from_le_bytes([c[0], c[1]]);
        }
        for (d, c) in m.tile_flags[..0x10000].iter_mut().zip(tf.chunks(4)) {
            *d = u32::from_le_bytes(c.try_into().unwrap());
        }
        let r = &mut self.creep_rec;
        for (k, nd) in r.nodes.iter_mut().enumerate() {
            let b = k * 0x14;
            nd.next = node_ptr(u32_at(&nodes, b));
            nd.prev = node_ptr(u32_at(&nodes, b + 4));
            nd.hnext = node_ptr(u32_at(&nodes, b + 8));
            nd.hprev = node_ptr(u32_at(&nodes, b + 12));
            (nd.x, nd.y, nd.bucket) = (nodes[b + 16], nodes[b + 17], nodes[b + 18]);
        }
        for (k, h) in r.hash.iter_mut().enumerate() {
            *h = node_ptr(u32_at(&hash, k * 4));
        }
        for (k, h) in r.heads.iter_mut().enumerate() {
            *h = node_ptr(u32_at(&heads, k * 4));
        }
        for (k, c) in r.counts.iter_mut().enumerate() {
            *c = u16::from_le_bytes([counts[k * 2], counts[k * 2 + 1]]);
        }
        true
    }
}
