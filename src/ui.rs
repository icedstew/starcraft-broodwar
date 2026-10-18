//! In-game UI: console overlay, bitmap fonts, minimap, resource bar.

use crate::storage::Storage;
use crate::grp::{Blend, Grp};
use std::io;

/// An 8-bit PCX in the game palette; index 0 is transparent when blitted.
pub struct Pcx {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

pub fn pcx8(d: &[u8]) -> Pcx {
    let (w, h, px) = crate::view::pcx(d);
    Pcx { w, h, px }
}

/// Blit `img` at (x, y), skipping index 0.
pub fn blit(img: &Pcx, x: i32, y: i32, dst: &mut [u8], dw: i32, dh: i32) {
    blit_w(img, x, y, img.w as i32, dst, dw, dh)
}

/// Blit the left `cols` columns of `img` at (x, y), skipping index 0.
pub fn blit_w(img: &Pcx, x: i32, y: i32, cols: i32, dst: &mut [u8], dw: i32, dh: i32) {
    for sy in 0..img.h as i32 {
        let py = y + sy;
        if py < 0 || py >= dh {
            continue;
        }
        for sx in 0..cols.min(img.w as i32) {
            let px = x + sx;
            let c = img.px[sy as usize * img.w + sx as usize];
            if c != 0 && px >= 0 && px < dw {
                dst[(py * dw + px) as usize] = c;
            }
        }
    }
}

#[derive(Clone)]
pub struct Font {
    d: Vec<u8>,
    low: u8,
    high: u8,
    /// Header +6: the widest glyph; a space is half of it, a tab stop twice it.
    pub max_w: i32,
    pub height: i32,
}

/// The text engine's settings for one draw (the 0x6ce0c4..0x6ce111 globals).
#[derive(Clone, Copy, Debug)]
pub struct TextOpts {
    /// 0x6ce110: 1 left, 2 centered, 4 right; 8 left for one line and centered for
    /// several; 0x10 no vertical alignment; 0x20 centered vertically (else bottom when
    /// 0x40 / 8 measure first).
    pub flags: u8,
    /// 0x6ce0c4: pixels after each glyph (1; 0 for controls with flag 0x10000).
    pub spacing: i32,
    /// 0x6ce0d4: pixels between lines (3; 0 in tooltips 0x481510).
    pub line_gap: i32,
    /// 0x6ce0c8 / 0x6ce0cc: left, top, right, bottom (inclusive).
    pub clip: (i32, i32, i32, i32),
}

impl TextOpts {
    /// 0x41fb30's defaults: left aligned, 1 px spacing, 3 px line gap, no clip.
    pub fn new() -> TextOpts {
        TextOpts { flags: 0x11, spacing: 1, line_gap: 3, clip: (0, 0, 0x7fff, 0x7fff) }
    }
}

impl Default for TextOpts {
    fn default() -> Self {
        Self::new()
    }
}

/// A text's bounding box (inclusive).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ext {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

/// 0x41f610: the tfontgam.pcx row a color code selects (0x6ce000 + 8 * row).
pub fn color_row(code: u8) -> Option<usize> {
    Some(match code {
        2..=7 => code as usize - 2,
        8 => 8,
        0xe..=0x11 => code as usize - 5,
        0x15..=0x19 => code as usize - 8,
        0x1b..=0x1e => code as usize - 9,
        0x1f => 23,
        _ => return None,
    })
}

/// 0x41f270: the top of a text block `end_y - y` tall moved to the bottom of the clip
/// (`b`), or to its middle with flag 0x20.
fn valign(y: i32, end_y: i32, b: i32, flags: u8) -> i32 {
    let (mut th, mut room) = (end_y - y, b - y);
    if flags & 0x20 != 0 {
        room /= 2;
        th /= 2;
    }
    y - th + room
}

/// Color codes 0x41fd70 switches on (1 restores the previous color).
fn is_color_code(c: u8) -> bool {
    matches!(c, 1..=8 | 0xe..=0x11 | 0x15..=0x19 | 0x1b..=0x1f)
}

/// Where glyphs go: an 8-bit surface.
pub struct TextDst<'a> {
    /// tfontgam.pcx: 8 palette indices per color row.
    pub rows: &'a [u8],
    pub out: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

/// Running color state: 0x6ce0c0 (current row) and 0x6ce0e8 (previous row).
struct Color {
    cur: usize,
    prev: usize,
}

impl Color {
    /// 0x41f610.
    fn set(&mut self, code: u8) {
        if code == 1 {
            self.cur = self.prev;
            return;
        }
        self.prev = self.cur;
        if let Some(r) = color_row(code) {
            self.cur = r;
        }
    }
}

impl Font {
    pub fn parse(d: Vec<u8>) -> Font {
        Font { low: d[4], high: d[5], max_w: d[6] as i32, height: d[7] as i32, d }
    }

