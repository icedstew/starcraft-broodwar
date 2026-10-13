//! The two storm.dll (1.16.1) decompressors MPQ sound files use: adaptive Huffman
//! (0x1501d090) and IMA-style ADPCM (0x1501c0f0). Their tables are read from the
//! user's storm.dll at startup (`load`), not shipped.

use std::io;
use std::sync::OnceLock;

struct Tables {
    /// 0x15050030: Huffman byte weights, 0x102 bytes per compression type 0..=8.
    weights: Vec<[u8; 0x102]>,
    /// 0x150509c8: ADPCM step sizes (89).
    steps: Vec<i32>,
    /// 0x15050948: ADPCM step index changes (32).
    next: Vec<i32>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read the decompression tables from storm.dll (1.16.1, image base 0x15000000).
pub fn load(dll: &[u8]) -> io::Result<()> {
    let u32_at = |o: usize| dll.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u16_at = |o: usize| dll.get(o..o + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    let pe = u32_at(0x3c).ok_or_else(|| err("storm.dll: no PE header"))? as usize;
    let n = u16_at(pe + 6).unwrap_or(0) as usize;
    let sections = pe + 24 + u16_at(pe + 20).unwrap_or(0) as usize;
    let base = u32_at(pe + 24 + 28).unwrap_or(0);
    let file_off = |va: u32| -> Option<usize> {
        (0..n).find_map(|i| {
            let s = sections + i * 40;
            let (vaddr, rsize, raw) = (u32_at(s + 12)?, u32_at(s + 16)?, u32_at(s + 20)?);
            let rva = va.checked_sub(base)?;
            (vaddr <= rva && rva < vaddr + rsize).then(|| (raw + rva - vaddr) as usize)
        })
    };
    let at = |va: u32, len: usize| file_off(va).and_then(|o| dll.get(o..o + len)).ok_or_else(|| err("storm.dll: table not found"));
    let ints = |b: &[u8]| b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>();
    let w = at(0x1505_0030, 9 * 0x102)?;
    let t = Tables {
        weights: w.chunks_exact(0x102).map(|c| c.try_into().unwrap()).collect(),
        steps: ints(at(0x1505_09c8, 89 * 4)?),
        next: ints(at(0x1505_0948, 32 * 4)?),
    };
    if t.steps[..3] != [7, 8, 9] {
        return Err(err("storm.dll: not the 1.16.1 storm.dll"));
    }
    let _ = TABLES.set(t);
    Ok(())
}

fn tables() -> io::Result<&'static Tables> {
    TABLES.get().ok_or_else(|| err("storm.dll tables not loaded"))
}

/// LSB-first bit reader.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> u32 {
        let b = self.d.get(self.pos / 8).map_or(0, |&b| (b >> (self.pos % 8)) & 1);
        self.pos += 1;
        b as u32
    }

    fn byte(&mut self) -> u32 {
        (0..8).map(|i| self.bit() << i).sum()
    }
}

const HEAD: usize = 0x203;
const NONE: usize = usize::MAX;

/// Storm's Huffman tree: items in a list kept sorted by weight, heaviest first
/// (`prev` points towards the head), with parent and low-child links.
struct Tree {
    prev: Vec<usize>,
    next: Vec<usize>,
    value: Vec<u32>,
    weight: Vec<u32>,
    parent: Vec<usize>,
    lo: Vec<usize>,
    used: usize,
    by_byte: [usize; 0x102],
}

impl Tree {
    fn new() -> Tree {
        let n = HEAD + 1;
        let mut t = Tree { prev: vec![NONE; n], next: vec![NONE; n], value: vec![0; n], weight: vec![0; n], parent: vec![NONE; n], lo: vec![NONE; n], used: 0, by_byte: [NONE; 0x102] };
        t.prev[HEAD] = HEAD;
        t.next[HEAD] = HEAD;
        t
    }

