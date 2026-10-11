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
    /// Placeholder without allocations (for moving the search state out briefly).
    pub fn empty() -> UnitSearch {
        UnitSearch { buf: Vec::new(), top: 0, stack: Vec::new(), marks: Vec::new(), generation: 0, max_w: 0, max_h: 0 }
    }

    /// 0x42ff80: units whose finder boxes overlap `b` (deduplicated with a generation
    /// mark). Returns the result start; close with `pop`.
    pub fn units_in_finder_box(&mut self, st: &State, b: [i16; 4]) -> usize {
        self.stack.push(self.top);
        let start = self.top;
        let mut out = start;
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.generation = 1;
        }
        let f = &st.finder;
        let n = f.count as i32;
        let iy0 = super::finder::search(&f.y, f.count, true, b[1] as i32) as i32;
        let mut iy1 = iy0;
        while iy1 < n && f.y[iy1 as usize].1 < b[3] as i32 {
            iy1 += 1;
        }
        let mut ix = if n == 0 { 0 } else { super::finder::search(&f.x, f.count, true, b[0] as i32) as i32 };
        if iy1 > iy0 {
            while ix < n && f.x[ix as usize].1 < b[2] as i32 {
                let e = f.x[ix as usize].0;
                if self.marks[e as usize] != self.generation {
                    self.marks[e as usize] = self.generation;
                    let fi = st.units[(e - 1) as usize].finder_index;
                    if fi[3] >= iy0 && fi[2] < iy1 {
                        self.put(out, e);
                        out += 1;
                    }
                }
                ix += 1;
            }
        }
        self.put(out, 0);
        self.top = out + 1;
        start
    }

    /// 0x430be0: units a unit of `u`'s size would touch standing at `p` (filtered in
    /// place in a 0x42ff80 search). Close with `pop`.
    pub fn units_touching_spot(&mut self, st: &State, dims: &dyn Fn(u16) -> [i16; 4], u: u32, p: (i16, i16)) -> usize {
        let (hw, hh) = ((self.max_w >> 1) as i16, (self.max_h >> 1) as i16);
        let b = [p.0.wrapping_sub(hw).wrapping_sub(1), p.1.wrapping_sub(hh).wrapping_sub(1), hw.wrapping_add(p.0).wrapping_add(1), hh.wrapping_add(p.1).wrapping_add(1)];
        let start = self.units_in_finder_box(st, b);
        let dim = dims;
        let [l, up, r, dn] = dim(st.units[u as usize].unit_type);
        let mut out = start;
        let mut i = start;
        while self.buf[i] != 0 {
            let c = self.buf[i] - 1;
            let cu = &st.units[c as usize];
            let sp = &st.sprites[cu.sprite as usize];
            let [cl, cu_, cr, cd] = dim(cu.unit_type);
            let left = sp.x.wrapping_sub(cl).wrapping_add((-1i16).wrapping_sub(r));
            let right = cr.wrapping_add(sp.x).wrapping_add(l.wrapping_add(1));
            let top = sp.y.wrapping_sub(cu_).wrapping_add((-1i16).wrapping_sub(dn));
            let bottom = cd.wrapping_add(sp.y).wrapping_add(up.wrapping_add(1));
            if left <= p.0 && p.0 <= right && top <= p.1 && p.1 <= bottom {
                self.buf[out] = self.buf[i];
                out += 1;
            }
            i += 1;
        }
        self.buf[out] = 0;
        start
    }
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

    pub(crate) fn put(&mut self, i: usize, v: u32) {
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

use super::sai::{ContourEntry, Contours};

fn c_v0(c: &Contours, e: &ContourEntry) -> i16 {
    c.dims_a[e.dir as usize & 3].wrapping_add(e.v[0])
}

fn c_lo(c: &Contours, e: &ContourEntry) -> i16 {
    c.dims_a[(e.flags & 3) as usize].wrapping_add(e.v[1])
}

fn c_hi(c: &Contours, e: &ContourEntry) -> i16 {
    c.dims_a[((e.flags >> 2) & 3) as usize].wrapping_add(e.v[2])
}

/// Binary search over a contour list by v0 (as in 0x429a60 and friends).
fn c_search(l: &[ContourEntry], key: i16) -> usize {
    let (mut lo, mut hi) = (0usize, l.len());
    while lo < hi {
        let mid = (hi + lo) >> 1;
        if key >= l[mid].v[0].wrapping_shl(1) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    hi
}

/// 0x429d50: shrink `b` (left, top, right, bottom) around (x, y) to the open area
/// bounded by the contours.
pub fn open_box(c: &Contours, x: i16, y: i16, b: &mut [i16; 4]) {
    let d = c.dims_a;
    let key = |v: i16, dd: i16, add: i16| v.wrapping_sub(dd).wrapping_mul(2).wrapping_add(add);
    let (l0, l1, l2, l3) = (&c.lists[0], &c.lists[1], &c.lists[2], &c.lists[3]);
    let mut c0 = c_search(l0, key(y, d[0], 1));
    let mut c1 = c_search(l1, key(x, d[1], -1));
    let mut c2 = c_search(l2, key(y, d[2], -1));
    let mut c3 = c_search(l3, key(x, d[3], 1));
    let (xi, yi) = (x as i32, y as i32);
    loop {
        let mut any;
        loop {
            any = false;
            if c0 > 0 {
                let e = &l0[c0 - 1];
                let (v, lo, hi) = (c_v0(c, e) as i32, c_lo(c, e) as i32, c_hi(c, e) as i32);
                any = true;
                if v < b[1] as i32 {
                    c0 = 0;
                } else {
                    c0 -= 1;
                    if b[0] as i32 <= hi && lo <= b[2] as i32 {
                        let dd = yi - v;
                        if xi < lo && dd < lo - xi {
                            b[2] = (lo - 1) as i16;
                        } else if hi < xi && dd < xi - hi {
                            b[0] = (hi + 1) as i16;
                        } else {
                            b[1] = (v + 1) as i16;
                        }
                    }
                }
            }
            if c1 < l1.len() {
                let e = &l1[c1];
                let (v, lo, hi) = (c_v0(c, e) as i32, c_lo(c, e) as i32, c_hi(c, e) as i32);
                any = true;
                if v <= b[2] as i32 {
                    if b[1] as i32 <= hi && lo <= b[3] as i32 {
                        let dd = v - xi;
                        if yi < lo && dd < lo - yi {
                            b[3] = (lo - 1) as i16;
                        } else if hi < yi && dd < yi - hi {
                            b[1] = (hi + 1) as i16;
                        } else {
                            b[2] = (v - 1) as i16;
                        }
                    }
                    c1 += 1;
                } else {
                    c1 = l1.len() + 1;
                }
            }
            if c2 < l2.len() {
                let e = &l2[c2];
                let (v, lo, hi) = (c_v0(c, e) as i32, c_lo(c, e) as i32, c_hi(c, e) as i32);
                any = true;
                if v <= b[3] as i32 {
                    if b[0] as i32 <= hi && lo <= b[2] as i32 {
                        let dd = v - yi;
                        if xi < lo && dd < lo - xi {
                            b[2] = (lo - 1) as i16;
                        } else if hi < xi && dd < xi - hi {
                            b[0] = (hi + 1) as i16;
                        } else {
                            b[3] = (v - 1) as i16;
                        }
                    }
                    c2 += 1;
                } else {
                    c2 = l2.len() + 1;
                }
            }
            if c3 == 0 {
                break;
            }
            let e = &l3[c3 - 1];
            let (v, lo, hi) = (c_v0(c, e) as i32, c_lo(c, e) as i32, c_hi(c, e) as i32);
            if v < b[0] as i32 {
                c3 = 0;
            } else {
                c3 -= 1;
                if b[1] as i32 <= hi && lo <= b[3] as i32 {
                    let dd = xi - v;
                    if yi < lo && dd < lo - yi {
                        b[3] = (lo - 1) as i16;
                    } else if hi < yi && dd < yi - hi {
                        b[1] = (hi + 1) as i16;
                    } else {
                        b[0] = (v + 1) as i16;
                    }
                }
            }
        }
        if !any {
            break;
        }
    }
}

/// One contour edge against a segment's span on the other axis. `contain` selects the
/// horizontal-edge rule (edge must cover the span) used for vertical moves.
fn edge_blocks(lo: i16, hi: i16, flags: u8, min: i16, max: i16, contain: bool) -> bool {
    if lo > max || hi < min {
        return false;
    }
    let strict = if contain { lo < min && hi > max } else { lo < max && hi > min };
    strict || flags & 0x10 == 0 || lo <= min || flags & 0x20 == 0 || hi >= max
}

/// 0x42a1b0: the segment (x0, y0) -> (x1, y1) crosses no contour.
pub fn segment_clear(c: &Contours, x0: i16, y0: i16, x1: i16, y1: i16) -> bool {
    let (xmin, xmax) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
    let (ymin, ymax) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
    let d = c.dims_a;
    if y1 < y0 {
        let l = &c.lists[0];
        let mut i = if l.is_empty() { 0 } else { c_search(l, y0.wrapping_sub(d[0]).wrapping_mul(2).wrapping_add(1)) };
        while i > 0 {
            let e = &l[i - 1];
            i -= 1;
            if c_v0(c, e) < y1 {
                break;
            }
            if edge_blocks(c_lo(c, e), c_hi(c, e), e.flags, xmin, xmax, true) {
                return false;
            }
        }
    } else if y0 < y1 {
        let l = &c.lists[2];
        let i = if l.is_empty() { 0 } else { c_search(l, y0.wrapping_sub(d[2]).wrapping_mul(2).wrapping_sub(1)) };
        for e in &l[i..] {
            if c_v0(c, e) > y1 {
                break;
            }
            if edge_blocks(c_lo(c, e), c_hi(c, e), e.flags, xmin, xmax, true) {
                return false;
            }
        }
    }
    if x1 < x0 {
        let l = &c.lists[3];
        let mut i = if l.is_empty() { 0 } else { c_search(l, x0.wrapping_sub(d[3]).wrapping_mul(2).wrapping_add(1)) };
        while i > 0 {
            let e = &l[i - 1];
            i -= 1;
            if c_v0(c, e) < x1 {
                return true;
            }
            if edge_blocks(c_lo(c, e), c_hi(c, e), e.flags, ymin, ymax, false) {
                return false;
            }
        }
    } else if x0 < x1 {
        let l = &c.lists[1];
        let i = if l.is_empty() { 0 } else { c_search(l, x0.wrapping_sub(d[1]).wrapping_mul(2).wrapping_sub(1)) };
        for e in &l[i..] {
            if c_v0(c, e) > x1 {
                return true;
            }
            if edge_blocks(c_lo(c, e), c_hi(c, e), e.flags, ymin, ymax, false) {
                return false;
            }
        }
    }
    true
}

impl State {
    fn contours_for_unit(&mut self, d: &Data, unit_type: u16) -> Option<Contours> {
        let mut c = self.sai.contours.take()?;
        let dims = |t: u16| {
            let v = d.dat.units.dimensions[t as usize];
            [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16]
        };
        (c.dims_a, c.dims_b) = super::short_path::contour_dims(unit_type, &dims);
        Some(c)
    }

    /// 0x4f1870: moving by (dx, dy) runs into terrain.
    pub fn terrain_blocks_move(&mut self, d: &Data, u: u32, dx: i16, dy: i16) -> bool {
        let s = self.units[u as usize].sprite as usize;
        let (sx, sy) = (self.sprites[s].x, self.sprites[s].y);
        let (nx, ny) = (sx.wrapping_add(dx), sy.wrapping_add(dy));
        let inside = |b: &[i16; 4]| nx >= b[0] && nx <= b[2] && ny >= b[1] && ny <= b[3];
        if inside(&self.units[u as usize].contour_bounds) {
            return false;
        }
        let mut b = [sx.wrapping_sub(0x80), sy.wrapping_sub(0x80), sx.wrapping_add(0x80), sy.wrapping_add(0x80)];
        let ut = self.units[u as usize].unit_type;
        let Some(c) = self.contours_for_unit(d, ut) else { return false };
        open_box(&c, sx as u16 as i16, sy as u16 as i16, &mut b);
        self.units[u as usize].contour_bounds = b;
        let r = if inside(&b) { false } else { !segment_clear(&c, sx, sy, nx, ny) };
        self.sai.contours = Some(c);
        r
    }

    /// 0x4f1980: the predicted position (0x63fed4/0x63fecc) is blocked by terrain or
    /// the map edge.
    pub fn predicted_move_blocked(&mut self, d: &Data, u: u32) -> bool {
        let (nx, ny) = self.mv.new_pos;
        if self.units[u as usize].status & 0x10_0000 != 0 {
            let s = self.units[u as usize].sprite as usize;
            let (dx, dy) = (nx as i32 - self.sprites[s].x as i32, ny as i32 - self.sprites[s].y as i32);
            return self.terrain_blocks_move(d, u, dx as i16, dy as i16);
        }
        let v = d.dat.units.dimensions[self.units[u as usize].unit_type as usize];
        let [l, up, r, dn] = [v as i16 as i32, (v >> 16) as i16 as i32, (v >> 32) as i16 as i32, (v >> 48) as i16 as i32];
        if nx as i32 - l < 0 || ny as i32 - up < 0 || r + nx as i32 >= self.map_px.0 as i32 {
            return true;
        }
        self.map_px.1 as i32 <= dn + ny as i32
    }

    /// 0x4304d0: units next to `u`'s box in the direction (dx, dy) within that distance.
    /// Entries may repeat. Returns the result start; close with `self.search.pop()`.
    pub fn units_ahead(&mut self, u: u32, mut dx: i16, mut dy: i16) -> usize {
        let s = self.units[u as usize].sprite as usize;
        if self.sprites[s].flags & super::sprite::sflag::HIDDEN != 0 {
            dx = 0;
            dy = 0;
        }
        self.search.stack.push(self.search.top);
        let start = self.search.top;
        let mut out = start;
        let fi = self.units[u as usize].finder_index;
        let f = &self.finder;
        let xv = |i: i32| f.x[i as usize].1;
        let yv = |i: i32| f.y[i as usize].1;
        let n = f.count as i32;
        let mut found = Vec::new();
        let (left, right, top, bottom) = (xv(fi[0]) as i16, xv(fi[1]) as i16, yv(fi[2]) as i16, yv(fi[3]) as i16);
        let other_fi = |e: u32| self.units[(e - 1) as usize].finder_index;
        if dx >= 1 {
            let mut i = fi[1];
            while i > 0 && f.x[(i - 1) as usize].1 == right as i32 {
                i -= 1;
            }
            while i < n {
                if (right.wrapping_add(dx) as i32) < xv(i) {
                    break;
                }
                let e = f.x[i as usize].0;
                if e != u + 1 {
                    let o = other_fi(e);
                    if top.wrapping_add(dy) as i32 <= yv(o[3]) && yv(o[2]) <= bottom.wrapping_add(dy) as i32 {
                        found.push(e);
                    }
                }
                i += 1;
            }
        } else if dx < 0 {
            let mut i = fi[0];
            while i + 1 < n && f.x[(i + 1) as usize].1 == left as i32 {
                i += 1;
            }
            while i >= 0 {
                if xv(i) < left.wrapping_add(dx) as i32 {
                    break;
                }
                let e = f.x[i as usize].0;
                if e != u + 1 {
                    let o = other_fi(e);
                    if top.wrapping_add(dy) as i32 <= yv(o[3]) && yv(o[2]) <= bottom.wrapping_add(dy) as i32 {
                        found.push(e);
                    }
                }
                i -= 1;
            }
        }
        if dy >= 1 {
            let mut i = fi[3];
            while i > 0 && f.y[(i - 1) as usize].1 == bottom as i32 {
                i -= 1;
            }
            while i < n {
                if (bottom.wrapping_add(dy) as i32) < yv(i) {
                    break;
                }
                let e = f.y[i as usize].0;
                if e != u + 1 {
                    let o = other_fi(e);
                    if left.wrapping_add(dx) as i32 <= xv(o[1]) && xv(o[0]) <= right.wrapping_add(dx) as i32 {
                        found.push(e);
                    }
                }
                i += 1;
            }
        } else if dy < 0 {
            let mut i = fi[2];
            while i + 1 < n && f.y[(i + 1) as usize].1 == top as i32 {
                i += 1;
            }
            while i >= 0 {
                if yv(i) < top.wrapping_add(dy) as i32 {
                    break;
                }
                let e = f.y[i as usize].0;
                if e != u + 1 {
                    let o = other_fi(e);
                    if left.wrapping_add(dx) as i32 <= xv(o[1]) && xv(o[0]) <= right.wrapping_add(dx) as i32 {
                        found.push(e);
                    }
                }
                i -= 1;
            }
        }
        for e in found {
            self.search.put(out, e);
            out += 1;
        }
        self.search.put(out, 0);
        self.search.top = out;
        start
    }

    /// 0x42dee0: `u` collides with `other` (alliance, larva and burrow rules).
    pub fn units_collide(&self, u: u32, other: u32) -> bool {
        let un = &self.units[u as usize];
        let ot = &self.units[other as usize];
        let st = un.status;
        if st & 0x40_0000 != 0 {
            let mut p = un.player;
            if p == 0xb {
                p = self.sprites[un.sprite as usize].owner;
            }
            if self.alliances[ot.player as usize][p as usize] == 0 {
                return false;
            }
        }
        if ot.unit_type == 0x23 {
            if st & 2 == 0 {
                return false;
            }
        } else if un.unit_type == 0x23 {
            return false;
        }
        if ot.status & 0x80_0000 != 0 && st & 2 == 0 {
            if st & 0x80_0000 != 0 && ot.order != 0x54 {
                return un.order == 0x52;
            }
            return false;
        }
        true
    }

    /// 0x4f20d0: first ground unit blocking the predicted move.
    pub fn unit_blocking_move(&mut self, u: u32) -> Option<u32> {
        let s = self.units[u as usize].sprite as usize;
        let (nx, ny) = self.mv.new_pos;
        let dx = nx.wrapping_sub(self.sprites[s].x);
        let dy = ny.wrapping_sub(self.sprites[s].y);
        let start = self.units_ahead(u, dx, dy);
        let mut hit = None;
        for o in self.search.results(start) {
            let ou = &self.units[o as usize];
            if ou.pathing_flags & 1 == 0 || ou.status & 0x20_0000 != 0 || o == u {
                continue;
            }
            if !self.units_collide(o, u) || self.units[o as usize].status & 0x10_0000 == 0 {
                continue;
            }
            hit = Some(o);
            break;
        }
        self.search.pop();
        hit
    }
}

/// 0x429b20: (x, y) is outside every contour obstacle (`c` dims set for the unit).
pub fn point_clear(c: &Contours, x: i16, y: i16) -> bool {
    let d = c.dims_a;
    let (l0, l1, l2, l3) = (&c.lists[0], &c.lists[1], &c.lists[2], &c.lists[3]);
    let mut i = if l0.is_empty() { 0 } else { c_search(l0, y.wrapping_mul(2).wrapping_sub(1)) };
    while i > 0 {
        let e = &l0[i - 1];
        if c_v0(c, e) < y {
            break;
        }
        if !(x < e.v[1].wrapping_add(d[1]) || e.v[2].wrapping_add(d[3]) < x) {
            return false;
        }
        i -= 1;
    }
    let i = if l1.is_empty() { 0 } else { c_search(l1, x.wrapping_mul(2).wrapping_add(1)) };
    for e in &l1[i..] {
        if x < c_v0(c, e) {
            break;
        }
        if e.v[1].wrapping_add(d[2]) <= y && y <= e.v[2].wrapping_add(d[0]) {
            return false;
        }
    }
    let i = if l2.is_empty() { 0 } else { c_search(l2, y.wrapping_mul(2).wrapping_add(1)) };
    for e in &l2[i..] {
        if y < c_v0(c, e) {
            break;
        }
        if e.v[1].wrapping_add(d[1]) <= x && x <= e.v[2].wrapping_add(d[3]) {
            return false;
        }
    }
    let mut i = if l3.is_empty() { 0 } else { c_search(l3, x.wrapping_mul(2).wrapping_sub(1)) };
    while i > 0 {
        let e = &l3[i - 1];
        if c_v0(c, e) < x {
            return true;
        }
        if !(y < e.v[1].wrapping_add(d[2]) || e.v[2].wrapping_add(d[0]) < y) {
            return false;
        }
        i -= 1;
    }
    true
}

impl State {
    /// 0x42d560: the minitile (or whole megatile) under (x, y) is walkable.
    pub fn point_walkable(&self, d: &Data, x: i16, y: i16) -> bool {
        let _ = d;
        let w = self.terrain.width as i32;
        let i = (x as i32 / 32 + (y as i32 / 32) * w) as usize;
        let f = self.terrain.tile_flags[i];
        if f & 0x40_0000 != 0 {
            return true;
        }
        if f & 0x2000_0000 == 0 {
            return f & 0x10000 != 0;
        }
        let mega = (self.terrain.megatiles[i] & 0x7fff) as usize;
        let mini = (((x as i32 / 8) & 3) + (((y as i32 / 8) & 3) * 4)) as usize;
        self.terrain.vf4.get(mega * 16 + mini).is_some_and(|v| v & 1 != 0)
    }

    /// 0x42d810: a unit of `unit_type` could stand at (x, y) as far as terrain goes.
    pub fn terrain_fits(&mut self, d: &Data, unit_type: u16, x: i16, y: i16) -> bool {
        let v = d.dat.units.dimensions[unit_type as usize];
        let [l, up, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        if x < l || r as i32 + x as i32 >= self.map_px.0 as i32 || y < up || dn as i32 + y as i32 >= self.map_px.1 as i32 {
            return false;
        }
        if !self.point_walkable(d, x, y) {
            return false;
        }
        let Some(c) = self.contours_for_unit(d, unit_type) else { return false };
        let r = point_clear(&c, x, y);
        self.sai.contours = Some(c);
        r
    }

    /// 0x42e1d0: what blocks `u` standing at `p`: None if free, Some(None) for terrain
    /// or the map edge, Some(Some(unit)) for the largest overlapping unit.
    pub fn spot_blocker(&mut self, d: &Data, u: u32, p: (i16, i16)) -> Option<Option<u32>> {
        let ut = self.units[u as usize].unit_type;
        let v = d.dat.units.dimensions[ut as usize];
        let [l, up, r, dn] = [v as i16 as i32, (v >> 16) as i16 as i32, (v >> 32) as i16 as i32, (v >> 48) as i16 as i32];
        let (x, y) = (p.0 as i32, p.1 as i32);
        if x - l < 0 || self.map_px.0 as i32 <= r + x || y - up < 0 || self.map_px.1 as i32 <= dn + y {
            return Some(None);
        }
        let b = self.unit_box_at(d, ut, p.0, p.1);
        let start = self.units_in_box(d, b);
        let mut best = None;
        let mut best_area = 0i32;
        for o in self.search.results(start) {
            // 0x42e0e0
            let ou = &self.units[o as usize];
            if ou.pathing_flags & 1 == 0 || ou.status & 0x20_0000 != 0 || o == u || !self.units_collide(o, u) {
                continue;
            }
            let fi = ou.finder_index;
            let f = &self.finder;
            if !((b[0] as i32) <= f.x[fi[1] as usize].1
                && f.x[fi[0] as usize].1 <= b[2] as i32
                && (b[1] as i32) <= f.y[fi[3] as usize].1
                && f.y[fi[2] as usize].1 <= b[3] as i32)
            {
                continue;
            }
            let ov = d.dat.units.dimensions[ou.unit_type as usize];
            let [ol, ou_, or, od] = [ov as i16, (ov >> 16) as i16, (ov >> 32) as i16, (ov >> 48) as i16];
            let area = (od.wrapping_add(ou_).wrapping_add(1) as i32) * (or.wrapping_add(ol).wrapping_add(1) as i32);
            if area - best_area != 0 && best_area <= area {
                best = Some(o);
                best_area = area;
            }
        }
        self.search.pop();
        if best.is_some() {
            return Some(best);
        }
        if self.terrain_fits(d, ut, p.0, p.1) { None } else { Some(None) }
    }

    /// Unit box (left, top, right, bottom) at its sprite position.
    pub fn unit_box(&self, d: &Data, u: u32) -> [i16; 4] {
        let s = self.units[u as usize].sprite as usize;
        self.unit_box_at(d, self.units[u as usize].unit_type, self.sprites[s].x, self.sprites[s].y)
    }

    /// 0x4f1a20: which side of `b` (the blocker) `u` is on: 0 below, 1 left, 2 above,
    /// 3 right (nearest side when overlapping).
    pub fn blocker_side(&self, d: &Data, b: u32, u: u32) -> u32 {
        let [bl, bt, br, bb] = self.unit_box(d, b);
        let [ul, ut, ur, ub] = self.unit_box(d, u);
        if br < ul {
            return 3;
        }
        if ur < bl {
            return 1;
        }
        if bb < ut {
            return 0;
        }
        if ub < bt {
            return 2;
        }
        let d3 = (br as i32 - ul as i32).abs();
        let d2 = (bt as i32 - ub as i32).abs();
        let d0 = (bb as i32 - ut as i32).abs();
        let mut cur = (bl as i32 - ur as i32).abs();
        let mut side = 1;
        if d3 < cur {
            side = 3;
            cur = d3;
        }
        if d2 < cur {
            side = 2;
            cur = d2;
        }
        if d0 < cur {
            side = 0;
        }
        side
    }

    /// 0x4f2a70: direction to sidestep blocker `b` (around its box corners), or -1.
    pub fn sidestep_dir(&mut self, d: &Data, b: u32, u: u32) -> i32 {
        if self.sprites[self.units[b as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return -1;
        }
        let s = self.units[u as usize].sprite as usize;
        let nw = self.units[u as usize].fl.next_waypoint;
        let dir = crate::game::flingy::direction_to(self.sprites[s].x as i32, self.sprites[s].y as i32, nw.0 as i32, nw.1 as i32) as u32;
        let q = dir >> 6;
        let mut diff = dir.wrapping_sub(self.units[u as usize].fl.velocity_dir as u32) & 0xff;
        if diff > 0x80 {
            diff = 0x100 - diff;
        }
        if diff >= 0x50 {
            return -1;
        }
        let v = d.dat.units.dimensions[self.units[u as usize].unit_type as usize];
        let [l, up, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let fi = self.units[b as usize].finder_index;
        let a = (self.finder.x[fi[0] as usize].1 as i16).wrapping_sub(r).wrapping_sub(1);
        let bb = (self.finder.x[fi[1] as usize].1 as i16).wrapping_add(l).wrapping_add(1);
        let c = (self.finder.y[fi[2] as usize].1 as i16).wrapping_sub(dn).wrapping_sub(1);
        let dd = (self.finder.y[fi[3] as usize].1 as i16).wrapping_add(up).wrapping_add(1);
        let (mut px, mut py) = (0i16, 0i16);
        let res = match self.blocker_side(d, b, u) {
            3 => {
                px = bb;
                match q {
                    3 => 0,
                    2 => 0x80,
                    _ => return -1,
                }
            }
            1 => {
                px = a;
                match q {
                    0 => 0,
                    1 => 0x80,
                    _ => return -1,
                }
            }
            0 => {
                py = dd;
                match q {
                    0 => 0x40,
                    3 => 0xc0,
                    _ => return -1,
                }
            }
            _ => {
                py = c;
                match q {
                    1 => 0x40,
                    2 => 0xc0,
                    _ => return -1,
                }
            }
        };
        let free = |st: &mut State, p: (i16, i16)| st.spot_blocker(d, u, p).is_none();
        match res {
            0 => {
                if free(self, (px, c)) {
                    return 0;
                }
                if free(self, (px, dd)) { 0x80 } else { -1 }
            }
            0x40 => {
                if free(self, (bb, py)) {
                    return 0x40;
                }
                if free(self, (a, py)) { 0xc0 } else { -1 }
            }
            0x80 => {
                if free(self, (px, dd)) {
                    return 0x80;
                }
                if free(self, (px, c)) { 0 } else { -1 }
            }
            _ => {
                if free(self, (a, py)) {
                    return 0xc0;
                }
                if free(self, (bb, py)) { 0x40 } else { -1 }
            }
        }
    }
}

impl State {
    /// 0x42da00 / 0x42d6b0: `p` lies in the box of unit `o`'s type centered at `center`,
    /// grown by the box of `unit_type`.
    pub fn point_in_unit_box(&self, d: &Data, o: u32, center: (i16, i16), unit_type: u16, p: (i16, i16)) -> bool {
        let dim = |t: u16| {
            let v = d.dat.units.dimensions[t as usize];
            [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16]
        };
        let [ol, ou, or, od] = dim(self.units[o as usize].unit_type);
        let [l, u, r, dn] = dim(unit_type);
        let (cx, cy) = center;
        cx.wrapping_sub(ol).wrapping_sub(r) <= p.0
            && p.0 <= or.wrapping_add(cx).wrapping_add(l)
            && cy.wrapping_sub(ou).wrapping_sub(dn) <= p.1
            && p.1 <= od.wrapping_add(cy).wrapping_add(u)
    }

    /// 0x430f10: the gap between the boxes of `a` (or its turret's base) and `b` is at
    /// most `dist`.
    pub fn box_distance_within(&self, d: &Data, a: u32, dist: u32, b: u32) -> bool {
        let a = if d.dat.units.flags[self.units[a as usize].unit_type as usize] & 0x10 != 0 { self.units[a as usize].subunit } else { a };
        let ba = self.unit_box(d, a);
        let bb = self.unit_box(d, b);
        let mut dx = ba[0] as i32 - (bb[2].wrapping_add(1) as i32);
        if dx < 0 {
            dx = bb[0] as i32 - (ba[2].wrapping_add(1) as i32);
            if dx < 0 {
                dx = 0;
            }
        }
        let mut dy = ba[1] as i32 - (bb[3].wrapping_add(1) as i32);
        if dy < 0 {
            dy = bb[1] as i32 - (ba[3].wrapping_add(1) as i32);
            if dy < 0 {
                dy = 0;
            }
        }
        dist >= super::path::approx_distance(0, dx, 0, dy)
    }
}
