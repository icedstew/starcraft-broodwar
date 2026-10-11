//! Unit paths (CPath, unit +0x100, pool 0x6bee8c) and path requests (Sai_path.cpp).

use super::path::{approx_distance, find_path, region_at, PathCtx};
use super::short_path::{next_leg, Env};
use super::{Data, State};

pub const PATH_COUNT: usize = 1024;

/// CPath (0x80 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Path {
    /// +0x0: position when made (free list: next free index + 1).
    pub start: (i16, i16),
    /// +0x4: next waypoint (free list: own index + 1).
    pub next_waypoint: (i16, i16),
    /// +0x8
    pub target: (i16, i16),
    /// +0xc: frame the path was created.
    pub creation_frame: u32,
    /// +0x10: handle of the unit being avoided.
    pub collision_unit: u32,
    /// +0x14: speed saved while avoiding.
    pub speed: i32,
    /// +0x18: flags (1 = repath requested).
    pub flags: u8,
    /// +0x19: wait countdown.
    pub wait: u8,
    /// +0x1a: avoidance direction.
    pub direction: u8,
    /// +0x1b: full region path length.
    pub total_regions: u8,
    /// +0x1c / +0x1d
    pub region_count: i8,
    pub region_index: i8,
    /// +0x1e / +0x1f
    pub waypoint_count: i8,
    pub waypoint_index: i8,
    /// +0x20: waypoints (4 bytes each), then region ids (2 bytes each).
    pub data: [u8; 0x60],
}

impl Default for Path {
    fn default() -> Path {
        Path {
            start: (0, 0),
            next_waypoint: (0, 0),
            target: (0, 0),
            creation_frame: 0,
            collision_unit: 0,
            speed: 0,
            flags: 0,
            wait: 0,
            direction: 0,
            total_regions: 0,
            region_count: 0,
            region_index: 0,
            waypoint_count: 0,
            waypoint_index: 0,
            data: [0; 0x60],
        }
    }
}

impl Path {
    pub fn waypoint(&self, i: i32) -> (i16, i16) {
        let o = (i * 4) as usize;
        let d = &self.data;
        (i16::from_le_bytes([d[o], d[o + 1]]), i16::from_le_bytes([d[o + 2], d[o + 3]]))
    }

    pub fn region(&self, i: i32) -> u16 {
        let o = (self.waypoint_count as i32 * 4 + i * 2) as usize;
        u16::from_le_bytes([self.data[o], self.data[o + 1]])
    }
}

impl State {
    /// 0x42f4f0: build the free list (0, 1, ..., 1023).
    pub fn path_pool_init(&mut self) {
        self.paths = vec![Path::default(); PATH_COUNT];
        for i in 0..PATH_COUNT {
            let p = &mut self.paths[i];
            p.next_waypoint.0 = (i + 1) as i16;
            p.start.0 = if i + 1 < PATH_COUNT { (i + 2) as i16 } else { 0 };
        }
        self.free_path = 1;
    }

    /// Push a path on the free list (0x42f740 / 0x42f830 head).
    fn path_release(&mut self, p: u32) {
        let head = self.free_path;
        let path = &mut self.paths[p as usize];
        path.next_waypoint = ((p + 1) as i16, 0);
        path.start = (head as i16, 0);
        self.free_path = p + 1;
    }

    /// 0x42f740: free the unit's path.
    pub fn path_delete(&mut self, u: u32) {
        let p = self.units[u as usize].path;
        if p != super::list::NIL {
            self.path_release(p);
            self.units[u as usize].path = super::list::NIL;
        }
    }

    fn path_pop(&mut self) -> Option<u32> {
        if self.free_path == 0 {
            return None;
        }
        let p = self.free_path - 1;
        self.free_path = self.paths[p as usize].start.0 as u16 as u32;
        Some(p)
    }

