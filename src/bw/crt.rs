//! C runtime routines whose exact behavior the game depends on.

use std::cmp::Ordering;

/// MSVC (VS2003) `qsort` (0x40cff0), including its element order for equal keys.
pub fn qsort<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let gt = |c: Ordering| c == Ordering::Greater;
    if v.len() < 2 {
        return;
    }
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let (mut lo, mut hi) = (0usize, v.len() - 1);
    loop {
        let size = hi - lo + 1;
        if size <= 8 {
            shortsort(v, lo, hi, &mut cmp);
        } else {
            let mut mid = lo + size / 2;
            if gt(cmp(&v[lo], &v[mid])) {
                v.swap(lo, mid);
            }
            if gt(cmp(&v[lo], &v[hi])) {
                v.swap(lo, hi);
            }
            if gt(cmp(&v[mid], &v[hi])) {
                v.swap(mid, hi);
            }
            let mut loguy = lo as isize;
            let mut higuy = hi as isize;
            let m = |mid: usize| mid as isize;
            loop {
                if m(mid) > loguy {
                    loop {
                        loguy += 1;
                        if !(loguy < m(mid) && !gt(cmp(&v[loguy as usize], &v[mid]))) {
                            break;
                        }
                    }
                }
                if m(mid) <= loguy {
                    loop {
                        loguy += 1;
                        if !(loguy <= hi as isize && !gt(cmp(&v[loguy as usize], &v[mid]))) {
                            break;
                        }
                    }
                }
                loop {
                    higuy -= 1;
                    if !(higuy > m(mid) && gt(cmp(&v[higuy as usize], &v[mid]))) {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                v.swap(loguy as usize, higuy as usize);
                if m(mid) == higuy {
                    mid = loguy as usize;
                }
            }
            higuy += 1;
            if m(mid) < higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > m(mid) && cmp(&v[higuy as usize], &v[mid]) == Ordering::Equal) {
                        break;
                    }
                }
            }
            if m(mid) >= higuy {
                loop {
                    higuy -= 1;
                    if !(higuy > lo as isize && cmp(&v[higuy as usize], &v[mid]) == Ordering::Equal) {
                        break;
                    }
                }
            }
            if higuy - lo as isize >= hi as isize - loguy {
                if (lo as isize) < higuy {
                    stack.push((lo, higuy as usize));
                }
                if loguy < hi as isize {
                    lo = loguy as usize;
                    continue;
                }
            } else {
                if loguy < hi as isize {
                    stack.push((loguy as usize, hi));
                }
                if (lo as isize) < higuy {
                    hi = higuy as usize;
                    continue;
                }
            }
        }
        match stack.pop() {
            Some((l, h)) => (lo, hi) = (l, h),
            None => return,
        }
    }
}

/// 0x40cf80
fn shortsort<T>(v: &mut [T], lo: usize, mut hi: usize, cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    while hi > lo {
        let mut max = lo;
        for p in lo + 1..=hi {
            if cmp(&v[p], &v[max]) == Ordering::Greater {
                max = p;
            }
        }
        v.swap(max, hi);
        hi -= 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sorts_and_keeps_msvc_order_for_ties() {
        let mut v: Vec<(i32, u32)> = (0..200u32).map(|i| (((i * 7919) % 13) as i32, i)).collect();
        super::qsort(&mut v, |a, b| a.0.cmp(&b.0));
        assert!(v.windows(2).all(|w| w[0].0 <= w[1].0));
    }
}