    fn glyph(&self, c: u8) -> Option<usize> {
        if c < self.low || c > self.high {
            return None;
        }
        let o = 8 + (c - self.low) as usize * 4;
        let off = u32::from_le_bytes(self.d.get(o..o + 4)?.try_into().ok()?) as usize;
        (off != 0 && off + 4 <= self.d.len()).then_some(off)
    }

    /// 0x41f7c0: advance of one character (codes 0, tab a full stop, space half the
    /// widest glyph, glyph width + x offset + spacing, missing glyphs 0).
    pub fn advance(&self, c: u8, spacing: i32) -> i32 {
        match c {
            9 => self.max_w * 2,
            0x20 => self.max_w >> 1,
            1..=0x1f => 0,
            _ => self.glyph(c).map_or(0, |g| self.d[g] as i32 + self.d[g + 2] as i32 + spacing),
        }
    }

    /// 0x41fcc0: whether the font has a glyph for `c`.
    pub fn has_glyph(&self, c: u8) -> bool {
        self.glyph(c).is_some()
    }

    /// `width` of bytes as stored (0x41fc80).
    pub fn width_bytes(&self, s: &[u8]) -> i32 {
        s.iter().map(|&c| self.advance(c, 1)).sum()
    }

    /// Width of `s` in pixels with the default 1 px spacing (no tabs or line breaks).
    pub fn width(&self, s: &str) -> i32 {
        s.bytes().map(|c| self.advance(c, 1)).sum()
    }

    /// 0x41f920 (word wrap, 0x51a0e8 = 1): the end of the line that starts at `i` with
    /// pen x `x`: (index after the line, the pen x 0x6ce108 at its end).
    fn line_end(&self, s: &[u8], mut i: usize, x0: i32, o: &TextOpts) -> (usize, i32) {
        let (left, right) = (o.clip.0, o.clip.2);
        let tab = |x: i32| x + (self.max_w * 2 - (x - left) % (self.max_w * 2));
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        let mut pen = x0;
        let mut x = x0;
        loop {
            let c = at(i);
            if c == 0 {
                return (i, pen);
            }
            match c {
                c if is_color_code(c) => {}
                b' ' => {
                    x += self.max_w >> 1;
                    pen = x;
                }
                b'\t' => {
                    x = tab(x);
                    pen = x;
                }
                b'\n' | 0x0c => return (i + 1, pen),
                0x1a | 0x0d | 0x12 | 0x13 => {}
                _ => {
                    if x > right {
                        return (i, pen);
                    }
                    let start = i;
                    let mut wx = x;
                    while !matches!(at(i), 0 | b' ' | b'\t' | b'\n' | 0x0c | 0x1a | 0x12 | 0x13) {
                        wx += self.advance(at(i), o.spacing);
                        i += 1;
                    }
                    if wx > right {
                        return (start, pen);
                    }
                    pen = wx;
                    x = wx;
                    continue;
                }
            }
            i += 1;
        }
    }

