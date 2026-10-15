//! Unit finder: units' bounding boxes sorted by x (0x66ff78) and y (0x6769b8).
//!
//! Each unit owns two entries per axis (left/right at CUnit +0x13c/+0x140,
//! top/bottom at +0x144/+0x148). Entries are (unit index + 1, coordinate).
//! Both arrays share one count (0x66ff74).

use super::{Data, State};

#[derive(Clone, Debug)]
pub struct Finder {
    pub x: Vec<(u32, i32)>,
    pub y: Vec<(u32, i32)>,
    /// 0x66ff74
    pub count: usize,
}

impl Finder {
    pub fn new(max_units: usize) -> Finder {
        Finder { x: vec![(0, 0); max_units * 2 + 2], y: vec![(0, 0); max_units * 2 + 2], count: 0 }
    }

    /// Coordinate of x entry `i`. A unit outside the finder has index -1, and 1.16.1
    /// then reads the dword before the array: the count (0x66ff74).
    pub fn x_value(&self, i: i32) -> i32 {
        if i < 0 { self.count as i32 } else { self.x[i as usize].1 }
    }

    /// Coordinate of y entry `i`; index -1 reads the last x entry (0x6769b4, x[3399]).
    pub fn y_value(&self, i: i32) -> i32 {
        if i < 0 { self.x[3399].1 } else { self.y[i as usize].1 }
    }
}

