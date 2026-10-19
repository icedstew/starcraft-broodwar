//! Fog of war shading of the terrain layer (1.16.1 mask.cpp): per-tile darkness levels
//! around the screen, blurred, spread to a grid of 8x8-pixel corners and applied with
//! bilinear interpolation through the tileset's dark.pcx (32 levels of 256 colors).

use crate::bw::State;

/// Full brightness (0x657a9c); fogged tiles get half of it, unexplored ones 0.
const FULL: u8 = 0x1f;
/// Tile levels: 24 x 17 tiles from one tile left of / above the screen (0x6d5c14,
/// blurred into 0x6d5c0c).
const LW: usize = 0x18;
const LH: usize = 0x11;
/// Corner grid 0x6d5c18: 0x58 bytes per row of 8-pixel corners, 56 rows.
const GW: usize = 0x58;
const GH: usize = 0x38;

pub struct Fog {
    /// 0x657aa0 (built by 0x480430): for left level a and right level b, the levels at
    /// the 4 corners between them.
    interp: Vec<[u8; 4]>,
}

impl Fog {
    /// 0x480430
    pub fn new() -> Fog {
        let mut interp = vec![[0u8; 4]; 32 * 32];
        let mut back = 0u32;
        let mut a = 0u32;
        let mut e = interp.iter_mut();
        while a < 0x2000 {
            let mut d = back;
            for _ in 0..32 {
                let q = d >> 2;
                let v = e.next().unwrap();
                v[0] = (a >> 8) as u8;
                v[1] = (a.wrapping_add(q) >> 8) as u8;
                let two = a.wrapping_add(q).wrapping_add(q);
                v[2] = (two >> 8) as u8;
                v[3] = (q.wrapping_add(two) >> 8) as u8;
                d = d.wrapping_add(0x100);
            }
            a += 0x100;
            back = back.wrapping_sub(0x100);
        }
        Fog { interp }
    }

    /// 0x47fc50: darkness of the tiles around the screen at tile (tx, ty), then the
    /// 3x3 blur (weights 1 2 1 / 2 4 2 / 1 2 1, / 16).
    fn tile_levels(st: &State, tx: i32, ty: i32) -> [u8; LW * LH] {
        let (w, h) = (st.terrain.width as u32, st.terrain.height as u32);
        let mask = st.local_vision_mask as u32;
        let mut raw = [0u8; LW * LH];
        let mut row_y = (ty - 1) as u32;
        let mut tile_row = (ty - 1).max(0) as u32;
        for r in 0..LH {
            let mut col_x = (tx - 1) as u32;
            let mut tile = tile_row * w + (tx - 1).max(0) as u32;
            for c in 0..LW {
                let f = st.terrain.tile_flags.get(tile as usize).copied().unwrap_or(0);
                raw[r * LW + c] = if st.replay {
                    // 0x6d0f1c (see everything): full brightness.
                    if st.reveal_all {
                        FULL
                    } else if !f & (mask << 8) == 0 {
                        0
                    } else if mask & !f == 0 {
                        FULL >> 1
                    } else {
                        FULL
                    }
                } else if f & (mask << 8) != 0 {
                    0
                } else if f & mask != 0 {
                    FULL >> 1
                } else {
                    FULL
                };
                // The original only steps right/down while inside the map (and not at
                // all from tile -1 at the left/top edge, an unsigned compare).
                if col_x < w - 1 {
                    tile += 1;
                }
                col_x = col_x.wrapping_add(1);
            }
            if row_y < h - 1 {
                tile_row += 1;
            }
            row_y = row_y.wrapping_add(1);
        }
        let mut out = [0u8; LW * LH];
        for r in 1..16 {
            for c in 1..23 {
                let p = |dr: isize, dc: isize| raw[(r as isize + dr) as usize * LW + (c as isize + dc) as usize] as u32;
                let s = p(-1, -1)
                    + (p(-1, 0) + p(0, 0) * 2 + p(1, 0) + p(0, -1) + p(0, 1)) * 2
                    + p(1, 1)
                    + p(-1, 1)
                    + p(1, -1);
                out[r * LW + c] = (s >> 4) as u8;
            }
        }
        out
    }

    /// 0x47fe10: the 8-pixel corner grid from the blurred tile levels.
    fn corner_grid(&self, lv: &[u8; LW * LH]) -> Vec<u8> {
        let mut g = vec![0u8; GW * GH];
        for tr in 0..14 {
            for tc in 0..21 {
                let at = |r: usize, c: usize| lv[(r + 1) * LW + c + 1] as i32;
                let (c0, c1, c2, c3) = (at(tr, tc), at(tr, tc + 1), at(tr + 1, tc), at(tr + 1, tc + 1));
                let (mut left, mut right) = (c0 * 0x100, c1 * 0x100);
                let dl = (c2 - c0) * 0x100 / 4;
                let dr = (c3 - c1) * 0x100 / 4;
                for k in 0..4 {
                    let v = self.interp[((left >> 8) * 0x20 + (right >> 8)) as usize & 0x3ff];
                    let o = (tr * 4 + k) * GW + tc * 4;
                    g[o..o + 4].copy_from_slice(&v);
                    left += dl;
                    right += dr;
                }
            }
        }
        g
    }

    /// 0x4808e0 / 0x4805f0: shade the terrain in `out` (`w` wide, the top 400 rows at
    /// most) for the screen at map position (vx, vy).
    /// 0x480000: remap an 8x8 block of one level through dark.pcx (0x5973a0), the `l =>`
    /// arm (inline here). 0x4800a0: zero an 8x8 block, the `0 =>` arm (inline here).
    /// 0x4804d0: compares the blurred levels 0x6d5c0c with the previous frame's 0x6d5c10
    /// to mark dirty 32-pixel runs (0x41e0d0); with a full redraw it has no pixel
    /// effect, so it is not ported.
    pub fn shade(&self, st: &State, dark: &[u8], vx: i32, vy: i32, out: &mut [u8], w: usize, h: usize) {
        let lv = Fog::tile_levels(st, vx >> 5, vy >> 5);
        let g = self.corner_grid(&lv);
        let (ox, oy) = (((vx >> 3) & 3) as usize, ((vy >> 3) & 3) as usize);
        let dk = |level: u8, px: u8| dark.get(level as usize * 256 + px as usize).copied().unwrap_or(px);
        for by in 0..h.min(400) / 8 {
            for bx in 0..w / 8 {
                let i = (by + oy) * GW + bx + ox;
                if i + GW + 1 >= g.len() {
                    continue;
                }
                let (tl, tr, bl, br) = (g[i] as i32, g[i + 1] as i32, g[i + GW] as i32, g[i + GW + 1] as i32);
                let base = by * 8 * w + bx * 8;
                if tl == tr && tl == bl && tl == br {
                    match tl {
                        0 => (0..8).for_each(|y| out[base + y * w..base + y * w + 8].fill(0)),
                        0x1f => {}
                        l => (0..8).for_each(|y| out[base + y * w..base + y * w + 8].iter_mut().for_each(|p| *p = dk(l as u8, *p))),
                    }
                    continue;
                }
                // 0x47ff10: levels interpolated down the left and right edges, then
                // across each row.
                let (mut left, mut right) = (tl << 8, tr << 8);
                for y in 0..8 {
                    let step = (left - right) >> 3;
                    for x in 0..8 {
                        let l = ((right + (7 - x as i32) * step) as u32 >> 8) as u8;
                        let p = &mut out[base + y * w + x];
                        *p = dk(l, *p);
                    }
                    left += (bl - tl) * 0x20;
                    right += (br - tr) * 0x20;
                }
            }
        }
    }
}