    /// 0x41f920 from pen x `x0`: the end of the line starting at `i`, at word breaks
    /// (0x51a0e8 = 1) or else at the first character past the clip's right edge.
    pub fn wrap(&self, s: &[u8], i: usize, x0: i32, o: &TextOpts, words: bool) -> usize {
        if words {
            return self.line_end(s, i, x0, o).0;
        }
        // ponytail: 0x4161c0's Korean double-byte step back is left out (LANGID 0x412).
        let left = o.clip.0;
        let mut pen = x0;
        let mut i = i;
        while let Some(&c) = s.get(i).filter(|&&c| c != 0) {
            match c {
                b' ' => pen += self.max_w >> 1,
                b'\t' => pen += self.max_w * 2 - (pen - left) % (self.max_w * 2),
                b'\n' | 0x0c => return i + 1,
                0x1a | 0x12 | 0x13 => {}
                _ => {
                    pen += self.advance(c, o.spacing);
                    if pen > o.clip.2 {
                        break;
                    }
                }
            }
            i += 1;
        }
        i
    }

    /// 0x41fd70: draw s[i..end] at (x, y); returns where it stopped (end, a NUL / form
    /// feed, or a character the font lacks). `pen` is 0x6ce108, the logical x.
    #[allow(clippy::too_many_arguments)]
    fn draw_run(&self, s: &[u8], mut i: usize, end: usize, x: i32, y: i32, pen: &mut i32, color: &mut Color, o: &TextOpts, dst: &mut Option<TextDst>, ext: &mut Ext) -> usize {
        // ponytail: the 0x6ce104 (0xa6 after code 4) deferred marker and the Korean
        // double-byte path are left out (LANGID 0x412 only).
        let mut dx = x - *pen;
        while i < end {
            let c = s.get(i).copied().unwrap_or(0);
            if c == 0 || c == 0x0c {
                break;
            }
            match c {
                c if is_color_code(c) => {
                    if dst.is_some() && color.cur != 3 {
                        color.set(c);
                    }
                }
                b'\n' | 0x0d | 0x1a => {}
                b'\t' => *pen += self.max_w * 2 - (*pen - o.clip.0) % (self.max_w * 2),
                b' ' => *pen += self.max_w >> 1,
                0x12 | 0x13 => {
                    let (_, e) = self.line_end(s, i, *pen, o);
                    let shift = o.clip.2 - e;
                    dx += if c == 0x12 { shift } else { shift >> 1 };
                    dx -= e - *pen;
                    *pen = e;
                }
                _ => {
                    let Some(g) = self.glyph(c) else { break };
                    // 0x6ce0de: the lowest glyph row.
                    ext.b = ext.b.max(self.d[g + 1] as i32 - 1 + self.d[g + 3] as i32 + y);
                    if let Some(t) = dst.as_mut() {
                        let rows = t.rows;
                        let row = rows.get(color.cur * 8..color.cur * 8 + 8).unwrap_or(&rows[..8]);
                        self.blit_glyph(g, *pen + dx, y, row, t);
                    }
                    *pen += self.advance(c, o.spacing);
                }
            }
            i += 1;
        }
        i
    }

    /// 0x410330: one glyph's RLE pixels (skip = byte >> 3, then shade byte & 7; a skip
    /// with shade 0 writes nothing).
    fn blit_glyph(&self, g: usize, x: i32, y: i32, rows: &[u8], t: &mut TextDst) {
        let (gw, gh, gx, gy) = (self.d[g] as i32, self.d[g + 1] as i32, self.d[g + 2] as i32, self.d[g + 3] as i32);
        let mut p = g + 4;
        let mut n = 0;
        while n < gw * gh && p < self.d.len() {
            let b = self.d[p];
            p += 1;
            let skip = (b >> 3) as i32;
            n += skip;
            if skip != 0 && b & 7 == 0 {
                continue;
            }
            if n >= gw * gh {
                break;
            }
            let (px, py) = (x + gx + n % gw, y + gy + n / gw);
            if px >= 0 && py >= 0 && px < t.w && py < t.h {
                t.out[(py * t.w + px) as usize] = rows[(b & 7) as usize];
            }
            n += 1;
        }
    }

