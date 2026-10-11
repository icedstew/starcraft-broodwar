//! Unit box searches (result stack 0x6bb938) and unit/unit overlap flags.

use super::{Data, State};

/// 0x6bb938 / 0x6bd3d0 sizes (units + 1).
pub const SEARCH_SLOTS: usize = 0x6a5;

#[derive(Clone, Debug)]
pub struct UnitSearch {
    /// 0x6bb938: results (unit index + 1, 0-terminated runs).
    pub buf: Vec<u32>,
    /// 0x6bee64: next free slot.
    pub top: usize,
    /// 0x6bee70 / 0x6bee6c: saved tops of the open searches.
    pub stack: Vec<usize>,
    /// 0x6bd3d0: per-unit (index + 1) marks shared by the box searches.
    pub marks: Vec<u32>,
    /// 0x6d63ec: mark generation for 0x42ff80.
    pub generation: u32,
    /// 0x6bee68 / 0x6bb930: largest unit box (units.dat).
    pub max_w: u16,
    pub max_h: u16,
}

impl Default for UnitSearch {
    fn default() -> UnitSearch {
        UnitSearch {
            buf: vec![0; SEARCH_SLOTS],
            top: 0,
            stack: Vec::new(),
            marks: vec![0; SEARCH_SLOTS],
            generation: 0,
            max_w: 0,
            max_h: 0,
        }
    }
}

impl UnitSearch {
    /// 0x42fee0: reset and size from the largest units.dat box.
    pub fn init(&mut self, d: &Data) {
        *self = UnitSearch::default();
        let (mut w, mut h) = (0u32, 0u32);
        for t in (0..228).rev() {
            let v = d.dat.units.dimensions[t];
            let [l, u, r, dn] = [v as i16 as i32, (v >> 16) as i16 as i32, (v >> 32) as i16 as i32, (v >> 48) as i16 as i32];
            let uw = (r + 1 + l) as u32;
            let uh = (dn + 1 + u) as u32;
            if ((w & 0xffff) as i32) < uw as i32 {
                w = uw;
            }
            if ((h & 0xffff) as i32) < uh as i32 {
                h = uh;
            }
        }
        self.max_w = w as u16;
        self.max_h = h as u16;
    }

    fn put(&mut self, i: usize, v: u32) {
        if i >= self.buf.len() {
            // ponytail: the original overruns 0x6bb938 here; grow instead.
            self.buf.resize(i + 1, 0);
        }
        self.buf[i] = v;
    }

    /// Results of a search started at `start`, as unit indices.
    pub fn results(&self, start: usize) -> Vec<u32> {
        self.buf[start..].iter().take_while(|&&v| v != 0).map(|&v| v - 1).collect()
    }

    /// Close the most recent search.
    pub fn pop(&mut self) {
        self.top = self.stack.pop().unwrap_or(0);
    }
}

impl State {
    /// 0x402070: unit type's box placed at (x, y).
    pub fn unit_box_at(&self, d: &Data, unit_type: u16, x: i16, y: i16) -> [i16; 4] {
        let v = d.dat.units.dimensions[unit_type as usize];
        let [l, u, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        [x.wrapping_sub(l), y.wrapping_sub(u), r.wrapping_add(x), dn.wrapping_add(y)]
    }

    /// 0x4308a0: units whose finder boxes overlap `b`. Returns the result start; close
    /// with `self.search.pop()`.
    pub fn units_in_box(&mut self, d: &Data, b: [i16; 4]) -> usize {
        let s = &mut self.search;
        s.stack.push(s.top);
        let start = s.top;
        let mut out = start;
        let (x0, y0) = (b[0], b[1]);
        let mut x1 = b[2] as u16;
        let mut y1 = b[3] as u16;
        let ext_x = (b[2] as i32 - x0 as i32) + 1 < s.max_w as i32;
        if ext_x {
            x1 = (x0 as u16).wrapping_sub(1).wrapping_add(s.max_w);
        }
        let ext_y = (b[3] as i32 - y0 as i32) + 1 < s.max_h as i32;
        if ext_y {
            y1 = (y0 as u16).wrapping_sub(1).wrapping_add(s.max_h);
        }
        let f = &self.finder;
        let ix0 = super::finder::search(&f.x, f.count, true, x0 as i32);
        let ix1 = super::finder::search(&f.x, f.count, true, x1 as i32);
        let iy0 = super::finder::search(&f.y, f.count, true, y0 as i32);
        let iy1 = super::finder::search(&f.y, f.count, true, y1 as i32);
        if ix1 as i32 - ix0 as i32 > 0 && iy1 as i32 - iy0 as i32 > 0 {
            for i in ix0..ix1 {
                let e = self.finder.x[i].0;
                let mark = if ext_x {
                    let u = (e - 1) as usize;
                    let sp = &self.sprites[self.units[u].sprite as usize];
                    let l = d.dat.units.dimensions[self.units[u].unit_type as usize] as i16;
                    (sp.x as i32 - l as i32 <= b[2] as i32) as u32
                } else {
                    1
                };
                self.search.marks[e as usize] = mark;
            }
            for i in iy0..iy1 {
                let e = self.finder.y[i].0;
                let pass = if ext_y {
                    let u = (e - 1) as usize;
                    let sp = &self.sprites[self.units[u].sprite as usize];
                    let up = (d.dat.units.dimensions[self.units[u].unit_type as usize] >> 16) as i16;
                    sp.y as i32 - up as i32 <= b[3] as i32
                } else {
                    true
                };
                if pass && self.search.marks[e as usize] == 1 {
                    self.search.marks[e as usize] = 3;
                }
            }
            for i in ix0..ix1 {
                let e = self.finder.x[i].0;
                if self.search.marks[e as usize] == 3 {
                    self.search.put(out, e);
                    out += 1;
                }
                self.search.marks[e as usize] = 0;
            }
        }
        self.search.put(out, 0);
        self.search.top = out;
        start
    }

    /// 0x469b60: finder boxes of `a` and `b` overlap.
    pub fn finder_boxes_overlap(&self, a: u32, b: u32) -> bool {
        let (fa, fb) = (self.units[a as usize].finder_index, self.units[b as usize].finder_index);
        let x = |i: i32| self.finder.x[i as usize].1;
        let y = |i: i32| self.finder.y[i as usize].1;
        x(fa[0]) <= x(fb[1]) && x(fb[0]) <= x(fa[1]) && y(fa[2]) <= y(fb[3]) && y(fb[2]) <= y(fa[3])
    }

    /// 0x42d9a0: refresh the "overlapping another unit" status (0x40000) of the units
    /// touching `u`'s box (0x4e8830 with callback 0x42d650).
    pub fn update_unit_overlaps(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite as usize;
        let (x, y) = (self.sprites[s].x, self.sprites[s].y);
        let b = self.unit_box_at(d, self.units[u as usize].unit_type, x, y);
        let start = self.units_in_box(d, b);
        for other in self.search.results(start) {
            // 0x42d650 (ecx = found unit, edx = u)
            let st = self.units[other as usize].status;
            if st & 2 != 0 {
                self.units[u as usize].status |= 0x40000;
                continue;
            }
            if st & 4 == 0
                && (st & 0x80_0000 == 0 || self.units[u as usize].status & 2 != 0)
                && self.finder_boxes_overlap(u, other)
            {
                self.units[other as usize].status = st | 0x40000;
            }
        }
        self.search.pop();
    }
}