    fn unlink(&mut self, i: usize) {
        let (p, n) = (self.prev[i], self.next[i]);
        if p != NONE {
            self.next[p] = n;
            self.prev[n] = p;
        }
        self.prev[i] = NONE;
        self.next[i] = NONE;
    }

    /// Put `i` right after `at` (towards the tail).
    fn link_after(&mut self, at: usize, i: usize) {
        let n = self.next[at];
        self.prev[i] = at;
        self.next[i] = n;
        self.prev[n] = i;
        self.next[at] = i;
    }

    fn create(&mut self, value: u32, weight: u32, front: bool) -> usize {
        let i = self.used;
        self.used += 1;
        let at = if front { HEAD } else { self.prev[HEAD] };
        self.link_after(at, i);
        self.value[i] = value;
        self.weight[i] = weight;
        self.parent[i] = NONE;
        self.lo[i] = NONE;
        i
    }

    /// Walking towards the head from `i`, the first item weighing at least `w`.
    fn higher_or_equal(&self, mut i: usize, w: u32) -> usize {
        while i != HEAD {
            if self.weight[i] >= w {
                return i;
            }
            i = self.prev[i];
        }
        HEAD
    }

    fn fixup(&mut self, i: usize, max: u32) -> u32 {
        if self.weight[i] < max {
            let h = self.higher_or_equal(self.prev[HEAD], self.weight[i]);
            self.unlink(i);
            self.link_after(h, i);
            max
        } else {
            self.weight[i]
        }
    }

    /// 0x1501c980
    fn build(&mut self, w: &[u8; 0x102]) {
        let mut max = 0;
        for (i, &wt) in w.iter().take(0x100).enumerate() {
            if wt != 0 {
                let it = self.create(i as u32, wt as u32, true);
                max = self.fixup(it, max);
                self.by_byte[i] = it;
            }
        }
        self.by_byte[0x100] = self.create(0x100, 1, false);
        self.by_byte[0x101] = self.create(0x101, 1, false);
        let mut lo = self.prev[HEAD];
        while lo != HEAD {
            let hi = self.prev[lo];
            if hi == HEAD {
                break;
            }
            let p = self.create(0, self.weight[hi] + self.weight[lo], true);
            self.parent[lo] = p;
            self.parent[hi] = p;
            self.lo[p] = lo;
            max = self.fixup(p, max);
            lo = self.prev[hi];
        }
    }

    /// Exchange the list positions of `a` and `b`.
    fn swap_positions(&mut self, a: usize, b: usize) {
        let pa = self.prev[a];
        self.unlink(a);
        let pb = self.prev[b];
        self.unlink(b);
        if pa == b {
            // b came right before a.
            self.link_after(pb, a);
            self.link_after(a, b);
        } else if pb == a {
            unreachable!()
        } else {
            self.link_after(pa, b);
            self.link_after(pb, a);
        }
    }

    /// 0x15003e30: weight += 1 up to the root, swapping each item ahead of the
    /// lighter items it now outweighs.
    fn inc_weights(&mut self, mut item: usize) {
        while item != NONE {
            self.weight[item] += 1;
            let w = self.weight[item];
            let mut c = item;
            while self.prev[c] != HEAD && self.weight[self.prev[c]] < w {
                c = self.prev[c];
            }
            if c != item {
                self.swap_positions(item, c);
                let (ip, cp) = (self.parent[item], self.parent[c]);
                let c_parent_lo = if cp != NONE { self.lo[cp] } else { NONE };
                if ip != NONE && self.lo[ip] == item {
                    self.lo[ip] = c;
                }
                if c_parent_lo == c {
                    self.lo[cp] = item;
                }
                self.parent.swap(item, c);
            }
            item = self.parent[item];
        }
    }

    /// 0x1501c7f0: the tail item becomes a parent of itself and a new value.
    fn insert_new_branch(&mut self, v: u32) {
        let last = self.prev[HEAD];
        let (lv, lw) = (self.value[last], self.weight[last]);
        let hi = self.create(lv, lw, false);
        self.parent[hi] = last;
        self.by_byte[lv as usize] = hi;
        let lo = self.create(v, 0, false);
        self.parent[lo] = last;
        self.lo[last] = lo;
        self.by_byte[v as usize] = lo;
        self.inc_weights(lo);
    }