    /// 0x4200d0: lay out (and draw when `dst` is set) the text from `start` at (x, y);
    /// returns (lines, y after the last line, where it stopped).
    fn layout(&self, s: &[u8], x: i32, mut y: i32, color: &mut Color, o: &TextOpts, dst: &mut Option<TextDst>, ext: &mut Ext) -> (usize, i32) {
        let (l, _, r, b) = o.clip;
        *ext = Ext { l: 0x7fff, t: y, r: 0, b: y };
        let mut lines = 1;
        let mut i = 0;
        let mut pen = x;
        loop {
            if s.get(i).copied().unwrap_or(0) == 0 {
                break;
            }
            let (end, e) = self.line_end(s, i, pen, o);
            let w = e - pen;
            let lx = if o.flags & 2 != 0 {
                let lx = (r - pen) / 2 - w / 2 + pen;
                ext.r = ext.r.max(lx + w);
                lx
            } else if o.flags & 4 != 0 {
                ext.r = r;
                (l + r - w) - pen
            } else {
                ext.r = ext.r.max(e);
                pen
            };
            ext.l = ext.l.min(lx);
            let mut p = pen;
            i = self.draw_run(s, i, end, lx, y, &mut p, color, o, dst, ext);
            pen = l;
            y += o.line_gap + self.height;
            ext.b += o.line_gap - 1;
            lines += 1;
            if s.get(i) == Some(&0x0c) {
                return (lines, y);
            }
            if self.height + y > b {
                break;
            }
        }
        ext.b += 1 - o.line_gap;
        (lines - 1, y - o.line_gap)
    }

    /// 0x4202b0: draw `s` at (x, y) in color code `color` (0x41f610) with `o`. Returns
    /// the text's extent grown by 2 within the clip (0x6ce0d8..0x6ce0de).
    pub fn draw_text(&self, s: &str, x: i32, y: i32, color: u8, o: &TextOpts, dst: TextDst) -> Ext {
        self.draw_bytes(s.as_bytes(), x, y, color, o, dst)
    }

    /// draw_text for game strings as stored (bytes 0x80.. are the font's own glyphs).
    pub fn draw_bytes(&self, s: &[u8], x: i32, mut y: i32, color: u8, o: &TextOpts, dst: TextDst) -> Ext {
        let mut o = *o;
        o.clip.0 = o.clip.0.max(0);
        o.clip.1 = o.clip.1.max(0);
        o.clip.2 = o.clip.2.min(dst.w - 1);
        o.clip.3 = o.clip.3.min(dst.h - 1);
        let (l, t, r, b) = o.clip;
        let mut ext = Ext { l: x, t: y, r: x, b: y };
        let grow = |e: Ext| Ext { l: (e.l - 2).max(l), t: (e.t - 2).max(t), r: (e.r + 2).min(r), b: (e.b + 2).min(b) };
        if !(x >= l && y >= t && x <= r && y + self.height <= b) {
            return grow(ext);
        }
        let mut c = Color { cur: 0, prev: 0 };
        c.set(color);
        let mut lines = 0;
        if o.flags & 0x68 != 0 {
            let (n, end_y) = self.layout(s, x, y, &mut Color { cur: c.cur, prev: c.prev }, &o, &mut None, &mut ext);
            lines = n;
            if o.flags & 0x10 == 0 {
                y = valign(y, end_y, b, o.flags);
            }
        }
        if o.flags & 8 != 0 {
            o.flags ^= if lines > 1 { 9 } else { 0xa };
        }
        self.layout(s, x, y, &mut c, &o, &mut Some(dst), &mut ext);
        grow(ext)
    }

