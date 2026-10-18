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
