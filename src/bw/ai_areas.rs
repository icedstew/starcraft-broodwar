//! Resource areas (0x692688, 0x30 bytes each, 1-based, count 0x695568): one per start
//! location and one per cluster of mineral fields and geysers, made at game start
//! (0x446350) and recounted every 0x1c2 frames (0x445610). Resources remember their
//! area in unit +0xd8. The AI's towns take their resources from them.

use super::list::NIL;
use super::path::approx_distance;
use super::State;

/// One resource area.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceArea {
    /// +0
    pub pos: (i16, i16),
    /// +4 / +5
    pub mineral_fields: u8,
    pub geysers: u8,
    /// +6: a start location.
    pub start: u8,
    /// +7: bit 0 a building stands at it (a player starts there), bit 1 a town has it.
    pub flags: u8,
    /// +8 / +0xc: resources left.
    pub minerals: u32,
    pub gas: u32,
}

/// 0x692688 .. 0x6955e0: 250 slots, slot 0 unused.
pub const AREA_SLOTS: usize = 0xfa;

fn is_mineral(t: u16) -> bool {
    (0xb0..=0xb2).contains(&t)
}

impl State {
    fn visible_list(&self) -> Vec<u32> {
        let mut v = Vec::new();
        let mut u = self.visible_units.head;
        while u != NIL {
            v.push(u);
            u = self.units[u as usize].link.next;
        }
        v
    }

    fn alive(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        un.sprite != NIL && un.order != 0
    }

    fn is_resource(&self, d: &super::Data, u: u32) -> bool {
        d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000 != 0
    }

    fn group_at(&self, p: (i16, i16)) -> u16 {
        self.sai.regions[super::path::region_at(&self.sai, p.0 as i32, p.1 as i32) as usize].group
    }

    fn level_at(&self, p: (i16, i16)) -> u8 {
        self.los_elevation(p.0 as i32, p.1 as i32)
    }

    /// 0x446350: the start locations become areas (flag 1 when a human or computer
    /// starts there), resources near one join it, the rest form new areas (geysers
    /// first, 0x445e80).
    pub fn init_resource_areas(&mut self, d: &super::Data) {
        self.areas = vec![ResourceArea::default(); AREA_SLOTS];
        self.area_count = 0;
        for p in 0..8 {
            let s = self.start_locations[p];
            if s != (0, 0) {
                self.area_count += 1;
                let a = &mut self.areas[self.area_count as usize];
                a.pos = s;
                a.start = 1;
                if matches!(self.player_types[p], 1 | 2) {
                    a.flags |= 1;
                }
            }
        }
        let units = self.visible_list();
        let mut marks = vec![0u8; units.len()];
        for (i, &u) in units.iter().enumerate() {
            if !self.alive(u) || !self.is_resource(d, u) {
                marks[i] = 0;
                continue;
            }
            let mineral = is_mineral(self.units[u as usize].unit_type);
            marks[i] = if mineral { 1 } else { 2 };
            self.units[u as usize].set_u8_at(0xd8, 0);
            let area = self.start_area_for(u);
            if area != 0 {
                let amount = self.units[u as usize].resource_amount() as u32;
                let a = &mut self.areas[area as usize];
                if mineral {
                    a.minerals = a.minerals.wrapping_add(amount);
                    a.mineral_fields = a.mineral_fields.wrapping_add(1);
                    marks[i] = 3;
                } else {
                    a.gas = a.gas.wrapping_add(amount);
                    a.geysers = a.geysers.wrapping_add(1);
                    marks[i] = 4;
                }
                self.units[u as usize].set_u8_at(0xd8, area as u8);
            }
        }
        self.form_areas(d, &units, &mut marks, 2);
        self.form_areas(d, &units, &mut marks, 1);
    }

    /// 0x445930: the first area resource `u` belongs to: close (0x280 for minerals,
    /// 0x380 for geysers), same region group and elevation, and few path tiles away
    /// (0x14 / 0x1c).
    fn start_area_for(&mut self, u: u32) -> u32 {
        let up = self.unit_sprite_pos(u);
        let mineral = is_mineral(self.units[u as usize].unit_type);
        for i in 1..=self.area_count {
            let ap = self.areas[i as usize].pos;
            let dist = approx_distance(ap.0 as i32, up.0 as i32, ap.1 as i32, up.1 as i32);
            if dist > if mineral { 0x280 } else { 0x380 } {
                continue;
            }
            if self.group_at(up) != self.group_at(ap) || self.level_at(up) != self.level_at(ap) {
                continue;
            }
            let tiles = self.ai_path_tiles((ap.0 as i32, ap.1 as i32), (up.0 as i32, up.1 as i32));
            if tiles <= if mineral { 0x14 } else { 0x1c } {
                return i;
            }
        }
        0
    }

