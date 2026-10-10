//! Ground pathing.
//!
//! ponytail: A* over the 8x8 minitile walkability grid with line-of-sight
//! smoothing. BW builds regions (SAI) at map load and paths region-to-region;
//! port that from the binary when exact path shapes matter (replay sync).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub struct WalkGrid {
    pub w: usize,
    pub h: usize,
    pub walk: Vec<bool>,
}

impl WalkGrid {
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && self.walk[y as usize * self.w + x as usize]
    }

    /// Bresenham walk between minitiles; true if every cell is walkable.
    fn line_clear(&self, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) -> bool {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            if !self.walkable(x, y) {
                return false;
            }
            if x == x1 && y == y1 {
                return true;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    fn nearest_walkable(&self, (x, y): (i32, i32)) -> Option<(i32, i32)> {
        for r in 0i32..32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) == r && self.walkable(x + dx, y + dy) {
                        return Some((x + dx, y + dy));
                    }
                }
            }
        }
        None
    }

    /// Pixel path from `from` to `to` as waypoints (excluding the start). None = unreachable.
    pub fn find(&self, from: (i32, i32), to: (i32, i32)) -> Option<Vec<(i16, i16)>> {
        let cell = |p: (i32, i32)| (p.0 >> 3, p.1 >> 3);
        let start = self.nearest_walkable(cell(from))?;
        let goal_cell = cell(to);
        let goal = if self.walkable(goal_cell.0, goal_cell.1) { goal_cell } else { self.nearest_walkable(goal_cell)? };
        if self.line_clear(start, goal) {
            return Some(vec![if goal == goal_cell { (to.0 as i16, to.1 as i16) } else { center(goal) }]);
        }
        let idx = |(x, y): (i32, i32)| y as usize * self.w + x as usize;
        let h = |(x, y): (i32, i32)| {
            let (dx, dy) = ((x - goal.0).abs(), (y - goal.1).abs());
            (dx.max(dy) * 10 + dx.min(dy) * 4) as u32
        };
        let mut g = vec![u32::MAX; self.w * self.h];
        let mut parent = vec![u32::MAX; self.w * self.h];
        let mut open = BinaryHeap::new();
        g[idx(start)] = 0;
        open.push(Reverse((h(start), start.0, start.1)));
        let mut expanded = 0;
        while let Some(Reverse((_, x, y))) = open.pop() {
            if (x, y) == goal {
                break;
            }
            expanded += 1;
            if expanded > 200_000 {
                return None;
            }
            let gc = g[idx((x, y))];
            for (dx, dy, c) in [(1, 0, 10), (-1, 0, 10), (0, 1, 10), (0, -1, 10), (1, 1, 14), (1, -1, 14), (-1, 1, 14), (-1, -1, 14)] {
                let n = (x + dx, y + dy);
                // No corner cutting.
                if !self.walkable(n.0, n.1) || (dx != 0 && dy != 0 && (!self.walkable(x + dx, y) || !self.walkable(x, y + dy))) {
                    continue;
                }
                let ng = gc + c;
                if ng < g[idx(n)] {
                    g[idx(n)] = ng;
                    parent[idx(n)] = idx((x, y)) as u32;
                    open.push(Reverse((ng + h(n), n.0, n.1)));
                }
            }
        }
        if parent[idx(goal)] == u32::MAX {
            return None;
        }
        let mut cells = vec![goal];
        let mut c = idx(goal);
        while c != idx(start) {
            c = parent[c] as usize;
            cells.push(((c % self.w) as i32, (c / self.w) as i32));
        }
        cells.reverse();
        // String-pull: keep the farthest visible cell each step.
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < cells.len() {
            let mut j = cells.len() - 1;
            while j > i + 1 && !self.line_clear(cells[i], cells[j]) {
                j -= 1;
            }
            out.push(center(cells[j]));
            i = j;
        }
        if goal == goal_cell {
            if let Some(last) = out.last_mut() {
                *last = (to.0 as i16, to.1 as i16);
            }
        }
        Some(out)
    }
}

fn center((x, y): (i32, i32)) -> (i16, i16) {
    ((x * 8 + 4) as i16, (y * 8 + 4) as i16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_around_wall() {
        // 10x10 grid with a wall at x=5 except y=9.
        let mut walk = vec![true; 100];
        for y in 0..9 {
            walk[y * 10 + 5] = false;
        }
        let g = WalkGrid { w: 10, h: 10, walk };
        let p = g.find((4, 4), (76, 4)).unwrap();
        assert!(p.len() >= 2, "{p:?}");
        assert_eq!(*p.last().unwrap(), (76, 4));
        assert!(p.iter().any(|&(_, y)| y >= 72), "{p:?}");
    }
}
