//! Pathing regions (SAI, 0x6d5bfc): sai_PathCreate.cpp.
//!
//! The 1.16.1 structure is one 0x97a20-byte allocation; this keeps the same fields and
//! the same in-place aliasing (neighbor lists live either in a region's inline array
//! or in the shared neighbor buffer).

use super::map::GameMap;

pub const MAX_REGIONS: usize = 5000;
const NEIGHBOR_BUF: usize = 10000;
pub const SPLIT_COUNT: usize = 25000;

/// Tile value for unassigned walkability classes (before regions are made).
pub const UNWALKABLE: u16 = 0x1ffd;
pub const REMOVED: u16 = 0x1fff;

/// Where a region's neighbor list (region +0xc) points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NeighborPtr {
    #[default]
    Null,
    /// Inline array (+0x2c) of the given region slot.
    Inline(u16),
    /// Index into the shared neighbor buffer (0x92bfc).
    Shared(u16),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Region {
    /// 0x00: walkability class (0x1ff9..0x1ffd; 0x1ffd unwalkable; 0x1fff removed).
    pub flags: u16,
    /// 0x02: connected group id (0x4000 = map-bottom region).
    pub group: u16,
    /// 0x04
    pub tile_count: u16,
    /// 0x06: neighbors in the same walkable class come first.
    pub walkable_neighbors: u8,
    /// 0x07
    pub neighbor_count: u8,
    pub unk8: u32,
    /// 0x0c
    pub neighbors: NeighborPtr,
    /// 0x10 / 0x14: center, pixels << 8 (tile coordinates until 0x483b50 converts them).
    pub center: (i32, i32),
    /// 0x18: left, top, right, bottom.
    pub rect: [i16; 4],
    /// 0x20 (0x483160)
    pub properties: u8,
    /// 0x21: neighbors with region id 0.
    pub zero_neighbors: u8,
    pub unk22: [u8; 10],
    /// 0x2c
    pub inline_neighbors: [u16; 10],
}

/// Split tile (0x2000c + 6*i): minitile mask and the two regions it divides.
#[derive(Clone, Copy, Debug, Default)]
pub struct Split {
    pub mask: u16,
    pub rgn1: u16,
    pub rgn2: u16,
}

pub struct Sai {
    /// 0x0
    pub region_count: u16,
    /// 0x4: next free slot in the shared neighbor buffer.
    pub neighbor_cursor: u16,
    /// 0x8: split tile pointer set (always 0x2000c once set).
    pub splits_ptr_set: bool,
    /// 0xc: region id per tile, 256 per row. >= 0x2000 = split tile index + 0x2000.
    pub tiles: Vec<u16>,
    /// 0x2000c
    pub splits: Vec<Split>,
    /// 0x449fc
    pub regions: Vec<Region>,
    /// 0x92bfc
    pub neighbor_buf: Vec<u16>,
}

impl Sai {
    fn new() -> Sai {
        Sai {
            region_count: 0,
            neighbor_cursor: 0,
            splits_ptr_set: false,
            tiles: vec![0; 0x10000],
            splits: vec![Split::default(); SPLIT_COUNT],
            regions: vec![Region::default(); MAX_REGIONS],
            neighbor_buf: vec![0; NEIGHBOR_BUF],
        }
    }

    fn tile(&self, x: i32, y: i32) -> u16 {
        self.tiles[(y * 256 + x) as usize]
    }

    fn set_tile(&mut self, x: i32, y: i32, v: u16) {
        self.tiles[(y * 256 + x) as usize] = v;
    }

    pub fn neighbor(&self, r: usize, i: usize) -> u16 {
        match self.regions[r].neighbors {
            NeighborPtr::Inline(s) => self.inline_at(s as usize, i),
            NeighborPtr::Shared(o) => self.neighbor_buf[o as usize + i],
            NeighborPtr::Null => panic!("null neighbor list"),
        }
    }