    /// One left-aligned line at (x, y) in the color row `colors` (8 palette indices).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, s: &str, x: i32, y: i32, colors: &[u8], dst: &mut [u8], dw: i32, dh: i32) -> i32 {
        let o = TextOpts::new();
        let mut pen = x;
        let mut t = Some(TextDst { rows: colors, out: dst, w: dw, h: dh });
        let s = s.as_bytes();
        self.draw_run(s, 0, s.len(), x, y, &mut pen, &mut Color { cur: 0, prev: 0 }, &o, &mut t, &mut Ext::default());
        pen
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
    pub console: Pcx,
    pub font: Font,
    /// font\font8.fnt: text controls with flag 0x400.
    pub font8: Font,
    pub icons: Grp,
    /// The cursors in 1.16.1's order (0x512708, loaded by 0x4d1560).
    pub cursors: Vec<Grp>,
    pub wireframe: Grp,
    /// unit\wirefram\grpwire.grp: the small wireframes of a multiple selection.
    pub grpwire: Grp,
    /// unit\wirefram\tranwire.grp: wireframes of a transport's passengers.
    pub tranwire: Grp,
    /// game\twire.pcx (0x68c208): the 24 wireframe colors.
    pub twire: Vec<u8>,
    pub cmdicons: Grp,
    /// ticon.pcx: 6 states x 16 colors for command icons.
    pub ticon: Vec<u8>,
    pub stat_txt: Vec<String>,
    /// rez\stat_txt.tbl as read (button strings, `dat::tbl_button`).
    pub stat_raw: Vec<u8>,
    /// rez\tips.tbl (0x658adc).
    pub tips: Vec<String>,
    /// rez\network.tbl (0x6d1220, 0x4cdbd0).
    pub network: Vec<String>,
    /// rez\gluAll.tbl (0x4ddd30).
    pub glu_all: Vec<String>,
    /// rez\statbtnn.bin: the command card dialog while watching a replay.
    /// rez\statbtn<race>.bin, statbtnn.bin while watching a replay (0x459b90).
    pub statbtn: crate::dlg::Dialog,
    /// tfontgam rows of 8 palette indices per text color.
    pub text_colors: Vec<u8>,
    /// rez\statdata.bin: the status panel dialog (statdata.cpp).
    pub statdata: crate::dlg::Dialog,
    /// unit\cmdbtns\<race>cmdbtns.grp (0x68c1c0, 0x459b90): slot frames and numbers.
    pub cmdbtns: Grp,
    /// game\<race>pbrempt.pcx / pbrfull.pcx: progress bar (0x458050).
    pub pbar_empty: Pcx,
    pub pbar_full: Pcx,
    /// The dialog graphics, fonts and colors (dlgs\<race>.grp, 0x4ee2d0).
    pub skin: crate::dlg::Skin,
    /// rez\stat_f10.bin: the MENU button (0x4f5300).
    pub stat_f10: crate::dlg::Dialog,
    /// rez\statport.bin: the portrait (0x45f320) and arr\portdata.
    pub statport: crate::dlg::Dialog,
    /// game\[tzpn]conover.pcx: the frame drawn over the portrait (0x45f0b0, 0x45e650).
    pub conover: Option<Pcx>,
    pub portraits: Option<crate::portrait::Portraits>,
    /// rez\statres.bin: the resource bar (0x4e5a80).
    pub statres: crate::dlg::Dialog,
    /// 0x68c230 / 0x68c22c: the minerals and gas the bar shows (they count toward the
    /// real amounts, 0x4e52e0), and the player shown in a replay (0x5153f8).
    pub res_shown: std::cell::Cell<(i32, i32, u8)>,
    /// rez\minimap.bin: the minimap and its buttons (0x4a5a80).
    pub minimap: crate::dlg::Dialog,
}

/// 0x4dab00: fonts are encrypted with the key in `font\font.gid` (grid.cpp).
pub(crate) fn read_font(files: &Storage, name: &str) -> io::Result<Vec<u8>> {
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
        Ui::load_with(files, race, false)
    }