    /// 0x42f830: replace the unit's path with a direct one.
    pub fn path_direct(&mut self, u: u32, next: (i16, i16), target: (i16, i16)) {
        let old = self.units[u as usize].path;
        if old != super::list::NIL {
            self.path_release(old);
        }
        let p = self.path_pop();
        self.units[u as usize].path = p.unwrap_or(super::list::NIL);
        if let Some(p) = p {
            let s = self.units[u as usize].sprite as usize;
            let pos = (self.sprites[s].x, self.sprites[s].y);
            let path = &mut self.paths[p as usize];
            path.waypoint_count = 1;
            path.region_count = 1;
            path.total_regions = 1;
            path.waypoint_index = 0;
            path.region_index = 0;
            path.start = pos;
            path.next_waypoint = next;
            path.target = target;
            path.wait = 0;
            path.flags = 0;
            path.data[0..2].copy_from_slice(&next.0.to_le_bytes());
            path.data[2..4].copy_from_slice(&next.1.to_le_bytes());
        }
    }

    /// 0x4e42f0 + 0x42f560: store the search result as the unit's new path.
    fn path_from_ctx(&mut self, ctx: &mut PathCtx) -> Option<u32> {
        let size = |r: i32, w: i32| ((r + w * 2) * 2 + 0x23) & 0xfffc;
        if size(ctx.region_count as u16 as i32, ctx.waypoint_count as u16 as i32) > 0x80 {
            if ctx.waypoint_count < 0x18 {
                ctx.region_count = (0x18 - ctx.waypoint_count) * 2;
            } else {
                ctx.waypoint_count = 0x17;
                ctx.region_count = 1;
            }
            if size(ctx.region_count as i32, ctx.waypoint_count as u16 as i32) > 0x80 {
                return None;
            }
        }
        let p = self.path_pop()?;
        let frame = self.frame;
        let path = &mut self.paths[p as usize];
        path.wait = 0;
        path.creation_frame = frame;
        path.flags = 0;
        path.waypoint_count = ctx.waypoint_count as i8;
        path.waypoint_index = 0;
        path.region_count = ctx.region_count as i8;
        path.total_regions = ctx.region_path_len;
        path.region_index = (ctx.region_index as i8).min(path.region_count);
        let wn = path.waypoint_count.max(0) as usize;
        for i in 0..wn {
            let (x, y) = ctx.waypoints[i];
            path.data[i * 4..i * 4 + 2].copy_from_slice(&x.to_le_bytes());
            path.data[i * 4 + 2..i * 4 + 4].copy_from_slice(&y.to_le_bytes());
        }
        for i in 0..path.region_count.max(0) as usize {
            let o = wn * 4 + i * 2;
            if o + 2 <= path.data.len() {
                path.data[o..o + 2].copy_from_slice(&ctx.regions[i].to_le_bytes());
            }
        }
        Some(p)
    }

    /// 0x42f790: search context from the unit's current path.
    fn ctx_from_path(&self, u: u32, p: u32) -> PathCtx {
        let s = self.units[u as usize].sprite as usize;
        let path = &self.paths[p as usize];
        let mut ctx = PathCtx {
            unit: u,
            start: (self.sprites[s].x, self.sprites[s].y),
            target: path.target,
            ..Default::default()
        };
        for i in 0..path.waypoint_count.max(0) as usize {
            ctx.waypoints[i] = path.waypoint(i as i32);
        }
        ctx.waypoint_count = path.waypoint_count as i16;
        ctx.waypoint_index = path.waypoint_index as i16;
        for i in 0..path.region_count.max(0) as usize {
            ctx.regions[i] = path.region(i as i32);
        }
        ctx.region_count = path.region_count as i16;
        ctx.region_index = path.region_index as i16;
        ctx.region_path_len = path.total_regions;
        ctx
    }

