//! In-game UI: console overlay, bitmap fonts, minimap, resource bar.

use crate::storage::Storage;
use crate::grp::{Blend, Grp};
use std::io;

pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

/// An 8-bit PCX as RGBA with its own palette; palette index 0 is transparent.
pub fn pcx_rgba(d: &[u8]) -> io::Result<Rgba> {
    let (w, h, idx) = crate::view::pcx(d);
    let pal = d.len().checked_sub(768).filter(|&o| o > 0 && d[o - 1] == 0x0c).map(|o| &d[o..]);
    let pal = pal.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PCX without palette"))?;
    let px = idx.iter().map(|&i| {
        let c = &pal[i as usize * 3..i as usize * 3 + 3];
        [c[0], c[1], c[2], if i == 0 { 0 } else { 255 }]
    });
    Ok(Rgba { w, h, px: px.collect() })
}

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

    /// Width of `s` in pixels.
    pub fn width(&self, s: &str) -> i32 {
        s.bytes().map(|c| self.glyph(c).map_or(self.height / 3, |g| self.d[g] as i32)).sum()
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

/// 0x512708: cursor GRPs. Indices: 0 arrow, 1 illegal, 2..=5 targeting yellow / red /
/// green / none, 6..=8 magnifier green / red / yellow, 9 drag, 10 time, 11..=18 scroll
/// up, up-right, right, down-right, down, down-left, left, up-left.
pub const CURSORS: [&str; 19] = [
    "arrow.grp", "illegal.grp", "targy.grp", "targr.grp", "targg.grp", "targn.grp", "magg.grp", "magr.grp", "magy.grp",
    "drag.grp", "time.grp", "scrollu.grp", "scrollur.grp", "scrollr.grp", "scrolldr.grp", "scrolld.grp", "scrolldl.grp",
    "scrolll.grp", "scrollul.grp",
];

pub struct Ui {
    pub console: Rgba,
    pub font: Font,
    pub icons: Grp,
    /// The cursors in 1.16.1's order (0x512708, loaded by 0x4d1560).
    pub cursors: Vec<Grp>,
    pub wireframe: Grp,
    /// game\twire.pcx (0x68c208): the 24 wireframe colors.
    pub twire: Vec<u8>,
    pub cmdicons: Grp,
    /// ticon.pcx: 6 states x 16 colors for command icons.
    pub ticon: Vec<u8>,
    pub stat_txt: Vec<String>,
    /// tfontgam rows of 8 palette indices per text color.
    pub text_colors: Vec<u8>,
    /// rez\statdata.bin: the status panel dialog (statdata.cpp).
    pub statdata: crate::statdata::Dialog,
    /// unit\cmdbtns\<race>cmdbtns.grp (0x68c1c0, 0x459b90): slot frames and numbers.
    pub cmdbtns: Grp,
    /// game\<race>pbrempt.pcx / pbrfull.pcx: progress bar (0x458050).
    pub pbar_empty: Rgba,
    pub pbar_full: Rgba,
}

/// 0x4dab00: fonts are encrypted with the key in `font\font.gid` (grid.cpp).
fn read_font(files: &Storage, name: &str) -> io::Result<Vec<u8>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, format!("{name}: decryption failed"));
    let key = crate::grid::font_key(&files.read("font/font.gid")?).ok_or_else(bad)?;
    let mut d = files.read(name)?;
    let n = crate::grid::decrypt(&mut d, &key).ok_or_else(bad)?;
    d.truncate(n);
    Ok(d)
}