    fn set_neighbor(&mut self, r: usize, i: usize, v: u16) {
        match self.regions[r].neighbors {
            NeighborPtr::Inline(s) => self.set_inline_at(s as usize, i, v),
            NeighborPtr::Shared(o) => self.neighbor_buf[o as usize + i] = v,
            NeighborPtr::Null => panic!("null neighbor list"),
        }
    }

    /// Inline neighbor storage is the last 10 words of the 0x40-byte region; indexing
    /// past it runs into the next region, which this does not model.
    fn inline_at(&self, slot: usize, i: usize) -> u16 {
        self.regions[slot].inline_neighbors[i]
    }

    fn set_inline_at(&mut self, slot: usize, i: usize, v: u16) {
        self.regions[slot].inline_neighbors[i] = v;
    }

    fn shared_write(&mut self, i: usize, v: u16) {
        // ponytail: the original overruns its 10000-word buffer into the contour pointer
        // and past the allocation; grow instead.
        if i >= self.neighbor_buf.len() {
            self.neighbor_buf.resize(i + 1, 0);
        }
        self.neighbor_buf[i] = v;
    }
}

/// 0x484290: create pathing regions for the map. None if the map needs >= 5000 regions.
pub fn create(map: &mut GameMap, ts: &super::map::TilesetData) -> Option<Sai> {
    let mut sai = Sai::new();
    init_tiles(&mut sai, map, ts);
    add_bottom_region(&mut sai, map);
    if !split_regions(&mut sai, map, (0, 0, map.width as i32, map.height as i32)) {
        return None;
    }
    Some(sai)
}

/// 0x483d20: minitile map, then the walkability class of every tile.
fn init_tiles(sai: &mut Sai, map: &mut GameMap, ts: &super::map::TilesetData) {
    map.build_minitiles(ts);
    let w = map.width as usize;
    for y in 0..map.height as usize {
        for x in 0..w {
            let f = map.tile_flags[y * w + x];
            sai.tiles[y * 256 + x] = if f & 0x41_0000 == 0 {
                UNWALKABLE
            } else {
                match f & 0x601_0000 {
                    0 => UNWALKABLE,
                    0x201_0000 => 0x1ff9,
                    m => (m != 0x401_0000) as u16 + 0x1ffa,
                }
            };
        }
    }
}

/// 0x483c00: one unwalkable region covering the bottom tile row.
fn add_bottom_region(sai: &mut Sai, map: &GameMap) {
    let idx = sai.region_count;
    sai.region_count += 1;
    let (w, h) = (map.width as u32, map.height as u32);
    let r = &mut sai.regions[idx as usize];
    r.flags = UNWALKABLE;
    r.center = ((((w * 0x20) / 2 + 0x10) * 0x100) as i32, (h * 0x2000).wrapping_sub(0x1000) as i32);
    r.rect = [0, (h * 0x20 - 0x20) as i16, (w << 5) as i16, (h << 5) as i16];
    for x in 0..w as i32 {
        sai.set_tile(x, h as i32 - 1, idx);
    }
    let r = &mut sai.regions[idx as usize];
    r.tile_count = w as u16;
    r.group = 0x4000;
}

/// 0x484210
fn split_regions(sai: &mut Sai, map: &mut GameMap, area: (i32, i32, i32, i32)) -> bool {
    let first = sai.region_count as i32;
    if !make_regions(sai, map, area) {
        return false;
    }
    update_regions(sai, map, first);
    merge_small_regions(sai);
    compact_regions(sai, map);
    update_regions(sai, map, first);
    sai.splits_ptr_set = true;
    true
}

