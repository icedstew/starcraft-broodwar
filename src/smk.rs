//! Smacker video (SMK2 / SMK4), as Storm's SVid plays the unit portraits
//! (portrait\*.smk). Video only: audio chunks are skipped.

/// Bits read least significant first.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> u32 {
        let b = self.d.get(self.pos >> 3).map_or(0, |&b| (b >> (self.pos & 7)) & 1);
        self.pos += 1;
        b as u32
    }

    fn bits(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |v, i| v | self.bit() << i)
    }
}

/// A Huffman tree of 8-bit leaves (the low / high byte trees of a header tree).
#[derive(Default)]
struct Tree8 {
    /// Nodes: (left, right) for branches; leaves hold their value.
    nodes: Vec<Node>,
}

#[derive(Clone, Copy)]
enum Node {
    Leaf(u32),
    Branch(usize, usize),
}

impl Tree8 {
    fn read(b: &mut Bits) -> Tree8 {
        let mut t = Tree8::default();
        if b.bit() != 0 {
            t.node(b);
            b.bit();
        }
        t
    }

    fn node(&mut self, b: &mut Bits) -> usize {
        let i = self.nodes.len();
        if b.bit() != 0 {
            self.nodes.push(Node::Leaf(0));
            let l = self.node(b);
            let r = self.node(b);
            self.nodes[i] = Node::Branch(l, r);
        } else {
            self.nodes.push(Node::Leaf(b.bits(8)));
        }
        i
    }

    fn get(&self, b: &mut Bits) -> u32 {
        let mut i = 0;
        loop {
            match self.nodes.get(i) {
                None => return 0,
                Some(Node::Leaf(v)) => return *v,
                Some(Node::Branch(l, r)) => i = if b.bit() != 0 { *r } else { *l },
            }
        }
    }
}

/// A 16-bit header tree (MMAP, MCLR, FULL, TYPE) with its three "recent value" leaves.
#[derive(Default)]
struct Tree16 {
    nodes: Vec<Node>,
    /// Leaf indices of the three escape codes (most recent first).
    last: [usize; 3],
}

impl Tree16 {
    fn read(b: &mut Bits) -> Tree16 {
        let mut t = Tree16::default();
        if b.bit() == 0 {
            t.nodes.push(Node::Leaf(0));
            t.last = [0; 3];
            return t;
        }
        let lo = Tree8::read(b);
        let hi = Tree8::read(b);
        let esc = [b.bits(16), b.bits(16), b.bits(16)];
        let mut last = [usize::MAX; 3];
        t.node(b, &lo, &hi, &esc, &mut last);
        b.bit();
        for l in last.iter_mut() {
            if *l == usize::MAX {
                *l = t.nodes.len();
                t.nodes.push(Node::Leaf(0));
            }
        }
        t.last = last;
        t
    }

    fn node(&mut self, b: &mut Bits, lo: &Tree8, hi: &Tree8, esc: &[u32; 3], last: &mut [usize; 3]) -> usize {
        let i = self.nodes.len();
        self.nodes.push(Node::Leaf(0));
        if b.bit() != 0 {
            let l = self.node(b, lo, hi, esc, last);
            let r = self.node(b, lo, hi, esc, last);
            self.nodes[i] = Node::Branch(l, r);
        } else {
            let v = lo.get(b) | hi.get(b) << 8;
            match esc.iter().position(|&e| e == v) {
                Some(k) => last[k] = i,
                None => self.nodes[i] = Node::Leaf(v),
            }
        }
        i
    }

    /// Clears the recent values (each frame).
    fn reset(&mut self) {
        for k in 0..3 {
            if let Some(n) = self.nodes.get_mut(self.last[k]) {
                *n = Node::Leaf(0);
            }
        }
    }

    fn get(&mut self, b: &mut Bits) -> u32 {
        let mut i = 0;
        let v = loop {
            match self.nodes.get(i) {
                None => return 0,
                Some(Node::Leaf(v)) => break *v,
                Some(Node::Branch(l, r)) => i = if b.bit() != 0 { *r } else { *l },
            }
        };
        let val = |t: &Self, k: usize| match t.nodes.get(t.last[k]) {
            Some(Node::Leaf(v)) => *v,
            _ => 0,
        };
        if v != val(self, 0) {
            let (v1, v0) = (val(self, 1), val(self, 0));
            if let Some(n) = self.nodes.get_mut(self.last[2]) {
                *n = Node::Leaf(v1);
            }
            if let Some(n) = self.nodes.get_mut(self.last[1]) {
                *n = Node::Leaf(v0);
            }
            if let Some(n) = self.nodes.get_mut(self.last[0]) {
                *n = Node::Leaf(v);
            }
        }
        v
    }
}

pub struct Smk {
    pub w: usize,
    pub h: usize,
    pub frames: usize,
    /// Milliseconds per frame.
    pub ms: u32,
    v4: bool,
    flags: u32,
    sizes: Vec<usize>,
    types: Vec<u8>,
    data: Vec<u8>,
    /// Offset of frame 0's data.
    start: usize,
    mmap: Tree16,
    mclr: Tree16,
    full: Tree16,
    ty: Tree16,
    /// The decoded image (palette indices) and its palette.
    pub pixels: Vec<u8>,
    pub palette: [[u8; 3]; 256],
    next: usize,
    offset: usize,
}

/// The 6-bit palette component scale (smacker's palmap).
fn pal6(v: u8) -> u8 {
    let v = (v & 0x3f) as u32;
    ((v * 255 + 31) / 63) as u8
}

const RUNS: [u32; 64] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
    39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 128, 256, 512, 1024, 2048,
];

