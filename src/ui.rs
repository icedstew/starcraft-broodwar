//! In-game UI: console overlay (SD DDS), bitmap fonts, minimap, resource bar.

use crate::casc::Casc;
use crate::grp::{Blend, Grp};
use std::io;

pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

/// DDS: uncompressed 32bpp (bitmasks) or DXT1/DXT5.
pub fn dds(d: &[u8]) -> io::Result<Rgba> {
    let r32 = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    if d.len() < 128 || &d[0..4] != b"DDS " {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a DDS"));
    }
    let (h, w) = (r32(12) as usize, r32(16) as usize);
    let four = &d[84..88];
    let body = &d[128..];
    let mut px = vec![[0u8; 4]; w * h];
    match four {
        b"DXT1" | b"DXT5" => {
            let bc3 = four == b"DXT5";
            let bs = if bc3 { 16 } else { 8 };
            for (bi, blk) in body.chunks_exact(bs).enumerate().take(w.div_ceil(4) * h.div_ceil(4)) {
                let (bx, by) = (bi % w.div_ceil(4) * 4, bi / w.div_ceil(4) * 4);
                let (alpha, c) = if bc3 { (Some(&blk[0..8]), &blk[8..16]) } else { (None, blk) };
                let cols = bc_colors(c, !bc3);
                let idx = u32::from_le_bytes(c[4..8].try_into().unwrap());
                for i in 0..16 {
                    let (x, y) = (bx + i % 4, by + i / 4);
                    if x < w && y < h {
                        let mut p = cols[(idx >> (i * 2)) as usize & 3];
                        if let Some(a) = alpha {
                            p[3] = bc3_alpha(a, i);
                        }
                        px[y * w + x] = p;
                    }
                }
            }
        }
        _ => {
            // Uncompressed: assume 32bpp with masks.
            let masks = [r32(92), r32(96), r32(100), r32(104)];
            let ch = |v: u32, m: u32| if m == 0 { 255 } else { ((v & m) >> m.trailing_zeros()) as u8 };
            for (i, p) in body.chunks_exact(4).take(w * h).enumerate() {
                let v = u32::from_le_bytes(p.try_into().unwrap());
                px[i] = [ch(v, masks[0]), ch(v, masks[1]), ch(v, masks[2]), ch(v, masks[3])];
            }
        }
    }
    Ok(Rgba { w, h, px })
}

fn bc_colors(c: &[u8], dxt1: bool) -> [[u8; 4]; 4] {
    let (c0, c1) = (u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]]));
    let rgb = |v: u16| [((v >> 11) as u32 * 255 / 31) as u8, ((v >> 5 & 63) as u32 * 255 / 63) as u8, ((v & 31) as u32 * 255 / 31) as u8, 255];
    let (a, b) = (rgb(c0), rgb(c1));
    let mix = |p: u32, q: u32, n: u32| -> [u8; 4] {
        let f = |i: usize| ((a[i] as u32 * p + b[i] as u32 * q) / n) as u8;
        [f(0), f(1), f(2), 255]
    };
    if c0 > c1 || !dxt1 { [a, b, mix(2, 1, 3), mix(1, 2, 3)] } else { [a, b, mix(1, 1, 2), [0, 0, 0, 0]] }
}

fn bc3_alpha(a: &[u8], i: usize) -> u8 {
    let (a0, a1) = (a[0] as u32, a[1] as u32);
    let bits = u64::from_le_bytes([a[2], a[3], a[4], a[5], a[6], a[7], 0, 0]);
    let k = (bits >> (i * 3)) as u32 & 7;
    (match (k, a0 > a1) {
        (0, _) => a0,
        (1, _) => a1,
        (k, true) => ((8 - k) * a0 + (k - 1) * a1) / 7,
        (6, false) => 0,
        (7, false) => 255,
        (k, false) => ((6 - k) * a0 + (k - 1) * a1) / 5,
    }) as u8
}

/// Classic FNT: "FONT", u8 low, u8 high, u8 max_w, u8 max_h, u32 offsets[], glyphs
/// {u8 w, h, x, y; RLE bytes (skip << 3 | color)}.
pub struct Font {
    d: Vec<u8>,
    low: u8,
    high: u8,
    pub height: i32,
}

impl Font {
    pub fn parse(d: Vec<u8>) -> Font {
        Font { low: d[4], high: d[5], height: d[7] as i32, d }
    }

    fn glyph(&self, c: u8) -> Option<usize> {
        if c < self.low || c > self.high {
            return None;
        }
        let o = 8 + (c - self.low) as usize * 4;
        let off = u32::from_le_bytes(self.d.get(o..o + 4)?.try_into().ok()?) as usize;
        (off != 0 && off + 4 <= self.d.len()).then_some(off)
    }

