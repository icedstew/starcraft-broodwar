//! Path search (Sai_path.cpp): region-level A* and the per-request context.

use super::sai::Sai;

/// 0x40c360: octagonal distance approximation.
pub fn approx_distance(x1: i32, x2: i32, y1: i32, y2: i32) -> u32 {
    let abs = |d: i32| {
        let s = (d >> 31) as u32;
        ((d as u32) ^ s).wrapping_sub(s)
    };
    let (mut big, mut small) = (abs(x1.wrapping_sub(x2)), abs(y1.wrapping_sub(y2)));
    if (big as i32) < (small as i32) {
        std::mem::swap(&mut big, &mut small);
    }
    if ((big >> 2) as i32) < small as i32 {
        big = big
            .wrapping_sub(big >> 4)
            .wrapping_add(small.wrapping_mul(3) >> 3)
            .wrapping_sub(big >> 6)
            .wrapping_add(small.wrapping_mul(3) >> 8);
    }
    big
}

/// 0x49c9f0: region id at a pixel position (split tiles pick by minitile).
pub fn region_at(sai: &Sai, x: i32, y: i32) -> u16 {
    let v = sai.tiles[((y / 32) * 256 + x / 32) as usize];
    if v < 0x2000 {
        return v;
    }
    let s = &sai.splits[v as usize - 0x2000];
    let bit = ((((y / 8) & 3) << 2) as u8).wrapping_add(((x / 8) & 3) as u8) & 0x1f;
    if s.mask as u32 & (1 << bit) != 0 { s.rgn2 } else { s.rgn1 }
}

/// Search context (0x16c bytes on the stack of 0x42fc10).
#[derive(Clone, Debug)]
pub struct PathCtx {
    /// +0x0: the unit.
    pub unit: u32,
    /// +0x4
    pub start: (i16, i16),
    /// +0x8
    pub target: (i16, i16),
    /// +0xc: unit at the goal.
    pub goal_unit: u32,
    /// +0x10: goal unit box (left, top, right, bottom).
    pub goal_box: [i16; 4],
    /// +0x18
    pub goal: (i16, i16),
    /// +0x1c: region search fell back to the closest reachable region.
    pub unreachable: u8,
    pub unk1d: u8,
    pub unk1e: u8,
    pub unk1f: u8,
    /// +0x20
    pub regions: [u16; 50],
    /// +0x84
    pub region_index: i16,
    /// +0x86
    pub region_count: i16,
    /// +0x88
    pub waypoints: [(i16, i16); 50],
    /// +0x150
    pub waypoint_index: i16,
    /// +0x152
    pub waypoint_count: i16,
    /// +0x154: full region path length (may exceed 50).
    pub region_path_len: u8,
    /// +0x158: unit box extents (-left, -up, right, down).
    pub dims: [i16; 4],
    /// +0x160 / +0x162
    pub start_region: u16,
    pub target_region: u16,
    /// +0x164: region search nodes used, +0x168: largest open-set size.
    pub nodes_used: u16,
    pub unk166: u16,
    pub open_max: u16,
    pub unk16a: u16,
}

impl Default for PathCtx {
    fn default() -> PathCtx {
        PathCtx {
            unit: 0,
            start: (0, 0),
            target: (0, 0),
            goal_unit: 0,
            goal_box: [0; 4],
            goal: (0, 0),
            unreachable: 0,
            unk1d: 0,
            unk1e: 0,
            unk1f: 0,
            regions: [0; 50],
            region_index: 0,
            region_count: 0,
            waypoints: [(0, 0); 50],
            waypoint_index: 0,
            waypoint_count: 0,
            region_path_len: 0,
            dims: [0; 4],
            start_region: 0,
            target_region: 0,
            nodes_used: 0,
            unk166: 0,
            open_max: 0,
            unk16a: 0,
        }
    }
}