/// 0x483e30: cover every unassigned tile in `area` with rectangular regions.
fn make_regions(sai: &mut Sai, map: &mut GameMap, (x0, y0, x1, y1): (i32, i32, i32, i32)) -> bool {
    let (mw, mh) = (map.width as i32, map.height as i32);
    let initial = sai.region_count as i32;
    let row_skip = x0 - x1 + 0x100;
    let (mut cur_x, mut cur_y) = (x0, y0);
    let mut expanded = false;
    let mut best_value: u16 = 0;
    let mut threshold = 0x38;
    loop {
        let mut best = [0i16; 4];
        let mut tried = 0;
        let mut best_area = 0;
        let mut x = cur_x;
        let mut y = cur_y;
        if cur_x >= x1 {
            x = x0;
            y = cur_y + 1;
            if y >= y1 {
                y = y0;
            }
        }
        let mut pos = y * 256 + x;
        let start = pos;
        loop {
            let v = sai.tiles[pos as usize];
            if v >= MAX_REGIONS as u16 {
                let r = region_rect(sai, map, x, y, v);
                tried += 1;
                let area = (r[3] as i32 - r[1] as i32) * (r[2] as i32 - r[0] as i32);
                if best_area < area {
                    best_area = area;
                    best = r;
                    best_value = sai.tiles[pos as usize];
                }
                if best_area >= threshold || tried > 0x19 {
                    cur_y = best[1] as i32;
                    cur_x = best[2] as i32;
                    break;
                }
                let right = r[2] as i32;
                pos += right - x - 1;
                x = right - 1;
            }
            pos += 1;
            x += 1;
            if x >= x1 {
                pos += row_skip;
                x = x0;
                y += 1;
                if y >= y1 {
                    y = y0;
                    pos = y * 256 + x;
                }
            }
            if pos == start {
                break;
            }
        }
        let idx = sai.region_count;
        if idx >= MAX_REGIONS as u16 {
            // todo_port: 0x484131 frees the minitile map and shows the "too many regions" error.
            return false;
        }
        let w = best[2] as i32 - best[0] as i32;
        let h = best[3] as i32 - best[1] as i32;
        threshold = h * w;
        if threshold == 0 {
            return true;
        }
        fill_rect(sai, best, idx);
        let r = &mut sai.regions[idx as usize];
        r.center = (w / 2 + best[0] as i32, h / 2 + best[1] as i32);
        expand_rect(&mut best, mw, mh);
        absorb_neighbors(sai, map, idx as u32, best, best_value);
        let r = &mut sai.regions[idx as usize];
        r.rect = best;
        r.flags = best_value;
        sai.region_count += 1;
        if threshold > 6 || expanded {
            continue;
        }
        expanded = true;
        for i in initial..sai.region_count as i32 {
            let mut rect = sai.regions[i as usize].rect;
            expand_rect(&mut rect, mw, mh);
            sai.regions[i as usize].rect = rect;
            let f = sai.regions[i as usize].flags;
            absorb_neighbors(sai, map, i as u32, rect, f);
        }
    }
}

fn expand_rect(r: &mut [i16; 4], mw: i32, mh: i32) {
    if r[0] > 0 {
        r[0] -= 1;
    }
    if (r[2] as i32) < mw {
        r[2] += 1;
    }
    if r[1] > 0 {
        r[1] -= 1;
    }
    if (r[3] as i32) < mh {
        r[3] += 1;
    }
}

/// 0x483960: largest rect of tiles equal to `v` growing right/down from (x, y),
/// at most 8x7 and at most 3:1.
fn region_rect(sai: &Sai, map: &GameMap, x0: i32, y0: i32, v: u16) -> [i16; 4] {
    let mut out = [x0 as i16, y0 as i16, 0, 0];
    let xmax = (x0 + 8).min(map.width as i32);
    let ymax = (y0 + 7).min(map.height as i32);
    let (mut right_ok, mut down_ok, mut diag_ok) = (true, true, true);
    let mut x = x0 + 1;
    let mut y = y0 + 1;
    let mut width = x - x0;
    let mut height = y - y0;
    while x < xmax && y < ymax {
        if right_ok {
            if (0..height).any(|i| sai.tile(x, y0 + i) != v) {
                right_ok = false;
            }
        } else if !down_ok {
            break;
        }
        if down_ok && (0..width).any(|i| sai.tile(x0 + i, y) != v) {
            down_ok = false;
        }
        if sai.tile(x, y) != v {
            diag_ok = false;
        }
        if diag_ok {
            if down_ok {
                y += 1;
                height += 1;
            }
            if right_ok {
                x += 1;
                width += 1;
            }
        } else if down_ok {
            y += 1;
            height += 1;
        } else if right_ok {
            x += 1;
            width += 1;
        }
    }
    let mut w = x as i16 as i32 - out[0] as i32;
    let mut h = y as i16 as i32 - out[1] as i32;
    if w > h {
        w = w.min(h * 3);
    } else {
        h = h.min(w * 3);
    }
    out[2] = out[0] + w as i16;
    out[3] = out[1] + h as i16;
    out
}

