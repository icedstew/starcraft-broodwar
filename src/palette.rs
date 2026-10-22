//! The palette pipeline (0x41dc20, 0x41dcf0, 0x41e5c0, 0x41e8b0, 0x41e930): the game
//! palette 0x6ce320 is gamma-corrected on its way to the screen, and fades are blends
//! between two palettes (0x6ce720 -> 0x6ceb40) in steps of 10 ms.

pub type Pal = [[u8; 3]; 256];

/// 0x41dcf0: sets the options gamma 0x51a158: 60 ..= 140, else 100.
pub fn set_gamma(v: i32) -> i16 {
    if (0x3c..=0x8c).contains(&v) { v as i16 } else { 100 }
}

/// 0x41dc20 / 0x41e870: (eax src, edx dst, ecx 256) every channel becomes
/// `(c / 256) ^ ((200 - gamma) * 0.01) * 256`, truncated (the CRT's pow 0x4f7c30 and
/// _ftol 0x4f7b70); a gamma of 100 copies. 0x41e870 / 0x41e5c0 / 0x41ea30 apply it to
/// 0x6ce320 before setting the hardware palette (Ordinal_357), so every palette shown
/// goes through here. Uses f64 `powf` where the CRT has its own pow (last-bit
/// differences before the truncation are possible).
pub fn gamma(pal: &Pal, gamma: i16) -> Pal {
    if gamma == 100 {
        return *pal;
    }
    let e = (200 - gamma as i32) as f64 * 0.01;
    pal.map(|c| c.map(|v| ((v as f64 * 0.00390625).powf(e) * 256.0) as u8))
}

/// 0x41e930 (the grey target 0x6ceb40 of the pause): every color becomes the luminance
/// `(g * 0x97 >> 8) + (r * 0x4d >> 8) + (b * 0x1c >> 8)` (bytes wrap).
pub fn grey(pal: &Pal) -> Pal {
    pal.map(|[r, g, b]| {
        let v = ((g as u32 * 0x97 >> 8) + (r as u32 * 0x4d >> 8) + (b as u32 * 0x1c >> 8)) as u8;
        [v; 3]
    })
}

/// 0x41e5c0 (esi `total`, edi `step`): the palette `step` / `total` of the way from
/// `from` (0x6ce720) to `to` (0x6ceb40): `(to - from) * step / total + from` per channel
/// with C's truncating division. Not gamma-corrected.
pub fn blend(from: &Pal, to: &Pal, step: i32, total: i32) -> Pal {
    let mut out = *from;
    if total == 0 {
        return out;
    }
    for i in 0..256 {
        for k in 0..3 {
            let (f, t) = (from[i][k] as i32, to[i][k] as i32);
            out[i][k] = ((t - f) * step / total) as u8; // (char)
            out[i][k] = out[i][k].wrapping_add(f as u8);
        }
    }
    out
}

/// 0x41e8b0 (eax `n`): the fade's (step, total) pairs, one every 10 ms: `n` is capped at
/// 10; 0 is the one immediate (1, 1); else steps 0 ..= n of n.
pub fn fade_steps(n: u32) -> Vec<(i32, i32)> {
    match n.min(10) {
        0 => vec![(1, 1)],
        n => (0..=n as i32).map(|s| (s, n as i32)).collect(),
    }
}

/// 0x5127dc[tileset]: the color-cycling record of each tileset: the enable mask (word at +0)
/// and the ranges `(delay, first, last)` from +8 (8 bytes each, ended by a zero delay; the
/// pointer / length dword of an entry, which makes 0x41e4f0 play a table instead, is 0 in
/// all of them). Tilesets 1 and 2 point at the zeroed 0x6d1228: no ranges.
pub fn cycling_record(tileset: u16) -> (u16, &'static [(u8, u8, u8)]) {
    match tileset {
        0 | 4 => (7, &[(8, 1, 6), (8, 7, 13), (8, 248, 254)]),
        3 => (7, &[(8, 1, 4), (8, 5, 8), (8, 9, 12)]),
        5..=7 => (3, &[(8, 1, 13), (8, 248, 254)]),
        _ => (0, &[]),
    }
}

/// 0x4cb560 (eax `index`): whether a palette index lies inside a cycling range of the tileset
/// (0x4c99c0 builds the list of these 0x5994e0 from the same test).
pub fn in_cycle_range(tileset: u16, index: usize) -> bool {
    cycling_record(tileset).1.iter().any(|&(_, lo, hi)| (lo as usize..=hi as usize).contains(&index))
}

/// One 16-byte entry of 0x6ce2a0.
#[derive(Clone, Copy, Default)]
struct Range {
    /// +0: rotating.
    on: bool,
    /// +1: ticks per step; +2: ticks left.
    delay: u8,
    left: u8,
    /// +3 / +5.
    first: u8,
    last: u8,
}

/// The color-cycling state 0x6ce2a0 (8 ranges) with its palette: 0x6ce320 as rotated.
pub struct Cycle {
    ranges: [Range; 8],
    /// 0x6ce320 (before gamma / grey).
    pub pal: Pal,
    /// 0x6d6374: GetTickCount of the last step.
    last: Option<std::time::Instant>,
}

impl Cycle {
    /// 0x4cb5b0 (edx the record's entries): clears 0x6ce2a0, then one entry per range, on
    /// when its bit of the record's mask is set.
    pub fn new(tileset: u16, pal: Pal) -> Cycle {
        let mut ranges = [Range::default(); 8];
        let (mask, rec) = cycling_record(tileset);
        for (i, &(delay, first, last)) in rec.iter().enumerate() {
            ranges[i] = Range { on: mask >> i & 1 != 0, delay, left: delay, first, last };
        }
        Cycle { ranges, pal, last: None }
    }

    /// 0x4d93f0 (the palette part, run every pass of the game's loop): when the options'
    /// color cycling is on (0x5137d4) and 10 ms have passed, 0x41e900 steps the ranges.
    pub fn tick(&mut self, enabled: bool) {
        let now = std::time::Instant::now();
        if enabled && self.last.map_or(true, |t| now.duration_since(t).as_millis() > 9) {
            self.last = Some(now);
            self.step();
        }
    }

    /// 0x41e900 / 0x41e7b0: each range that is on counts down; at 0 it restarts and
    /// 0x41e4b0 rotates its colors by one (the last becomes the first).
    pub fn step(&mut self) {
        for r in self.ranges.iter_mut().take_while(|r| r.delay != 0) {
            if r.on {
                r.left = r.left.wrapping_sub(1);
                if r.left == 0 {
                    r.left = r.delay;
                    self.pal[r.first as usize..=r.last as usize].rotate_right(1);
                }
            }
        }
    }
}