impl PathCtx {
    /// The 1.16.1 byte layout (unit pointers zeroed), for differential tests.
    pub fn layout(&self) -> Vec<u8> {
        let mut b = vec![0u8; 0x16c];
        let w16 = |b: &mut Vec<u8>, o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        w16(&mut b, 4, self.start.0 as u16);
        w16(&mut b, 6, self.start.1 as u16);
        w16(&mut b, 8, self.target.0 as u16);
        w16(&mut b, 0xa, self.target.1 as u16);
        for i in 0..4 {
            w16(&mut b, 0x10 + i * 2, self.goal_box[i] as u16);
            w16(&mut b, 0x158 + i * 2, self.dims[i] as u16);
        }
        w16(&mut b, 0x18, self.goal.0 as u16);
        w16(&mut b, 0x1a, self.goal.1 as u16);
        b[0x1c] = self.unreachable;
        b[0x1d] = self.unk1d;
        b[0x1e] = self.unk1e;
        b[0x1f] = self.unk1f;
        for i in 0..50 {
            w16(&mut b, 0x20 + i * 2, self.regions[i]);
            w16(&mut b, 0x88 + i * 4, self.waypoints[i].0 as u16);
            w16(&mut b, 0x8a + i * 4, self.waypoints[i].1 as u16);
        }
        w16(&mut b, 0x84, self.region_index as u16);
        w16(&mut b, 0x86, self.region_count as u16);
        w16(&mut b, 0x150, self.waypoint_index as u16);
        w16(&mut b, 0x152, self.waypoint_count as u16);
        b[0x154] = self.region_path_len;
        w16(&mut b, 0x160, self.start_region);
        w16(&mut b, 0x162, self.target_region);
        w16(&mut b, 0x164, self.nodes_used);
        w16(&mut b, 0x166, self.unk166);
        w16(&mut b, 0x168, self.open_max);
        w16(&mut b, 0x16a, self.unk16a);
        b
    }
}

/// Region search node (0x18 bytes in a 349-entry stack pool, root separate).
#[derive(Clone, Copy, Default)]
struct Node {
    closed: bool,
    region: u16,
    parent: Option<usize>,
    h: i32,
    g: i32,
    f: i32,
}

const NODE_POOL: usize = 349;
const OPEN_MAX: usize = 0x7d;
/// Sentinel heap slot 0 (f = -1) stops sift-up.
const SENTINEL: usize = usize::MAX;

struct Open {
    /// 0x6cad98: 1-based min-heap of node indices.
    heap: [usize; 0x7e],
    /// 0x6caf90
    count: usize,
}

impl Open {
    fn f(&self, nodes: &[Node], i: usize) -> i32 {
        let n = self.heap[i];
        if n == SENTINEL { -1 } else { nodes[n].f }
    }

    /// Insert at `count` and sift up (inlined in 0x4e1d90).
    fn sift_up(&mut self, nodes: &[Node], mut i: usize) {
        let x = self.heap[i];
        self.heap[0] = SENTINEL;
        let xf = nodes[x].f;
        while xf <= self.f(nodes, i / 2) {
            self.heap[i] = self.heap[i / 2];
            i /= 2;
        }
        self.heap[i] = x;
    }

    fn push(&mut self, nodes: &[Node], n: usize) {
        self.count += 1;
        self.heap[self.count] = n;
        self.sift_up(nodes, self.count);
    }

    /// 0x404920
    fn sift_down(&mut self, nodes: &[Node], mut i: usize) {
        let n = self.count as i32;
        let x = self.heap[i];
        if (n / 2) < i as i32 {
            self.heap[i] = x;
            return;
        }
        loop {
            let mut c = i * 2;
            if (c as i32) < n && self.f(nodes, c + 1) < self.f(nodes, c) {
                c += 1;
            }
            let y = self.heap[c];
            if nodes[x].f <= self.f(nodes, c) {
                break;
            }
            self.heap[i] = y;
            i = c;
            if (n / 2) < i as i32 {
                self.heap[i] = x;
                return;
            }
        }
        self.heap[i] = x;
    }

    fn pop(&mut self, nodes: &[Node]) -> usize {
        let top = self.heap[1];
        self.heap[1] = self.heap[self.count];
        self.count -= 1;
        self.sift_down(nodes, 1);
        top
    }
}