/// 0x482c60
fn fill_rect(sai: &mut Sai, r: [i16; 4], v: u16) {
    for y in r[1] as i32..r[3] as i32 {
        for x in r[0] as i32..r[2] as i32 {
            sai.set_tile(x, y, v);
        }
    }
}

/// 0x482ae0: tiles of class `v` in `r` touching region `id` join it (two passes).
fn absorb_neighbors(sai: &mut Sai, map: &GameMap, id: u32, r: [i16; 4], v: u16) {
    let (mw, mh) = (map.width as i32, map.height as i32);
    for _ in 0..2 {
        for y in r[1] as i32..r[3] as i32 {
            for x in r[0] as i32..r[2] as i32 {
                if sai.tile(x, y) != v {
                    continue;
                }
                let touches = (y > 0 && id == sai.tile(x, y - 1) as u32)
                    || (x > 0 && id == sai.tile(x - 1, y) as u32)
                    || (x + 1 < mw && id == sai.tile(x + 1, y) as u32)
                    || (y + 1 < mh && id == sai.tile(x, y + 1) as u32);
                if touches {
                    sai.set_tile(x, y, id as u16);
                }
            }
        }
    }
}

/// 0x483b50: tile counts/bounds, neighbor lists, centers to pixels, groups.
fn update_regions(sai: &mut Sai, map: &GameMap, first: i32) {
    sai.neighbor_cursor = 0;
    region_bounds(sai, map);
    build_neighbors(sai, map);
    for i in first..sai.region_count as i32 {
        let c = &mut sai.regions[i as usize].center;
        if c.0 < 0x100 {
            c.0 = c.0 * 0x2000 + 0x1000;
        }
        if c.1 < 0x100 {
            c.1 = c.1 * 0x2000 + 0x1000;
        }
    }
    assign_groups(sai);
}

/// 0x482900
fn region_bounds(sai: &mut Sai, map: &GameMap) {
    for r in &mut sai.regions[..sai.region_count.max(0) as usize] {
        r.tile_count = 0;
        r.rect = [0x7fff, 0x7fff, -1, -1];
    }
    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let v = sai.tile(x, y);
            if v >= MAX_REGIONS as u16 {
                continue;
            }
            let r = &mut sai.regions[v as usize];
            r.tile_count = r.tile_count.wrapping_add(1);
            if r.rect[1] as i32 > y * 32 {
                r.rect[1] = (y << 5) as i16;
            }
            if r.rect[0] as i32 > x * 32 {
                r.rect[0] = (x << 5) as i16;
            }
            if (r.rect[3] as i32) < y * 32 + 32 {
                r.rect[3] = ((y + 1) << 5) as i16;
            }
            if (r.rect[2] as i32) < x * 32 + 32 {
                r.rect[2] = ((x + 1) << 5) as i16;
            }
        }
    }
    for r in &mut sai.regions[..sai.region_count as usize] {
        if r.tile_count == 0 && r.flags != REMOVED {
            r.flags = REMOVED;
        }
    }
}

/// 0x482850
fn build_neighbors(sai: &mut Sai, map: &GameMap) {
    sai.neighbor_cursor = 0;
    for i in 0..sai.region_count as usize {
        if sai.regions[i].tile_count != 0 {
            find_neighbors(sai, map, i);
            sort_neighbors(sai, i);
        }
    }
}