    /// 0x15004000: walk from the root; bit 1 takes the heavier child.
    fn decode(&self, bits: &mut Bits) -> u32 {
        let mut i = self.next[HEAD];
        while self.lo[i] != NONE {
            let lo = self.lo[i];
            i = if bits.bit() != 0 { self.prev[lo] } else { lo };
        }
        self.value[i]
    }
}

/// 0x1501d090 / 0x1501cf40: adaptive Huffman, up to `want` bytes.
pub fn huffman_decompress(input: &[u8], want: usize) -> io::Result<Vec<u8>> {
    let t = tables()?;
    let mut bits = Bits { d: input, pos: 0 };
    let kind = bits.byte() as usize;
    let w = t.weights.get(kind).ok_or_else(|| err("bad Huffman compression type"))?;
    let adaptive = kind == 0;
    let mut tree = Tree::new();
    tree.build(w);
    let mut out = Vec::with_capacity(want);
    while out.len() < want {
        let mut v = tree.decode(&mut bits);
        if v == 0x101 {
            v = bits.byte();
            tree.insert_new_branch(v);
            if !adaptive {
                tree.inc_weights(tree.by_byte[v as usize]);
            }
        }
        if v == 0x100 {
            break;
        }
        out.push(v as u8);
        if adaptive {
            tree.inc_weights(tree.by_byte[v as usize]);
        }
    }
    Ok(out)
}

/// 0x1501c0f0: ADPCM with `channels` interleaved 16-bit channels.
pub fn adpcm_decompress(input: &[u8], want: usize, channels: usize) -> io::Result<Vec<u8>> {
    let t = tables()?;
    let mut out = Vec::with_capacity(want);
    let shift = *input.get(1).ok_or_else(|| err("short ADPCM block"))? as u32;
    let mut pos = 2;
    let mut step = [0x2c_i32; 2];
    let mut pred = [0i32; 2];
    let push = |out: &mut Vec<u8>, v: i32| {
        if out.len() + 2 <= want {
            out.extend((v as i16).to_le_bytes());
            true
        } else {
            false
        }
    };
    for p in pred.iter_mut().take(channels) {
        let Some(b) = input.get(pos..pos + 2) else { return Ok(out) };
        *p = i16::from_le_bytes([b[0], b[1]]) as i32;
        pos += 2;
        if !push(&mut out, *p) {
            return Ok(out);
        }
    }
    let mut ch = channels - 1;
    while pos < input.len() {
        if channels == 2 {
            ch ^= 1;
        }
        let b = input[pos] as i32;
        pos += 1;
        if b & 0x80 != 0 {
            match b & 0x7f {
                0 => {
                    if step[ch] != 0 {
                        step[ch] -= 1;
                    }
                    if !push(&mut out, pred[ch]) {
                        break;
                    }
                }
                1 => {
                    step[ch] = (step[ch] + 8).min(0x58);
                    if channels == 2 {
                        ch ^= 1;
                    }
                }
                2 => {
                    step[ch] = (step[ch] - 8).max(0);
                    if channels == 2 {
                        ch ^= 1;
                    }
                }
                _ => {}
            }
            continue;
        }
        let s = t.steps[step[ch] as usize];
        let mut d = s >> shift;
        for k in 0..6 {
            if b & (1 << k) != 0 {
                d += s >> k;
            }
        }
        pred[ch] = if b & 0x40 != 0 { (pred[ch] - d).max(-0x8000) } else { (pred[ch] + d).min(0x7fff) };
        if !push(&mut out, pred[ch]) {
            break;
        }
        step[ch] = (step[ch] + t.next[(b & 0x1f) as usize]).clamp(0, 0x58);
    }
    Ok(out)
}