/// 0x4e1d90: A* over regions from the start region to the target region. If they are
/// not connected, finds the region closest to the target that can be reached.
/// Region +8 holds the node index + 1 during the search.
pub fn find_region_path(sai: &mut Sai, ctx: &mut PathCtx) -> bool {
    let start = ctx.start_region;
    let mut target = ctx.target_region;
    if start == target {
        return false;
    }
    let mut retry = false;
    ctx.unreachable = 0;
    // nodes[0] is the root (0x2138), nodes[1..] the pool (0x2120..0x68).
    let mut nodes: Vec<Node> = Vec::with_capacity(NODE_POOL + 1);
    let mut open = Open { heap: [SENTINEL; 0x7e], count: 0 };
    let (best, same) = loop {
        ctx.nodes_used = 0;
        ctx.open_max = 0;
        let group = sai.regions[start as usize].group;
        let same = group == sai.regions[target as usize].group || retry;
        let s_pos = ((ctx.start.0 as i32) << 8, (ctx.start.1 as i32) << 8);
        let t_pos = ((ctx.target.0 as i32) << 8, (ctx.target.1 as i32) << 8);
        let (root, goal, goal_pos) = if same { (start, target, t_pos) } else { (target, start, s_pos) };
        nodes.clear();
        nodes.push(Node { closed: false, region: root, parent: None, h: 0x400000, g: 0, f: 0x400000 });
        open.count = 0;
        sai.regions[root as usize].unk8 = 1;
        open.push(&nodes, 0);
        let pos_of = |sai: &Sai, r: u16| {
            if r == start {
                s_pos
            } else if r == target {
                t_pos
            } else {
                sai.regions[r as usize].center
            }
        };
        let mut found: Option<usize> = None;
        let mut exhausted = open.count == 0;
        while !exhausted {
            let cur = open.pop(&nodes);
            if nodes[cur].region == goal {
                found = Some(cur);
                break;
            }
            nodes[cur].closed = true;
            let cr = nodes[cur].region as usize;
            let reg = sai.regions[cr];
            let cnt = if reg.group != group { reg.neighbor_count } else { reg.walkable_neighbors };
            let cpos = pos_of(sai, nodes[cur].region);
            for k in 0..cnt as usize {
                let id = sai.neighbor(cr, k);
                let nr = sai.regions[id as usize];
                if nr.unk8 == 0 {
                    let n = nodes.len();
                    let npos = pos_of(sai, id);
                    let mut d = approx_distance(cpos.0, npos.0, cpos.1, npos.1) as i32;
                    if nr.group != group {
                        d = double_rounded(d);
                    }
                    let h = approx_distance(npos.0, goal_pos.0, npos.1, goal_pos.1) as i32;
                    let g = nodes[cur].g.wrapping_add(d);
                    nodes.push(Node { closed: false, region: id, parent: Some(cur), h, g, f: g.wrapping_add(h) });
                    sai.regions[id as usize].unk8 = n as u32 + 1;
                    open.push(&nodes, n);
                    if open.count == OPEN_MAX || nodes.len() == NODE_POOL + 1 {
                        break;
                    }
                } else if nodes[cur].parent != Some(nr.unk8 as usize - 1) {
                    let e = nr.unk8 as usize - 1;
                    let epos = pos_of(sai, nodes[e].region);
                    let mut d = approx_distance(epos.0, cpos.0, epos.1, cpos.1) as i32;
                    if sai.regions[nodes[e].region as usize].group != group {
                        d = double_rounded(d);
                    }
                    let g = nodes[cur].g.wrapping_add(d);
                    if g < nodes[e].g {
                        nodes[e].parent = Some(cur);
                        nodes[e].g = g;
                        let nf = nodes[e].h.wrapping_add(g);
                        if !nodes[e].closed {
                            let old = nodes[e].f;
                            if old != nf {
                                nodes[e].f = nf;
                                let mut i = open.count;
                                while i > 0 && open.heap[i] != e {
                                    i -= 1;
                                }
                                if i != 0 {
                                    if old > nf {
                                        open.sift_up(&nodes, i);
                                    } else {
                                        open.sift_down(&nodes, i);
                                    }
                                }
                            }
                        } else {
                            nodes[e].f = nf;
                            nodes[e].closed = false;
                            open.push(&nodes, e);
                        }
                        if open.count == OPEN_MAX {
                            break;
                        }
                    }
                }
            }
            if (ctx.open_max as i16 as i32) < open.count as i32 {
                ctx.open_max = open.count as u16;
            }
            if open.count == OPEN_MAX || nodes.len() == NODE_POOL + 1 {
                if open.count != 0 {
                    found = Some(open.pop(&nodes));
                }
                break;
            }
            exhausted = open.count == 0;
        }
        let best = match found {
            Some(n) => n,
            None => {
                ctx.unreachable = 1;
                let rc = sai.regions[root as usize].center;
                nodes[0].h = approx_distance(rc.0, goal_pos.0, rc.1, goal_pos.1) as i32;
                let mut best = 0;
                let mut bh = nodes[0].h;
                for (i, n) in nodes.iter().enumerate().skip(1) {
                    if n.h < bh {
                        bh = n.h;
                        best = i;
                    }
                }
                best
            }
        };
        if !same && nodes[best].region != goal {
            for n in &nodes {
                sai.regions[n.region as usize].unk8 = 0;
            }
            target = nodes[best].region;
            retry = true;
            continue;
        }
        break (best, same);
    };
    ctx.nodes_used = nodes.len() as u16;
    let mut len: i32 = 0;
    if same {
        let mut n = Some(best);
        while let Some(i) = n {
            len += 1;
            n = nodes[i].parent;
        }
        let mut idx = len - 1;
        let mut cur = Some(best);
        if idx >= 0x32 {
            let skip = idx - 0x31;
            idx -= skip;
            for _ in 0..skip {
                cur = nodes[cur.unwrap()].parent;
            }
        }
        if idx >= 0 {
            for i in (0..=idx as usize).rev() {
                let c = cur.unwrap();
                ctx.regions[i] = nodes[c].region;
                cur = nodes[c].parent;
            }
        }
    } else {
        let group = sai.regions[start as usize].group;
        let mut cur = Some(best);
        while let Some(c) = cur {
            if sai.regions[nodes[c].region as usize].group != group {
                break;
            }
            if len < 0x32 {
                ctx.regions[len as usize] = nodes[c].region;
            }
            cur = nodes[c].parent;
            len += 1;
        }
    }
    ctx.region_path_len = len as u8;
    ctx.region_count = len.min(0x32) as i16;
    ctx.region_index = -1;
    for n in &nodes {
        sai.regions[n.region as usize].unk8 = 0;
    }
    len > 0
}