/// 0x482090: the 8 tiles around (x, y): up, left, right, down, then up-left, up-right,
/// down-left, down-right; 0x1fff outside the map.
fn tile_neighbors(sai: &Sai, map: &GameMap, x: i32, y: i32) -> [u16; 8] {
    let (mw, mh) = (map.width as i32, map.height as i32);
    let t = |dx: i32, dy: i32| sai.tile(x + dx, y + dy);
    [
        if y > 0 { t(0, -1) } else { REMOVED },
        if x > 0 { t(-1, 0) } else { REMOVED },
        if x + 1 < mw { t(1, 0) } else { REMOVED },
        if y + 1 < mh { t(0, 1) } else { REMOVED },
        if y > 0 && x > 0 { t(-1, -1) } else { REMOVED },
        if y > 0 && x + 1 < mw { t(1, -1) } else { REMOVED },
        if y + 1 < mh && x > 0 { t(-1, 1) } else { REMOVED },
        if y + 1 < mh && x + 1 < mw { t(1, 1) } else { REMOVED },
    ]
}

/// 0x482290: collect the regions adjacent to region `r`. Diagonal neighbors of a
/// walkable region count only if the minitiles let a unit cut the corner.
fn find_neighbors(sai: &mut Sai, map: &GameMap, r: usize) {
    let cursor = sai.neighbor_cursor as usize;
    let reg = sai.regions[r];
    let direct = if reg.flags == UNWALKABLE { 8 } else { 4 };
    let div = |v: i16| (v as i32 / 32) as i16 as i32;
    let (x0, y0, x1, y1) = (div(reg.rect[0]), div(reg.rect[1]), div(reg.rect[2]), div(reg.rect[3]));
    let mut count = 0usize;
    let m = &map.minitiles;
    let walk = |p: isize, o: isize| (m[(p + o) as usize] as i8) >= 0;
    let all = |p: isize, os: &[isize]| os.iter().all(|&o| walk(p, o));
    for y in y0..y1 {
        for x in x0..x1 {
            if sai.tile(x, y) as usize != r {
                continue;
            }
            let near = tile_neighbors(sai, map, x, y);
            for (i, &n) in near.iter().enumerate() {
                if n == REMOVED || n as usize == r {
                    continue;
                }
                let add = if i < direct || sai.regions[n as usize].flags == UNWALKABLE {
                    true
                } else {
                    let p = ((y * 0x400 + x) * 4) as isize;
                    match i {
                        4 => {
                            all(p, &[0, 1, 0x400, 0x401, -0x802, -0x801, -0x402, -0x401])
                                && (all(p, &[-2, -1, 0x3fe, 0x3ff]) || all(p, &[-0x800, -0x7ff, -0x400, -0x3ff]))
                        }
                        5 => {
                            all(p, &[2, 3, 0x402, 0x403, -0x7fc, -0x7fb, -0x3fc, -0x3fb])
                                && (all(p, &[4, 5, 0x404, 0x405]) || all(p, &[-0x7fe, -0x7fd, -0x3fe, -0x3fd]))
                        }
                        6 => {
                            all(p, &[0x800, 0x801, 0xc00, 0xc01, 0xffe, 0xfff, 0x13fe, 0x13ff])
                                && (all(p, &[0x7fe, 0x7ff, 0xbfe, 0xbff]) || all(p, &[0x1000, 0x1001, 0x1400, 0x1401]))
                        }
                        _ => {
                            all(p, &[0x802, 0x803, 0xc02, 0xc03, 0x1004, 0x1005, 0x1404, 0x1405])
                                && (all(p, &[0x804, 0x805, 0xc04, 0xc05]) || all(p, &[0x1002, 0x1003, 0x1402, 0x1403]))
                        }
                    }
                };
                if add && !(0..count).any(|j| sai.neighbor_buf.get(cursor + j) == Some(&n)) {
                    sai.shared_write(cursor + count, n);
                    count += 1;
                }
            }
        }
    }
    let reg = &mut sai.regions[r];
    if count > 10 {
        reg.neighbors = NeighborPtr::Shared(cursor as u16);
        sai.neighbor_cursor += count as u16;
    } else {
        reg.neighbors = NeighborPtr::Inline(r as u16);
        for i in 0..count {
            reg.inline_neighbors[i] = sai.neighbor_buf[cursor + i];
        }
        for i in count..10 {
            reg.inline_neighbors[i] = REMOVED;
        }
    }
    let reg = &mut sai.regions[r];
    reg.neighbor_count = count as u8;
    reg.zero_neighbors = 0;
}