impl Ui {
    /// race: "terran" | "protoss" | "zerg"
    pub fn load(files: &Storage, race: &str) -> io::Result<Ui> {
        // game\[tpz]console.pcx by the local race.
        let console = pcx_rgba(&files.read(&format!("game/{}console.pcx", &race[..1]))?)?;
        let font = Font::parse(read_font(files, "font/font10.fnt")?);
        let icons = Grp::parse(files.read("game/icons.grp")?);
        let cursors = CURSORS.iter().map(|n| files.read(&format!("cursor/{n}")).map(Grp::parse)).collect::<io::Result<Vec<_>>>()?;
        let wireframe = Grp::parse(files.read("unit/wirefram/wirefram.grp")?);
        let twire = crate::view::pcx(&files.read("game/twire.pcx")?).2;
        let cmdicons = Grp::parse(files.read("unit/cmdbtns/cmdicons.grp")?);
        let ticon = crate::view::pcx(&files.read("unit/cmdbtns/ticon.pcx")?).2;
        let stat_txt = crate::dat::parse_tbl(&files.read("rez/stat_txt.tbl")?);
        let (_, _, text_colors) = crate::view::pcx(&files.read("game/tfontgam.pcx")?);
        let r = &race[..1];
        let statdata = crate::statdata::Dialog::parse(&files.read("rez/statdata.bin")?);
        let cmdbtns = Grp::parse(files.read(&format!("unit/cmdbtns/{r}cmdbtns.grp"))?);
        let pbar_empty = pcx_rgba(&files.read(&format!("game/{r}pbrempt.pcx"))?)?;
        let pbar_full = pcx_rgba(&files.read(&format!("game/{r}pbrfull.pcx"))?)?;
        Ok(Ui { console, font, icons, cursors, wireframe, twire, cmdicons, ticon, stat_txt, text_colors, statdata, cmdbtns, pbar_empty, pbar_full })
    }

    pub fn text_color(&self, c: usize) -> &[u8] {
        let o = (c * 8).min(self.text_colors.len().saturating_sub(8));
        &self.text_colors[o..o + 8]
    }

    pub fn draw_icon(&self, frame: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
        draw_grp_rgb(&self.icons, frame, x, y, pal, dst, dw, dh);
    }

    /// Command icon `frame` at (x, y); state 0 normal, 1 disabled, 2 highlighted.
    pub fn draw_cmd_icon(&self, frame: usize, state: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
        let g = &self.cmdicons;
        let (w, h) = (g.w, g.h);
        let mut tmp = vec![0u8; (w * h) as usize];
        g.draw(frame, false, 0, 0, &mut tmp, w, h, Blend::Normal);
        for (i, &c) in tmp.iter().enumerate() {
            let (px, py) = (x + i as i32 % w, y + i as i32 / w);
            if c != 0 && px >= 0 && py >= 0 && px < dw && py < dh {
                let idx = self.ticon.get(state * 16 + (c as usize & 15)).copied().unwrap_or(c);
                let [r, g2, b] = pal[idx as usize];
                dst[(py * dw + px) as usize] = (r as u32) << 16 | (g2 as u32) << 8 | b as u32;
            }
        }
    }

    /// Cursor centered on (x, y), animating over `tick`.
    /// Cursor `kind` (index into CURSORS) at (x, y): frame `counter` (0x597390, one step
    /// per 100 ms), hot spot at (63, 63) of the 128x128 GRP (0x4be120).
    pub fn draw_cursor(&self, kind: usize, counter: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
        let Some(g) = self.cursors.get(kind) else { return };
        let f = counter % g.frame_count().max(1);
        draw_grp_rgb(g, f, x - 0x3f, y - 0x3f, pal, dst, dw, dh);
    }
}