    /// 0x42f920: the current path still leads somewhere useful.
    fn path_usable(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        let p = un.path;
        if p == super::list::NIL {
            return false;
        }
        let path = &self.paths[p as usize];
        let s = &self.sprites[un.sprite as usize];
        if (s.x, s.y) == path.next_waypoint {
            return false;
        }
        if path.next_waypoint == un.fl.move_target {
            return true;
        }
        if un.fl.movement_type != 0 {
            return true;
        }
        let d = approx_distance(s.x as i32, path.next_waypoint.0 as i32, s.y as i32, path.next_waypoint.1 as i32) as i32;
        if d > 0x10 {
            return true;
        }
        let mut diff = (un.fl.desired_dir.wrapping_sub(un.fl.velocity_dir_prev)) as i32;
        if diff > 0x80 {
            diff = 0x100 - diff;
        }
        if d * 2 >= (diff as u8) as i32 {
            return true;
        }
        if (path.waypoint_index as i32) < path.waypoint_count as i32 - 1 {
            return false;
        }
        region_at(&self.sai, s.x as i32, s.y as i32) != path.region(path.region_index as i32)
    }

    /// 0x42f700: the path's wait countdown (+0x19) ran out; otherwise count it down.
    pub fn path_wait_expired(&mut self, u: u32) -> bool {
        let p = self.units[u as usize].path;
        if p == super::list::NIL {
            return false;
        }
        let path = &mut self.paths[p as usize];
        if path.wait == 0 {
            return true;
        }
        path.wait -= 1;
        false
    }

    /// 0x42f720
    pub fn set_path_wait(&mut self, u: u32, v: u8) {
        let p = self.units[u as usize].path;
        if p != super::list::NIL {
            self.paths[p as usize].wait = v;
        }
    }

    /// 0x4eb130: set the next waypoint (+0x18) and next target waypoint (+0x1c).
    pub fn set_next_waypoint(&mut self, u: u32, p: (i16, i16)) {
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        if f.next_waypoint != p {
            f.next_waypoint = p;
            f.flags |= 1;
        }
    }

    /// 0x475f50: set the move target, leading a moving target unit.
    fn set_move_target_lead(&mut self, d: &Data, u: u32, mut p: (i16, i16)) {
        let tu = self.units[u as usize].move_target_unit;
        if let Some(t) = (tu != super::list::NIL).then_some(tu) {
            if self.units[t as usize].fl.flags & 2 != 0 {
                let hd = super::flingy::halt_distance(&self.units[u as usize].fl, d);
                let dir = self.units[t as usize].fl.velocity_dir_prev as usize;
                let (vx, vy) = crate::game::flingy::direction_table()[dir];
                let ts = &self.sprites[self.units[t as usize].sprite as usize];
                let x = (vx.wrapping_mul(hd.wrapping_mul(3)) >> 16) + ts.x as i32;
                let y = (vy.wrapping_mul(hd.wrapping_mul(3)) >> 16) + ts.y as i32;
                if (x as u32) < self.map_px.0 as u32 && (y as u32) < self.map_px.1 as u32 {
                    p = (x as i16, y as i16);
                }
            }
        }
        let f = &mut self.units[u as usize].fl;
        if f.move_target != p {
            f.move_target = p;
            f.flags = (f.flags & 0xfb) | 1;
            f.next_waypoint = f.move_target;
        }
    }