/// 0x4821a0: reorder a region's neighbors: walkable, then unwalkable, then region 0.
fn sort_neighbors(sai: &mut Sai, r: usize) {
    let n = sai.regions[r].neighbor_count as usize;
    let (mut unwalk, mut zero) = (Vec::new(), Vec::new());
    let mut out = 0;
    for i in 0..n {
        let v = sai.neighbor(r, i);
        if v == 0 {
            zero.push(v);
        } else if sai.regions[v as usize].flags == UNWALKABLE {
            unwalk.push(v);
        } else {
            sai.set_neighbor(r, out, v);
            out += 1;
        }
    }
    sai.regions[r].walkable_neighbors = out as u8;
    for v in unwalk {
        sai.set_neighbor(r, out, v);
        out += 1;
    }
    if !zero.is_empty() {
        sai.regions[r].zero_neighbors = zero.len() as u8;
        for v in zero {
            sai.set_neighbor(r, out, v);
            out += 1;
        }
    }
}

/// 0x482da0: connected groups, numbered from 1 (groups >= 0x4000 are kept).
fn assign_groups(sai: &mut Sai) {
    for r in &mut sai.regions[..sai.region_count as usize] {
        if r.group < 0x4000 {
            r.group = 0;
        }
    }
    let mut id = 1;
    for i in 0..sai.region_count as usize {
        if sai.regions[i].group == 0 && sai.regions[i].tile_count > 0 {
            flood_group(sai, i, id);
            id += 1;
        }
    }
}

/// 0x482d30
fn flood_group(sai: &mut Sai, r: usize, id: u16) {
    sai.regions[r].group = id;
    let reg = sai.regions[r];
    let (from, n) = if reg.flags == UNWALKABLE {
        (reg.walkable_neighbors as usize, reg.neighbor_count as i32 - reg.walkable_neighbors as i32)
    } else {
        (0, reg.walkable_neighbors as i32)
    };
    for i in 0..n.max(0) as usize {
        let nb = sai.neighbor(r, from + i) as usize;
        if sai.regions[nb].group == 0 {
            flood_group(sai, nb, id);
        }
    }
}

/// 0x482e10: fold small regions into their smallest same-class neighbor until fewer
/// than 2500 regions remain.
fn merge_small_regions(sai: &mut Sai) {
    let mut limit = 4;
    loop {
        limit += 2;
        for i in (0..sai.region_count as usize).rev() {
            let r = sai.regions[i];
            if r.tile_count == 0 || r.group >= 0x4000 || r.tile_count as i32 >= limit || r.neighbor_count == 0 {
                continue;
            }
            let mut best: Option<usize> = None;
            for k in 0..r.neighbor_count as usize {
                let n = sai.neighbor(i, k) as usize;
                let nr = &sai.regions[n];
                if nr.tile_count != 0
                    && nr.group < 0x4000
                    && r.flags == nr.flags
                    && best.is_none_or(|b| nr.tile_count < sai.regions[b].tile_count)
                {
                    best = Some(n);
                }
            }
            let Some(b) = best else { continue };
            let div = |v: i16| v as i32 / 32;
            for y in div(r.rect[1])..div(r.rect[3]) {
                for x in div(r.rect[0])..div(r.rect[2]) {
                    if sai.tile(x, y) as usize == i {
                        sai.set_tile(x, y, b as u16);
                    }
                }
            }
            let tc = sai.regions[i].tile_count;
            let br = &mut sai.regions[b];
            br.tile_count = br.tile_count.wrapping_add(tc);
            let r = &mut sai.regions[i];
            r.tile_count = 0;
            r.flags = REMOVED;
            let rr = r.rect;
            let br = &mut sai.regions[b];
            if rr[1] < br.rect[1] {
                br.rect[1] = rr[1];
            }
            if rr[3] > br.rect[3] {
                br.rect[3] = rr[3];
            }
            if rr[0] < br.rect[0] {
                br.rect[0] = rr[0];
            }
            if rr[2] > br.rect[2] {
                br.rect[2] = rr[2];
            }
        }
        let live = sai.regions[..sai.region_count as usize].iter().filter(|r| r.tile_count != 0).count();
        if (sai.region_count as i16) < 1 || live < 0x9c4 {
            return;
        }
    }
}