    /// `replay`: watching a replay, the console is game\nconsole.pcx (0x4c3950).
    pub fn load_with(files: &Storage, race: &str, replay: bool) -> io::Result<Ui> {
        // game\[tpz]console.pcx by the local race.
        let c = if replay { "n" } else { &race[..1] };
        let console = pcx8(&files.read(&format!("game/{c}console.pcx"))?);
        let conover = files.read(&format!("game/{c}conover.pcx")).ok().map(|d| pcx8(&d));
        let font = Font::parse(read_font(files, "font/font10.fnt")?);
        let font8 = Font::parse(read_font(files, "font/font8.fnt")?);
        let icons = Grp::parse(files.read("game/icons.grp")?);
        let cursors = CURSORS.iter().map(|n| files.read(&format!("cursor/{n}")).map(Grp::parse)).collect::<io::Result<Vec<_>>>()?;
        let wireframe = Grp::parse(files.read("unit/wirefram/wirefram.grp")?);
        let twire = crate::view::pcx(&files.read("game/twire.pcx")?).2;
        let grpwire = Grp::parse(files.read("unit/wirefram/grpwire.grp")?);
        let tranwire = Grp::parse(files.read("unit/wirefram/tranwire.grp")?);
        let cmdicons = Grp::parse(files.read("unit/cmdbtns/cmdicons.grp")?);
        let ticon = crate::view::pcx(&files.read("unit/cmdbtns/ticon.pcx")?).2;
        let stat_raw = files.read("rez/stat_txt.tbl")?;
        let stat_txt = crate::dat::parse_tbl(&stat_raw);
        let tips = files.read("rez/tips.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default();
        let network = files.read("rez/network.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default();
        let glu_all = files.read("rez/gluAll.tbl").map(|d| crate::dat::parse_tbl(&d)).unwrap_or_default();
        let statbtn = crate::dlg::Dialog::parse(&files.read(&format!("rez/statbtn{}.bin", if replay { 'n' } else { race.as_bytes()[0] as char }))?);
        let (_, _, text_colors) = crate::view::pcx(&files.read("game/tfontgam.pcx")?);
        let r = &race[..1];
        let statdata = crate::dlg::Dialog::parse(&files.read("rez/statdata.bin")?);
        let cmdbtns = Grp::parse(files.read(&format!("unit/cmdbtns/{r}cmdbtns.grp"))?);
        let pbar_empty = pcx8(&files.read(&format!("game/{r}pbrempt.pcx"))?);
        let pbar_full = pcx8(&files.read(&format!("game/{r}pbrfull.pcx"))?);
        let fonts = [
            Font::parse(read_font(files, "font/font8.fnt")?),
            Font::parse(read_font(files, "font/font10.fnt")?),
            Font::parse(read_font(files, "font/font16.fnt")?),
            Font::parse(read_font(files, "font/font16x.fnt")?),
        ];
        let skin = crate::dlg::Skin {
            grp: Grp::parse_raw(files.read(&format!("dlgs/{race}.grp"))?),
            tile: Grp::parse(files.read("dlgs/tile.grp")?),
            colors: std::cell::Cell::new([0; 26]),
            fonts,
            text_rows: text_colors.clone(),
            fx: None,
        };
        let mut stat_f10 = crate::dlg::Dialog::parse(&files.read("rez/stat_f10.bin")?);
        // 0x4f5180 (init): graphic 0x51276c, draw 0x4f4fb0.
        if let Some(c) = stat_f10.get_mut(1) {
            c.graphic = 0;
            c.update = crate::dlg::Update::MenuButton;
        }
        let statres = crate::dlg::Dialog::parse(&files.read("rez/statres.bin")?);
        let statport = crate::dlg::Dialog::parse(&files.read("rez/statport.bin")?);
        let portraits = crate::portrait::Portraits::load(files);
        let mut minimap = crate::dlg::Dialog::parse(&files.read("rez/minimap.bin")?);
        // 0x4a5230 (button init), single player (0x57f0b4 = 0): button 2 (graphic 3)
        // shows unless watching a replay; 4 (83, a replay's vision button 140) shows
        // only in a replay; 3 (130, diplomacy) needs two human players.
        for (id, gr, shown) in [(2, 3, !replay), (3, 130, false), (4, if replay { 140 } else { 83 }, replay)] {
            if let Some(c) = minimap.get_mut(id) {
                c.graphic = gr;
                if shown {
                    c.flags |= crate::dlg::flag::VISIBLE;
                } else {
                    c.flags &= !crate::dlg::flag::VISIBLE;
                }
            }
        }
        // The minimap itself (control 1, 0x4a3570) is drawn by the caller.
        if let Some(c) = minimap.get_mut(1) {
            c.flags &= !crate::dlg::flag::VISIBLE;
        }
        Ok(Ui { console, conover, font, font8, icons, cursors, wireframe, grpwire, tranwire, twire, cmdicons, ticon, stat_txt, stat_raw, tips, network, glu_all, statbtn, text_colors, statdata, cmdbtns, pbar_empty, pbar_full, skin, stat_f10, statport, portraits, statres, res_shown: std::cell::Cell::new((0, 0, 0xff)), minimap })
    }

    pub fn text_color(&self, c: usize) -> &[u8] {
        let o = (c * 8).min(self.text_colors.len().saturating_sub(8));
        &self.text_colors[o..o + 8]
    }

    pub fn draw_icon(&self, frame: usize, x: i32, y: i32, dst: &mut [u8], dw: i32, dh: i32) {
        self.icons.draw(frame, false, x, y, dst, dw, dh, Blend::Normal);
    }

    /// Command icon `frame` at (x, y) (grp box origin); `state` is the ticon.pcx row:
    /// 0 normal, 1 disabled, 2 highlighted.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_cmd_icon(&self, frame: usize, state: usize, x: i32, y: i32, dst: &mut [u8], dw: i32, dh: i32) {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = self.ticon.get(state * 16 + (i & 15)).copied().unwrap_or(i as u8);
        }
        self.cmdicons.draw(frame, false, x, y, dst, dw, dh, Blend::Colors(&t));
    }