    /// Draw `s` at (x, y); shade 0..8 maps through `colors` (8 palette indices). Returns the end x.
    pub fn draw(&self, s: &str, mut x: i32, y: i32, colors: &[u8], pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) -> i32 {
        for c in s.bytes() {
            let Some(g) = self.glyph(c) else {
                x += self.height / 3;
                continue;
            };
            let (gw, gh, gx, gy) = (self.d[g] as i32, self.d[g + 1] as i32, self.d[g + 2] as i32, self.d[g + 3] as i32);
            let mut p = g + 4;
            let mut n = 0;
            while n < gw * gh && p < self.d.len() {
                let b = self.d[p];
                p += 1;
                n += (b >> 3) as i32;
                if n >= gw * gh {
                    break;
                }
                let (px, py) = (x + gx + n % gw, y + gy + n / gw);
                if px >= 0 && py >= 0 && px < dw && py < dh {
                    let [r, g2, b2] = pal[colors[(b & 7) as usize] as usize];
                    dst[(py * dw + px) as usize] = (r as u32) << 16 | (g2 as u32) << 8 | b2 as u32;
                }
                n += 1;
            }
            x += gw;
        }
        x
    }
}

pub fn blit_rgba(img: &Rgba, x: i32, y: i32, dst: &mut [u32], dw: i32, dh: i32) {
    for sy in 0..img.h as i32 {
        let py = y + sy;
        if py < 0 || py >= dh {
            continue;
        }
        for sx in 0..img.w as i32 {
            let px = x + sx;
            if px < 0 || px >= dw {
                continue;
            }
            let [r, g, b, a] = img.px[(sy as usize) * img.w + sx as usize];
            if a == 0 {
                continue;
            }
            let o = &mut dst[(py * dw + px) as usize];
            let blend = |s: u8, d: u32| ((s as u32 * a as u32 + d * (255 - a as u32)) / 255) & 0xFF;
            *o = blend(r, *o >> 16 & 0xFF) << 16 | blend(g, *o >> 8 & 0xFF) << 8 | blend(b, *o & 0xFF);
        }
    }
}

pub struct Ui {
    pub console: Rgba,
    pub font: Font,
    pub icons: Grp,
    pub cursor: Grp,
    pub wireframe: Grp,
    pub stat_txt: Vec<String>,
    /// tfontgam rows of 8 palette indices per text color.
    pub text_colors: Vec<u8>,
}

impl Ui {
    /// race: "terran" | "protoss" | "zerg"
    pub fn load(casc: &Casc, race: &str) -> io::Result<Ui> {
        let console = dds(&casc.read(&format!("sd/game/consoles/{race}/console.dds"))?)?;
        let font = Font::parse(casc.read("sd/font/font10.fnt")?);
        let icons = Grp::parse(casc.read("game/icons.grp")?);
        let cursor = Grp::parse(casc.read("cursor/arrow.grp")?);
        let wireframe = Grp::parse(casc.read("unit/wirefram/wirefram.grp")?);
        let stat_txt = crate::dat::parse_tbl(&casc.read("rez/stat_txt.tbl")?);
        let (_, _, text_colors) = crate::view::pcx(&casc.read("game/tfontgam.pcx")?);
        Ok(Ui { console, font, icons, cursor, wireframe, stat_txt, text_colors })
    }

    pub fn text_color(&self, c: usize) -> &[u8] {
        let o = (c * 8).min(self.text_colors.len().saturating_sub(8));
        &self.text_colors[o..o + 8]
    }

    pub fn draw_icon(&self, frame: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
        draw_grp_rgb(&self.icons, frame, x, y, pal, dst, dw, dh);
    }

    /// Cursor centered on (x, y), animating over `tick`.
    pub fn draw_cursor(&self, tick: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
        let f = (tick / 4) % self.cursor.frame_count().max(1);
        draw_grp_rgb(&self.cursor, f, x - self.cursor.w / 2, y - self.cursor.h / 2, pal, dst, dw, dh);
    }
}

/// Draw a palette-indexed GRP frame onto an RGB buffer (via a scratch buffer).
pub fn draw_grp_rgb(grp: &Grp, frame: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
    let (w, h) = (grp.w, grp.h);
    let mut tmp = vec![0u8; (w * h) as usize];
    grp.draw(frame, false, 0, 0, &mut tmp, w, h, Blend::Normal);
    for (i, &c) in tmp.iter().enumerate() {
        let (px, py) = (x + i as i32 % w, y + i as i32 / w);
        if c != 0 && px >= 0 && py >= 0 && px < dw && py < dh {
            let [r, g, b] = pal[c as usize];
            dst[(py * dw + px) as usize] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
    }
}