/// Draw a palette-indexed GRP frame onto an RGB buffer (via a scratch buffer).
pub fn draw_grp_rgb(grp: &Grp, frame: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32) {
    draw_grp_rgb_blend(grp, frame, x, y, pal, dst, dw, dh, Blend::Normal);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_grp_rgb_blend(grp: &Grp, frame: usize, x: i32, y: i32, pal: &[[u8; 3]; 256], dst: &mut [u32], dw: i32, dh: i32, blend: Blend) {
    let (w, h) = (grp.w, grp.h);
    let mut tmp = vec![0u8; (w * h) as usize];
    grp.draw(frame, false, 0, 0, &mut tmp, w, h, blend);
    for (i, &c) in tmp.iter().enumerate() {
        let (px, py) = (x + i as i32 % w, y + i as i32 / w);
        if c != 0 && px >= 0 && py >= 0 && px < dw && py < dh {
            let [r, g, b] = pal[c as usize];
            dst[(py * dw + px) as usize] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
    }
}

/// statwire.cpp 0x4568f0: the selected unit's wireframe colors. Wireframe pixels 0xc0 /
/// 0xc1 show shields (0x4566b0), 0xd0..0xd3 hp with the unit's random damage pattern
/// (0x4567c0), 0xd8..0xdb hp (0x456730); all from twire.pcx.
pub fn wireframe_colors(st: &crate::bw::State, d: &crate::bw::Data, u: u32, twire: &[u8]) -> [u8; 256] {
    const HP_ODD: [u8; 40] = [10, 10, 10, 10, 10, 10, 10, 0, 10, 10, 0, 0, 10, 10, 0, 1, 10, 10, 1, 1, 10, 0, 1, 1, 10, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1];
    const HP_EVEN: [u8; 40] = [10, 10, 10, 10, 10, 10, 10, 0, 10, 10, 0, 0, 10, 0, 0, 0, 10, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1];
    const SHIELDS: [u8; 16] = [2, 2, 9, 2, 8, 9, 7, 8, 6, 7, 5, 6, 4, 5, 3, 4];
    const HP4: [u8; 24] = [14, 6, 7, 8, 19, 11, 13, 6, 19, 11, 10, 16, 1, 18, 20, 16, 1, 18, 20, 12, 0, 1, 18, 10];
    let tw = |i: u8| twire.get(i as usize).copied().unwrap_or(0);
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = i as u8;
    }
    let un = &st.units[u as usize];
    let ty = wire_type(st, u);
    let mut max = d.dat.units.hit_points[un.unit_type as usize] as i32 >> 8;
    let hp = (un.hp + 0xff) >> 8;
    if max == 0 {
        max = hp.max(1);
    }
    // 0x4567c0
    let blank = tw(2);
    let mut c = [blank; 4];
    let row = (hp * 9 / max) as usize;
    let seed = un.wireframe_seed as u32;
    let tbl = if seed & 1 != 0 { &HP_ODD } else { &HP_EVEN };
    for k in 0..4 {
        let mut i = (((seed >> k) & 7) * 3 / 7) as usize;
        while c[i] != blank {
            i = (i + 1) % 4;
        }
        c[i] = tw(tbl[row * 4 + k]);
    }
    t[0xd0..0xd4].copy_from_slice(&c);
    // 0x4566b0
    let ms = d.dat.units.shields[ty as usize] as i32;
    if d.dat.units.shield_enable[un.unit_type as usize] != 0 && ms != 0 {
        let r = ((un.shields >> 8) * 7 / ms) as usize;
        t[0xc0] = tw(SHIELDS[r * 2]);
        t[0xc1] = tw(SHIELDS[r * 2 + 1]);
    } else {
        t[0xc0] = blank;
        t[0xc1] = blank;
    }
    // 0x456730
    let r = (hp * 5 / max) as usize;
    for k in 0..4 {
        t[0xd8 + k] = tw(HP4[r * 4 + k]);
    }
    t
}

/// 0x47b270: the type shown for `u` (an egg-like morph shows what it becomes).
pub fn wire_type(st: &crate::bw::State, u: u32) -> u16 {
    let un = &st.units[u as usize];
    let q = un.build_queue[un.build_queue_slot as usize % 5];
    if un.status & 1 == 0 && matches!(q, 0x85 | 0x84 | 0x89 | 0x90 | 0x92) { q } else { un.unit_type }
}