/// 0x483030: drop emptied regions and renumber tiles and neighbor lists.
fn compact_regions(sai: &mut Sai, map: &GameMap) {
    let n = sai.region_count as i16 as i32;
    let mut remap = vec![0u16; MAX_REGIONS];
    let mut next = 0u16;
    for i in 0..n.max(0) as usize {
        remap[i] = next;
        if sai.regions[i].tile_count != 0 {
            next += 1;
        }
    }
    let mut dst = 0;
    for i in 0..n.max(0) as usize {
        if sai.regions[i].tile_count == 0 {
            continue;
        }
        for k in 0..sai.regions[i].neighbor_count as usize {
            let v = sai.neighbor(i, k);
            sai.set_neighbor(i, k, remap[v as usize]);
        }
        sai.regions[dst] = sai.regions[i];
        dst += 1;
    }
    sai.region_count = next;
    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let v = sai.tile(x, y);
            sai.set_tile(x, y, remap[v as usize]);
        }
    }
}

/// Serialize in the 1.16.1 memory layout (pointers as offsets from the SAI base) for
/// differential tests.
pub fn layout(sai: &Sai) -> Vec<u8> {
    let mut b = vec![0u8; 0x97a20];
    let w16 = |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let w32 = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    w16(&mut b, 0, sai.region_count);
    w32(&mut b, 4, 0x92bfc + sai.neighbor_cursor as u32 * 2);
    if sai.splits_ptr_set {
        w32(&mut b, 8, 0x2000c);
    }
    for (i, &t) in sai.tiles.iter().enumerate() {
        w16(&mut b, 0xc + i * 2, t);
    }
    for (i, s) in sai.splits.iter().enumerate() {
        let o = 0x2000c + i * 6;
        w16(&mut b, o, s.mask);
        w16(&mut b, o + 2, s.rgn1);
        w16(&mut b, o + 4, s.rgn2);
    }
    for (i, r) in sai.regions.iter().enumerate() {
        let o = 0x449fc + i * 0x40;
        w16(&mut b, o, r.flags);
        w16(&mut b, o + 2, r.group);
        w16(&mut b, o + 4, r.tile_count);
        b[o + 6] = r.walkable_neighbors;
        b[o + 7] = r.neighbor_count;
        w32(&mut b, o + 8, r.unk8);
        let p = match r.neighbors {
            NeighborPtr::Null => 0,
            NeighborPtr::Inline(s) => 0x449fc + s as u32 * 0x40 + 0x2c,
            NeighborPtr::Shared(n) => 0x92bfc + n as u32 * 2,
        };
        w32(&mut b, o + 0xc, p);
        w32(&mut b, o + 0x10, r.center.0 as u32);
        w32(&mut b, o + 0x14, r.center.1 as u32);
        for k in 0..4 {
            w16(&mut b, o + 0x18 + k * 2, r.rect[k] as u16);
        }
        b[o + 0x20] = r.properties;
        b[o + 0x21] = r.zero_neighbors;
        b[o + 0x22..o + 0x2c].copy_from_slice(&r.unk22);
        for k in 0..10 {
            w16(&mut b, o + 0x2c + k * 2, r.inline_neighbors[k]);
        }
    }
    for (i, &v) in sai.neighbor_buf.iter().take(NEIGHBOR_BUF).enumerate() {
        w16(&mut b, 0x92bfc + i * 2, v);
    }
    b
}