    /// 0x445820: resource `c` (not in an area yet) inside `lo`..`hi` belongs with the
    /// resource at `r`: same region group and elevation, close and few path tiles away.
    fn joins_cluster(&mut self, d: &super::Data, c: u32, r: (i16, i16), lo: (i16, i16), hi: (i16, i16)) -> bool {
        if !self.is_resource(d, c) || self.units[c as usize].u8_at(0xd8) != 0 {
            return false;
        }
        let cp = self.unit_sprite_pos(c);
        if cp.0 < lo.0 || cp.0 > hi.0 || cp.1 < lo.1 || cp.1 > hi.1 {
            return false;
        }
        if !self.same_group(c, r) || self.level_at(cp) != self.level_at(r) {
            return false;
        }
        let mineral = is_mineral(self.units[c as usize].unit_type);
        if approx_distance(r.0 as i32, cp.0 as i32, r.1 as i32, cp.1 as i32) > if mineral { 0x280 } else { 0x380 } {
            return false;
        }
        let tiles = self.ai_path_tiles((r.0 as i32, r.1 as i32), (cp.0 as i32, cp.1 as i32));
        tiles <= if mineral { 0x14 } else { 0x1c }
    }

    /// 0x445b00: 1 + for each resource in the box that would join `u`'s cluster, its
    /// amount / 500 + 1 (at most 4).
    fn cluster_score(&mut self, d: &super::Data, u: u32, lo: (i16, i16), hi: (i16, i16)) -> u32 {
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, [lo.0, lo.1, hi.0, hi.1]);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let r = self.unit_sprite_pos(u);
        let mut score = 1u32;
        for c in found {
            if self.joins_cluster(d, c, r, lo, hi) {
                let n = (self.units[c as usize].resource_amount() as u32 / 500 + 1).min(4);
                score += n;
            }
        }
        score
    }

    /// 0x445e80: resources marked `kind` (1 minerals, 2 geysers) not near a start
    /// location form new areas, the biggest clusters first; an area is centred on the
    /// box of its resources, seeded at the mineral field closest to the cluster's centre.
    fn form_areas(&mut self, d: &super::Data, units: &[u32], marks: &mut [u8], kind: u8) {
        struct Cand {
            lo: (i16, i16),
            hi: (i16, i16),
            score: u32,
            unit: u32,
        }
        let around = |p: (i16, i16)| ((p.0.wrapping_sub(0x3c0), p.1.wrapping_sub(0x3c0)), (p.0.wrapping_add(0x3c0), p.1.wrapping_add(0x3c0)));
        let mut cands = Vec::new();
        for (i, &u) in units.iter().enumerate() {
            if marks[i] == kind {
                let (lo, hi) = around(self.unit_sprite_pos(u));
                let score = self.cluster_score(d, u, lo, hi);
                cands.push(Cand { lo, hi, score, unit: u });
            }
        }
        let n = cands.len();
        for _ in 0..n {
            let mut pick = 0;
            let mut best = cands[0].score;
            for (j, c) in cands.iter().enumerate().skip(1) {
                if c.score > best {
                    best = c.score;
                    pick = j;
                }
            }
            cands[pick].score = 0;
            let (lo, hi) = (cands[pick].lo, cands[pick].hi);
            let r = self.unit_sprite_pos(cands[pick].unit);
            let (mut sx, mut sy, mut count) = (0u32, 0u32, 0u32);
            for &c in units {
                if self.joins_cluster(d, c, r, lo, hi) {
                    let p = self.unit_sprite_pos(c);
                    sx = sx.wrapping_add(p.0 as i32 as u32);
                    sy = sy.wrapping_add(p.1 as i32 as u32);
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            if self.area_count >= 0xf9 {
                return;
            }
            self.area_count += 1;
            let ai = self.area_count as usize;
            let seed = self.nearest_marked_mineral(units, marks, cands[pick].unit, (sx / count) as i32, (sy / count) as i32);
            let sp = self.unit_sprite_pos(seed);
            self.areas[ai].pos = sp;
            let (lo, hi) = around(sp);
            cands[pick].lo = lo;
            cands[pick].hi = hi;
            cands[pick].unit = seed;
            let (mut minx, mut maxx, mut miny, mut maxy) = (sp.0 as i32, sp.0 as i32, sp.1 as i32, sp.1 as i32);
            for (i, &c) in units.iter().enumerate() {
                if !self.joins_cluster(d, c, sp, lo, hi) {
                    continue;
                }
                let amount = self.units[c as usize].resource_amount() as u32;
                let a = &mut self.areas[ai];
                match marks[i] {
                    1 => {
                        marks[i] = 3;
                        a.mineral_fields = a.mineral_fields.wrapping_add(1);
                        a.minerals = a.minerals.wrapping_add(amount);
                    }
                    2 => {
                        marks[i] = 4;
                        a.geysers = a.geysers.wrapping_add(1);
                        a.gas = a.gas.wrapping_add(amount);
                    }
                    _ => continue,
                }
                self.units[c as usize].set_u8_at(0xd8, self.area_count as u8);
                let p = self.unit_sprite_pos(c);
                minx = minx.min(p.0 as i32);
                maxx = maxx.max(p.0 as i32);
                miny = miny.min(p.1 as i32);
                maxy = maxy.max(p.1 as i32);
            }
            self.areas[ai].pos = (((minx + maxx) / 2) as i16, ((miny + maxy) / 2) as i16);
        }
    }

    /// 0x4447d0: the unmarked mineral field (mark 1) at most 0x14 path tiles from
    /// (x, y), the closest (later ones win ties); else `fallback`.
    fn nearest_marked_mineral(&mut self, units: &[u32], marks: &[u8], fallback: u32, x: i32, y: i32) -> u32 {
        let mut best = 99_999_999u32;
        let mut found = NIL;
        for (i, &u) in units.iter().enumerate() {
            if marks[i] != 1 {
                continue;
            }
            let p = self.unit_sprite_pos(u);
            let t = self.ai_path_tiles((x, y), (p.0 as i32, p.1 as i32));
            if t <= best && t <= 0x14 {
                best = t;
                found = u;
            }
        }
        if found != NIL { found } else { fallback }
    }

    /// 0x445610: every 0x1c2 frames the areas' resources are recounted and areas get
    /// flag 1 when a building is near.
    pub(super) fn refresh_resource_areas(&mut self, d: &super::Data) {
        if self.area_count == 0 {
            return;
        }
        if self.area_timer != 0 {
            self.area_timer -= 1;
            return;
        }
        self.area_timer = 0x1c2;
        for a in self.areas[1..=self.area_count as usize].iter_mut() {
            a.minerals = 0;
            a.mineral_fields = 0;
            a.geysers = 0;
            a.gas = 0;
            a.flags &= !1;
        }
        for u in self.visible_list() {
            if !self.alive(u) {
                continue;
            }
            let t = self.units[u as usize].unit_type;
            let flags = d.dat.units.flags[t as usize];
            if flags & 0x2000 != 0 {
                let ai = self.units[u as usize].u8_at(0xd8) as usize;
                if ai == 0 {
                    continue;
                }
                let amount = self.units[u as usize].resource_amount() as u32;
                let a = &mut self.areas[ai];
                if is_mineral(t) {
                    a.minerals = a.minerals.wrapping_add(amount);
                    a.mineral_fields = a.mineral_fields.wrapping_add(1);
                } else {
                    a.gas = a.gas.wrapping_add(amount);
                    a.geysers = a.geysers.wrapping_add(1);
                }
            } else if flags & 1 != 0 {
                let up = self.unit_sprite_pos(u);
                for i in 1..=self.area_count as usize {
                    if self.areas[i].flags & 1 != 0 {
                        continue;
                    }
                    let ap = self.areas[i].pos;
                    if self.group_at(up) != self.group_at(ap) {
                        continue;
                    }
                    if approx_distance(up.0 as i32, ap.0 as i32, up.1 as i32, ap.1 as i32) > 0x180 {
                        continue;
                    }
                    if self.ai_path_tiles((up.0 as i32, up.1 as i32), (ap.0 as i32, ap.1 as i32)) <= 0xc {
                        self.areas[i].flags |= 1;
                        break;
                    }
                }
            }
        }
    }

    /// 0x443a20: the area closest to (x, y) (for a new AI without flag 0x20: of the 8
    /// first areas, to the player's start location). 0 when there are none.
    pub(super) fn nearest_resource_area(&self, player: u8, x: i16, y: i16) -> u32 {
        let (p, n) = if self.ai.players[player as usize].flags & 0x20 != 0 {
            ((x, y), self.area_count)
        } else {
            (self.start_locations[player as usize], 8)
        };
        let mut best = 99_999_999u32;
        let mut found = 0;
        for i in 1..=n {
            let a = self.areas[i as usize].pos;
            let dist = approx_distance(p.0 as i32, a.0 as i32, p.1 as i32, a.1 as i32);
            if dist <= best {
                best = dist;
                found = i;
            }
        }
        found
    }
}