    /// 0x4d1140: is screen point (x, y) over the console, so that a press there does not
    /// start a map selection: rows above the console's first non-empty row are the map,
    /// rows from its first fully opaque row are the console; between them the point is
    /// the console where the console bitmap has a pixel (the region test, 0x4d11a0 /
    /// 0x41d470).
    pub fn hud_hit(&self, x: i32, y: i32) -> bool {
        let c = &self.console;
        let y0 = crate::view::SCREEN_H - c.h as i32;
        let (sx, sy) = (x, y - y0);
        if sy < 0 || sy >= c.h as i32 || sx < 0 || sx >= c.w as i32 {
            return sy >= c.h as i32;
        }
        c.px[sy as usize * c.w + sx as usize] != 0
    }

    /// Cursor `kind` (index into CURSORS) at (x, y): frame `counter` (0x597390, one step
    /// per 100 ms), hot spot at (63, 63) of the 128x128 GRP (0x4be120).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_cursor(&self, kind: usize, counter: usize, x: i32, y: i32, dst: &mut [u8], dw: i32, dh: i32) {
        let Some(g) = self.cursors.get(kind) else { return };
        let f = counter % g.frame_count().max(1);
        g.draw(f, false, x - 0x3f, y - 0x3f, dst, dw, dh, Blend::Normal);
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
