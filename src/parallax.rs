//! The space platform's parallax stars (parallax\star.spk): five layers scrolled at their
//! own speeds behind the black (index 0) pixels of the game screen.

/// 0x512d20 - 4 * i: the layer speeds (8.8 fixed point per screen pixel).
const SPEED: [i32; 5] = [16, 21, 26, 31, 36];
/// 0x628490 / 0x62849c: the layer position ranges (648 x 488 pixels, 8.8 fixed point).
const RANGE: (i32, i32) = (0x28800, 0x1e800);

/// One star of star.spk: its position in the layer and its picture.
struct Star {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    px: Vec<u8>,
}

pub struct Parallax {
    /// The records in file order (0x658aa8).
    stars: Vec<Star>,
    /// 0x658ac4 + 4 * j: the record counts of the file's layers.
    counts: [u32; 5],
    /// 0x628484 - 4 * i / 0x62846c - 4 * i: the layer positions.
    pos: [(i32, i32); 5],
    /// 0x628448 / 0x628470: the screen position the layers were last moved for.
    prev: (i32, i32),
}

impl Parallax {
    /// 0x47f2c0 (from the game init 0x4bd6f0): read parallax\star.spk: u16 layer count,
    /// u16 record count per layer, then 8-byte records (u16 x, u16 y, u32 file offset of
    /// the picture: u16 width, u16 height, pixels). None when the file is bad (the game
    /// ends with an error).
    pub fn load(f: &[u8]) -> Option<Parallax> {
        let u16_at = |o: usize| f.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let layers = u16_at(0)? as usize;
        let mut counts = [0u32; 5];
        let mut total = 0usize;
        for j in 0..layers {
            let c = u16_at(2 + j * 2)?;
            // 0x658ac4 holds five counts; more layers would write past them.
            if let Some(v) = counts.get_mut(j) {
                *v = c as u32;
            }
            total += c as usize;
        }
        let base = 2 + layers * 2;
        let mut stars = Vec::with_capacity(total);
        for k in 0..total {
            let r = base + k * 8;
            let off = u32::from_le_bytes(f.get(r + 4..r + 8)?.try_into().ok()?) as usize;
            let (w, h) = (u16_at(off)?, u16_at(off + 2)?);
            let px = f.get(off + 4..off + 4 + w as usize * h as usize)?.to_vec();
            stars.push(Star { x: u16_at(r)?, y: u16_at(r + 2)?, w, h, px });
        }
        Some(Parallax { stars, counts, pos: [(0, 0); 5], prev: (0, 0) })
    }

    /// 0x49c440's tail: the screen moved to (`x`, `y`) (already clamped and 8-pixel
    /// aligned), so 0x49bfd0 moves each layer by its speed times the screen's move.
    pub fn scroll_to(&mut self, x: i32, y: i32) {
        if (x, y) == self.prev {
            return;
        }
        // 0x49bfd0 (C remainder: a layer position can go negative).
        let (dx, dy) = (self.prev.0 - x, self.prev.1 - y);
        for (i, p) in self.pos.iter_mut().enumerate() {
            p.0 = (SPEED[i] * dx + p.0) % RANGE.0;
            p.1 = (SPEED[i] * dy + p.1) % RANGE.1;
        }
        self.prev = (x, y);
    }

    /// 0x47ee20 (the full-screen redraw; 0x47ebf0 is the same for dirty cells only): every
    /// star onto the black pixels of the 640 x 400 game screen `out` (`w` wide). Layer i
    /// takes the next `counts[4 - i]` records in file order (the counts are read from the
    /// last layer back).
    pub fn draw(&self, out: &mut [u8], w: usize) {
        let mut next = 0;
        for i in 0..5 {
            let (lx, ly) = (self.pos[i].0 >> 8, self.pos[i].1 >> 8);
            for _ in 0..self.counts[4 - i] {
                let Some(s) = self.stars.get(next) else { return };
                next += 1;
                let wrap = |v: i32, m: i32| if v > m { v - m } else if v < 0 { v + m } else { v };
                let mut x = wrap(s.x as i32 + lx, 0x288) - 8;
                let (sw, sh) = (s.w as i32, s.h as i32);
                if x >= 0x280 || x + sw <= 0 {
                    continue;
                }
                let mut y = wrap(s.y as i32 + ly, 0x1e8) - 8;
                if y >= 0x190 || y + sh <= 0 {
                    continue;
                }
                let mut src = 0i32;
                let mut cols = sw;
                if x < 0 {
                    cols = x + sw;
                    src -= x;
                    x = 0;
                } else if x + sw >= 0x280 {
                    cols = 0x280 - x;
                }
                let mut rows = sh;
                if y < 0 {
                    rows = y + sh;
                    src -= sw * y;
                    y = 0;
                } else if y + sh >= 0x190 {
                    rows = 0x190 - y;
                }
                // 0x47ea60: a star pixel shows only where the screen is black.
                for r in 0..rows {
                    for c in 0..cols {
                        let o = (y + r) as usize * w + (x + c) as usize;
                        let Some(d) = out.get_mut(o) else { continue };
                        if *d == 0 {
                            *d = s.px[(src + r * sw + c) as usize];
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_and_draw() {
        // One layer-0 star (count read for layer 4 first: put it in the last count).
        let mut f = vec![];
        f.extend_from_slice(&5u16.to_le_bytes());
        for c in [0u16, 0, 0, 0, 1] {
            f.extend_from_slice(&c.to_le_bytes());
        }
        let img = (f.len() + 8) as u32;
        f.extend_from_slice(&[20, 0, 30, 0]);
        f.extend_from_slice(&img.to_le_bytes());
        f.extend_from_slice(&[1, 0, 1, 0, 7]);
        let mut p = Parallax::load(&f).unwrap();
        let mut out = vec![0u8; 640 * 400];
        p.draw(&mut out, 640);
        assert_eq!(out[22 * 640 + 12], 7);
        // Moving the screen 32 px right moves layer 0 (speed 16) 2 px left.
        p.scroll_to(32, 0);
        out.fill(0);
        p.draw(&mut out, 640);
        assert_eq!(out[22 * 640 + 10], 7);
        out.fill(1);
        p.draw(&mut out, 640);
        assert!(!out.contains(&7));
    }
}