impl Smk {
    pub fn parse(data: Vec<u8>) -> Option<Smk> {
        let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let sig = data.get(0..4)?;
        let v4 = match sig {
            b"SMK2" => false,
            b"SMK4" => true,
            _ => return None,
        };
        let (w, h, frames) = (u32_at(4)? as usize, u32_at(8)? as usize, u32_at(12)? as usize);
        let rate = u32_at(16)? as i32;
        let ms = if rate > 0 { rate as u32 } else if rate < 0 { (-rate) as u32 / 100 } else { 100 };
        let flags = u32_at(20)?;
        let trees = u32_at(0x34)? as usize;
        let n = frames + (flags & 1) as usize;
        let mut o = 0x68;
        let sizes: Vec<usize> = (0..n).map(|i| u32_at(o + i * 4).unwrap_or(0) as usize).collect();
        o += n * 4;
        let types = data.get(o..o + n)?.to_vec();
        o += n;
        let mut b = Bits { d: data.get(o..o + trees)?, pos: 0 };
        let mmap = Tree16::read(&mut b);
        let mclr = Tree16::read(&mut b);
        let full = Tree16::read(&mut b);
        let ty = Tree16::read(&mut b);
        let start = o + trees;
        let hh = if flags & 6 != 0 { h * 2 } else { h };
        Some(Smk { w, h: hh, frames, ms, v4, flags, sizes, types, start, mmap, mclr, full, ty, pixels: vec![0; w * hh], palette: [[0; 3]; 256], next: 0, offset: start, data })
    }

    /// Decodes the next frame; false after the last.
    pub fn next_frame(&mut self) -> bool {
        if self.next >= self.frames {
            return false;
        }
        let i = self.next;
        let size = self.sizes[i] & !3;
        let end = (self.offset + size).min(self.data.len());
        let mut p = self.offset;
        if self.types[i] & 1 != 0 {
            p = self.palette_chunk(p);
        }
        for track in 0..7 {
            if self.types[i] & (2 << track) != 0 {
                let n = self.data.get(p..p + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize;
                p += n.max(4);
            }
        }
        if p < end {
            let d = self.data[p..end].to_vec();
            self.video(&d);
        }
        self.offset += size;
        self.next += 1;
        true
    }

    /// Back to frame 0 (the image and palette carry over, as a loop does).
    pub fn rewind(&mut self) {
        self.next = 0;
        self.offset = self.start;
    }

    fn palette_chunk(&mut self, p: usize) -> usize {
        let len = self.data.get(p).copied().unwrap_or(0) as usize * 4;
        let old = self.palette;
        let mut q = p + 1;
        let end = p + len;
        let mut i = 0;
        while i < 256 && q < end {
            let t = self.data[q];
            q += 1;
            if t & 0x80 != 0 {
                i += (t & 0x7f) as usize + 1;
            } else if t & 0x40 != 0 {
                let n = (t & 0x3f) as usize + 1;
                let mut src = self.data.get(q).copied().unwrap_or(0) as usize;
                q += 1;
                for _ in 0..n {
                    if i >= 256 || src >= 256 {
                        break;
                    }
                    self.palette[i] = old[src];
                    i += 1;
                    src += 1;
                }
            } else {
                let (g, b) = (self.data.get(q).copied().unwrap_or(0), self.data.get(q + 1).copied().unwrap_or(0));
                q += 2;
                self.palette[i] = [pal6(t), pal6(g), pal6(b)];
                i += 1;
            }
        }
        p + len.max(1)
    }

    fn video(&mut self, d: &[u8]) {
        let mut b = Bits { d, pos: 0 };
        for t in [&mut self.mmap, &mut self.mclr, &mut self.full, &mut self.ty] {
            t.reset();
        }
        let (bw, bh) = (self.w / 4, self.h / 4);
        let stride = self.w;
        let blocks = bw * bh;
        let mut blk = 0;
        let rows_per = if self.flags & 6 != 0 { 2 } else { 1 };
        let _ = rows_per;
        while blk < blocks {
            let ty = self.ty.get(&mut b);
            let mut run = RUNS[((ty >> 2) & 0x3f) as usize];
            while run > 0 && blk < blocks {
                let (bx, by) = (blk % bw, blk / bw);
                let base = by * 4 * stride + bx * 4;
                match ty & 3 {
                    0 => {
                        let clr = self.mclr.get(&mut b);
                        let (hi, lo) = ((clr >> 8) as u8, clr as u8);
                        let mut map = self.mmap.get(&mut b);
                        for row in 0..4 {
                            for col in 0..4 {
                                self.pixels[base + row * stride + col] = if map & 1 != 0 { hi } else { lo };
                                map >>= 1;
                            }
                        }
                    }
                    1 => {
                        // ponytail: SMK4's half-size / double-size full block modes are
                        // read as the plain mode; the portraits are SMK2.
                        if self.v4 {
                            b.bit();
                        }
                        for row in 0..4 {
                            let o = base + row * stride;
                            let p = self.full.get(&mut b);
                            self.pixels[o + 2] = p as u8;
                            self.pixels[o + 3] = (p >> 8) as u8;
                            let p = self.full.get(&mut b);
                            self.pixels[o] = p as u8;
                            self.pixels[o + 1] = (p >> 8) as u8;
                        }
                    }
                    2 => {}
                    _ => {
                        let c = (ty >> 8) as u8;
                        for row in 0..4 {
                            self.pixels[base + row * stride..base + row * stride + 4].fill(c);
                        }
                    }
                }
                blk += 1;
                run -= 1;
            }
        }
    }
}