/// 0x469b00: binary search for the insert position of `value`.
/// `before_equal` (EDX != 0) places it before entries with the same value.
pub(crate) fn search(a: &[(u32, i32)], count: usize, before_equal: bool, value: i32) -> usize {
    if count == 0 {
        return 0;
    }
    let key = if before_equal { -1 } else { 1 } + value.wrapping_mul(2);
    let (mut lo, mut hi) = (0usize, count);
    let mut mid = count >> 1;
    loop {
        if key < a[mid].1.wrapping_mul(2) {
            hi = mid;
            mid = (mid + lo) >> 1;
        } else {
            lo = mid + 1;
            mid = (mid + hi + 1) >> 1;
        }
        if lo >= hi {
            return hi;
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Axis {
    X,
    Y,
}

impl State {
    fn finder_idx(&mut self, u: u32, slot: usize) -> &mut i32 {
        &mut self.units[u as usize].finder_index[slot]
    }

    fn finder_arr(&mut self, ax: Axis) -> &mut Vec<(u32, i32)> {
        match ax {
            Axis::X => &mut self.finder.x,
            Axis::Y => &mut self.finder.y,
        }
    }

    /// 0x469e50 (x) / 0x469de0 (y): open a hole at `at`, shifting later entries right.
    fn finder_shift(&mut self, ax: Axis, at: usize, count: usize) {
        if at >= count {
            return;
        }
        let (lo, hi) = match ax {
            Axis::X => (0, 1),
            Axis::Y => (2, 3),
        };
        for i in (at..count).rev() {
            let e = self.finder_arr(ax)[i].0;
            if e != 0 {
                let u = e - 1;
                if *self.finder_idx(u, lo) == i as i32 {
                    *self.finder_idx(u, lo) += 1;
                } else if *self.finder_idx(u, hi) == i as i32 {
                    *self.finder_idx(u, hi) += 1;
                }
            }
        }
        let a = self.finder_arr(ax);
        a.copy_within(at..count, at + 1);
    }

    /// 0x469c90 (x) / 0x469c20 (y): swap entry `i` with entry `i + dir`, fixing entry i's owner index.
    fn finder_swap(&mut self, ax: Axis, i: usize, dir: i32) {
        let (lo, hi) = match ax {
            Axis::X => (0, 1),
            Axis::Y => (2, 3),
        };
        let e = self.finder_arr(ax)[i].0;
        if e != 0 {
            let u = e - 1;
            if *self.finder_idx(u, lo) == i as i32 {
                *self.finder_idx(u, lo) += dir;
            } else if *self.finder_idx(u, hi) == i as i32 {
                *self.finder_idx(u, hi) += dir;
            }
        }
        let j = (i as i32 + dir) as usize;
        self.finder_arr(ax).swap(i, j);
    }

    fn finder_insert_one(&mut self, ax: Axis, u: u32, slot: usize, value: i32) {
        let count = self.finder.count;
        let at = search(self.finder_arr(ax), count, true, value);
        self.finder_shift(ax, at, count);
        *self.finder_idx(u, slot) = at as i32;
        self.finder_arr(ax)[at] = (u + 1, value);
    }

    /// 0x46a3a0: add a unit's box to the finder.
    pub fn finder_insert(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[ut] & 0x10 != 0 {
            return;
        }
        let s = self.units[u as usize].sprite;
        let (x, y) = (self.sprites[s as usize].x as i32, self.sprites[s as usize].y as i32);
        let dim = d.dat.units.dimensions[ut];
        let (l, t, r, b) = (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32);
        self.finder_insert_one(Axis::X, u, 0, x - l);
        self.finder_insert_one(Axis::Y, u, 2, y - t);
        self.finder.count += 1;
        self.finder_insert_one(Axis::X, u, 1, r + x);
        self.finder_insert_one(Axis::Y, u, 3, b + y);
        self.finder.count += 1;
    }

    /// 0x46a000: move a unit's finder entries by (dx, dy), keeping both arrays sorted.
    pub fn finder_move(&mut self, u: u32, dx: i32, dy: i32) {
        if self.units[u as usize].finder_index[0] < 0 || (dx == 0 && dy == 0) {
            return;
        }
        let fi = self.units[u as usize].finder_index;
        let x = &mut self.finder.x;
        x[fi[0] as usize].1 += dx;
        let left = x[fi[0] as usize].1;
        x[fi[1] as usize].1 += dx;
        let right = x[fi[1] as usize].1;
        let y = &mut self.finder.y;
        y[fi[2] as usize].1 += dy;
        let top = y[fi[2] as usize].1;
        y[fi[3] as usize].1 += dy;
        let bottom = y[fi[3] as usize].1;
        if dx > 0 {
            self.finder_bubble_up(Axis::X, u, 1, right);
            self.finder_bubble_up(Axis::X, u, 0, left);
        } else if dx < 0 {
            self.finder_bubble_down(Axis::X, u, 0, left);
            self.finder_bubble_down(Axis::X, u, 1, right);
        }
        if dy > 0 {
            self.finder_bubble_up(Axis::Y, u, 3, bottom);
            self.finder_bubble_up(Axis::Y, u, 2, top);
        } else if dy < 0 {
            self.finder_bubble_down(Axis::Y, u, 2, top);
            self.finder_bubble_down(Axis::Y, u, 3, bottom);
        }
    }

    fn finder_bubble_up(&mut self, ax: Axis, u: u32, slot: usize, value: i32) {
        while (self.units[u as usize].finder_index[slot] + 1) < self.finder.count as i32 {
            let i = self.units[u as usize].finder_index[slot] as usize;
            if value <= self.finder_arr(ax)[i + 1].1 {
                break;
            }
            self.finder_swap(ax, i + 1, -1);
            self.units[u as usize].finder_index[slot] += 1;
        }
    }

    fn finder_bubble_down(&mut self, ax: Axis, u: u32, slot: usize, value: i32) {
        while self.units[u as usize].finder_index[slot] > 0 {
            let i = self.units[u as usize].finder_index[slot] as usize;
            if value >= self.finder_arr(ax)[i - 1].1 {
                break;
            }
            self.finder_swap(ax, i - 1, 1);
            self.units[u as usize].finder_index[slot] -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_positions() {
        let a = [(1, 10), (2, 20), (3, 20), (4, 30), (0, 0)];
        assert_eq!(search(&a, 4, true, 20), 1);
        assert_eq!(search(&a, 4, false, 20), 3);
        assert_eq!(search(&a, 4, true, 5), 0);
        assert_eq!(search(&a, 4, true, 99), 4);
        assert_eq!(search(&a, 0, true, 7), 0);
    }

    #[test]
    fn move_keeps_sorted() {
        let mut st = State::new((8, 8), 0, 4);
        st.finder = Finder::new(4);
        // Two units with hand-built entries: u0 [0,10], u1 [20,30].
        st.finder.x[..4].copy_from_slice(&[(1, 0), (1, 10), (2, 20), (2, 30)]);
        st.finder.y[..4].copy_from_slice(&[(1, 0), (2, 0), (1, 10), (2, 10)]);
        st.finder.count = 4;
        st.units[0].finder_index = [0, 1, 0, 2];
        st.units[1].finder_index = [2, 3, 1, 3];
        st.finder_move(0, 25, 0);
        let xs: Vec<i32> = st.finder.x[..4].iter().map(|e| e.1).collect();
        assert_eq!(xs, vec![20, 25, 30, 35]);
        for u in 0..2u32 {
            for s in 0..2 {
                assert_eq!(st.finder.x[st.units[u as usize].finder_index[s] as usize].0, u + 1);
            }
        }
    }
}