/// `(d * 512) >> 8` with signed rounding toward zero.
fn double_rounded(d: i32) -> i32 {
    let v = d.wrapping_mul(0x200);
    (v + ((v >> 31) & 0xff)) >> 8
}

/// 0x4e25f0 up to the waypoint search: regions of start and target, then the region path.
pub fn find_path_regions(sai: &mut Sai, ctx: &mut PathCtx, unit_dims: [i16; 4]) -> bool {
    ctx.start_region = region_at(sai, ctx.start.0 as i32, ctx.start.1 as i32);
    ctx.target_region = region_at(sai, ctx.target.0 as i32, ctx.target.1 as i32);
    ctx.goal = ctx.target;
    ctx.unreachable = 0;
    ctx.unk1d = 0;
    ctx.unk1e = 0;
    ctx.unk1f = 0;
    ctx.dims = [unit_dims[0].wrapping_neg(), unit_dims[1].wrapping_neg(), unit_dims[2], unit_dims[3]];
    ctx.nodes_used = 0;
    ctx.unk166 = 0;
    ctx.open_max = 0;
    ctx.unk16a = 0;
    if ctx.start_region == ctx.target_region {
        ctx.regions[0] = ctx.start_region;
        ctx.region_index = -1;
        ctx.region_count = 1;
        ctx.region_path_len = 1;
        return true;
    }
    find_region_path(sai, ctx)
}

/// 0x4e25f0: full path search (region path, then waypoints for the first leg).
pub fn find_path(env: &mut super::short_path::Env, w: &mut super::short_path::Walker, ctx: &mut PathCtx) -> bool {
    let dims = (env.dims)(env.unit_type);
    if !find_path_regions(env.sai, ctx, dims) {
        return false;
    }
    super::short_path::next_leg(env, w, ctx)
}