    fn path_env<'a>(&'a mut self, d: &'a Data, u: u32, dims: &'a dyn Fn(u16) -> [i16; 4]) -> (Env<'a>, &'a mut super::short_path::Walker) {
        let ut = self.units[u as usize].unit_type;
        let _ = d;
        (Env { sai: &mut self.sai, map_w: self.map_px.0, map_h: self.map_px.1, dims, unit_type: ut }, &mut self.walker)
    }

    /// 0x42fc10: advance along the current path or search a new one. `next` gets the
    /// next waypoint.
    fn path_find(&mut self, d: &Data, u: u32, mut target: (i16, i16), next: &mut (i16, i16)) -> bool {
        let dims = |t: u16| {
            let v = d.dat.units.dimensions[t as usize];
            [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16]
        };
        {
            let f = &mut self.units[u as usize].fl;
            f.flags = (f.flags & 0xbf) | 0x10;
        }
        let mut ctx: Option<PathCtx> = None;
        let p = self.units[u as usize].path;
        if p != super::list::NIL {
            let path = &mut self.paths[p as usize];
            path.waypoint_index = path.waypoint_index.wrapping_add(1);
            if path.waypoint_index >= path.waypoint_count {
                let mut c = self.ctx_from_path(u, p);
                self.path_delete(u);
                let i = c.region_index as i32;
                let n = c.region_count as i32;
                if i + 1 < n && (c.region_path_len as i32 == n || i + 4 < n) {
                    let ok = continue_leg(self, &dims, u, &mut c);
                    if !(ok && c.region_count == 0) && c.waypoint_count != 0 && ok {
                        ctx = Some(c);
                    }
                }
            }
        }
        let mut new_path = false;
        if ctx.is_none() && self.units[u as usize].path == super::list::NIL {
            let s = self.units[u as usize].sprite as usize;
            let mut c = PathCtx { unit: u, start: (self.sprites[s].x, self.sprites[s].y), target, ..Default::default() };
            c.goal_unit = 0;
            let ok = {
                let (mut env, w) = self.path_env(d, u, &dims);
                find_path(&mut env, w, &mut c)
            };
            if !ok || c.region_count == 0 || c.waypoint_count == 0 {
                return false;
            }
            ctx = Some(c);
        }
        if let Some(mut c) = ctx {
            if c.unk1e != 0 && c.goal != target {
                target = c.goal;
                self.set_move_target_lead(d, u, c.goal);
                let st = &mut self.units[u as usize].status;
                *st = (*st & !0x80000) | ((c.unk1f as u32) << 19 & 0x80000);
            }
            let Some(p) = self.path_from_ctx(&mut c) else {
                self.units[u as usize].path = super::list::NIL;
                return false;
            };
            self.units[u as usize].path = p;
            let s = self.units[u as usize].sprite as usize;
            let pos = (self.sprites[s].x, self.sprites[s].y);
            let path = &mut self.paths[p as usize];
            path.start = pos;
            path.target = target;
            new_path = true;
        }
        let _ = new_path;
        let p = self.units[u as usize].path;
        let path = &mut self.paths[p as usize];
        let w = path.waypoint(path.waypoint_index as i32);
        path.next_waypoint = w;
        *next = w;
        let tgt = path.target;
        if w == self.units[u as usize].fl.move_target {
            self.units[u as usize].fl.flags &= 0xef;
            return true;
        }
        if (approx_distance(w.0 as i32, tgt.0 as i32, w.1 as i32, tgt.1 as i32) as i32) < 0x10 {
            self.units[u as usize].fl.flags |= 0x40;
        }
        true
    }

    /// 0x42fe00: make sure the unit has a path toward `target`.
    pub fn path_request(&mut self, d: &Data, u: u32, target: (i16, i16)) -> bool {
        if self.path_usable(u) {
            return true;
        }
        if self.units[u as usize].status & 0x10_0000 == 0 {
            self.set_move_target_xy(d, u, target);
            self.path_direct(u, target, target);
            return true;
        }
        let mut next = target;
        if !self.path_find(d, u, target, &mut next) && self.units[u as usize].path == super::list::NIL {
            return false;
        }
        self.set_next_waypoint(u, next);
        self.path_usable(u);
        true
    }
}

/// 0x4e2510: next leg from the unit's position along the stored region path.
fn continue_leg(st: &mut State, dims: &dyn Fn(u16) -> [i16; 4], u: u32, c: &mut PathCtx) -> bool {
    if c.start == c.target {
        return false;
    }
    c.start_region = region_at(&st.sai, c.start.0 as i32, c.start.1 as i32);
    c.target_region = region_at(&st.sai, c.target.0 as i32, c.target.1 as i32);
    c.goal = c.target;
    c.unreachable = 0;
    c.unk1d = 0;
    c.unk1e = 0;
    c.unk1f = 0;
    let ut = st.units[u as usize].unit_type;
    let [l, up, r, dn] = dims(ut);
    c.dims = [l.wrapping_neg(), up.wrapping_neg(), r, dn];
    c.nodes_used = 0;
    c.unk166 = 0;
    c.open_max = 0;
    c.unk16a = 0;
    let mut env = Env { sai: &mut st.sai, map_w: st.map_px.0, map_h: st.map_px.1, dims, unit_type: ut };
    next_leg(&mut env, &mut st.walker, c)
}
