//! Scenario (.chk) parsing and terrain rendering.

use crate::storage::Storage;
use std::io;

pub const TILESETS: [&str; 8] = ["badlands", "platform", "install", "ashworld", "jungle", "desert", "ice", "twilight"];

pub struct Chk {
    /// Every section in file order: 0x413550 runs a tag's handler once per instance.
    sections: Vec<([u8; 4], Vec<u8>)>,
    /// The file as loaded (0x5993d4, 0x5994d8 bytes): a saved game keeps it.
    pub raw: Vec<u8>,
}

impl Chk {
    pub fn parse(d: &[u8]) -> Chk {
        let mut sections = Vec::new();
        let mut i = 0;
        while i + 8 <= d.len() {
            let tag: [u8; 4] = d[i..i + 4].try_into().unwrap();
            let len = i32::from_le_bytes(d[i + 4..i + 8].try_into().unwrap());
            i += 8;
            // Negative lengths are a known map-protection trick: BW just seeks backward.
            if len < 0 {
                i = i.saturating_sub(len.unsigned_abs() as usize);
                continue;
            }
            let end = (i + len as usize).min(d.len());
            sections.push((tag, d[i..end].to_vec()));
            i = end;
        }
        Chk { sections, raw: d.to_vec() }
    }

    /// The last instance of `tag`: what the handlers that copy a section into fixed
    /// globals end up with.
    pub fn section(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.sections(tag).last()
    }

    /// Every instance of `tag` in file order (UNIT / THG2 handlers act on each).
    pub fn sections<'a>(&'a self, tag: &[u8; 4]) -> impl DoubleEndedIterator<Item = &'a [u8]> + use<'a> {
        let tag = *tag;
        self.sections.iter().filter(move |(t, _)| *t == tag).map(|(_, d)| d.as_slice())
    }

    /// MTXM as the handler 0x4cd0b0 leaves it: each instance is copied over the start
    /// of the tile buffer (0x5993c4), so a short later one patches the first tiles.
    pub fn mtxm(&self) -> Vec<u8> {
        let mut m = Vec::new();
        for s in self.sections(b"MTXM").filter(|s| s.len() <= 0x20000) {
            if m.len() < s.len() {
                m.resize(s.len(), 0);
            }
            m[..s.len()].copy_from_slice(s);
        }
        m
    }

    fn u16s(&self, tag: &[u8; 4]) -> Vec<u16> {
        self.section(tag).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    }

    pub fn dims(&self) -> (usize, usize) {
        let d = self.u16s(b"DIM ");
        (d.first().copied().unwrap_or(64) as usize, d.get(1).copied().unwrap_or(64) as usize)
    }

    pub fn tileset(&self) -> usize {
        self.u16s(b"ERA ").first().copied().unwrap_or(0) as usize & 7
    }

    /// OWNR: per-slot controller (0 inactive, 3 rescuable, 5 computer, 6 human, 7 neutral).
    pub fn owners(&self) -> [u8; 12] {
        let mut o = [0u8; 12];
        if let Some(s) = self.section(b"OWNR") {
            o[..s.len().min(12)].copy_from_slice(&s[..s.len().min(12)]);
        }
        o
    }

    /// SIDE: per-slot race (0 zerg, 1 terran, 2 protoss, 5 user select, 7 inactive).
    pub fn races(&self) -> [u8; 12] {
        let mut o = [0u8; 12];
        if let Some(s) = self.section(b"SIDE") {
            o[..s.len().min(12)].copy_from_slice(&s[..s.len().min(12)]);
        }
        o
    }

    /// STR string table (1-based ids, as referenced by other sections).
    pub fn string(&self, id: u16) -> Option<String> {
        Some(String::from_utf8_lossy(self.string_bytes(id)?).into_owned())
    }

    /// The STR string as stored (bytes 0x80.. are the fonts' own glyphs).
    pub fn string_bytes(&self, id: u16) -> Option<&[u8]> {
        let s = self.section(b"STR ")?;
        // A STR chunk shorter than its count (1 byte in some protected maps; 0x4cc650
        // copies any size) has no strings, as `setup::tbl_str` reads it.
        let n = u16::from_le_bytes([*s.first()?, *s.get(1)?]) as usize;
        let i = (id as usize).checked_sub(1).filter(|&i| i < n)?;
        let o = u16::from_le_bytes([*s.get(2 + i * 2)?, *s.get(3 + i * 2)?]) as usize;
        let t = s.get(o..)?;
        Some(&t[..t.iter().position(|&b| b == 0).unwrap_or(t.len())])
    }

    /// Megatile ids, row-major, padded to width*height.
    pub fn tiles(&self) -> Vec<u16> {
        let (w, h) = self.dims();
        let mut t: Vec<u16> = self.mtxm().chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        t.resize(w * h, 0);
        t
    }
}

pub struct Tileset {
    pub palette: [[u8; 3]; 256],
    /// 0x6ce2a0 / 0x6ce320: the palette color cycling (water, lava) has rotated so far.
    pub cycle: crate::palette::Cycle,
    cv5: Vec<u8>,
    vx4: Vec<u32>,
    vr4: Vec<u8>,
    /// Per megatile, 16 minitile flag words (0x1 walkable, 0x2 mid, 0x4 high, 0x8 blocks view).
    vf4: Vec<u16>,
}

impl Tileset {
    pub fn load(files: &Storage, id: usize) -> io::Result<Tileset> {
        let name = TILESETS[id];
        let rd = |ext: &str| files.read(&format!("tileset/{name}.{ext}"));
        let wpe = rd("wpe")?;
        let mut palette = [[0u8; 3]; 256];
        for (i, p) in palette.iter_mut().enumerate() {
            p.copy_from_slice(&wpe[i * 4..i * 4 + 3]);
        }
        let vx4 = rd("vx4")?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();
        let vf4 = rd("vf4")?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(Tileset { palette, cycle: crate::palette::Cycle::new(id as u16, palette), cv5: rd("cv5")?, vx4, vr4: rd("vr4")?, vf4 })
    }

    pub fn megatile(&self, tile: u16) -> usize {
        let (group, idx) = (tile as usize >> 4, tile as usize & 15);
        let o = group * 52 + 20 + idx * 2;
        self.cv5.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize)
    }

    /// cv5 group flags for a map tile: low byte 0x80 = unbuildable; high byte = ground height.
    pub fn tile_flags(&self, tile: u16) -> u16 {
        let o = (tile as usize >> 4) * 52 + 2;
        self.cv5.get(o..o + 2).map_or(0x80, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn minitile_flags(&self, mega: usize, mini: usize) -> u16 {
        self.vf4.get(mega * 16 + mini).copied().unwrap_or(0)
    }

    /// Blit a 32x32 megatile as palette indices into `dst` (row stride `pitch`).
    pub fn draw_megatile(&self, mega: usize, dst: &mut [u8], pitch: usize) {
        for m in 0..16 {
            let v = self.vx4.get(mega * 16 + m).copied().unwrap_or(0);
            let (img, flip) = ((v >> 1) as usize, v & 1 != 0);
            let src = self.vr4.get(img * 64..img * 64 + 64).unwrap_or(&[0; 64]);
            let (mx, my) = ((m % 4) * 8, (m / 4) * 8);
            for y in 0..8 {
                for x in 0..8 {
                    let sx = if flip { 7 - x } else { x };
                    dst[(my + y) * pitch + mx + x] = src[y * 8 + sx];
                }
            }
        }
    }
}

/// Render whole-map terrain to an indexed image (w*32 x h*32).
pub fn render_terrain(chk: &Chk, ts: &Tileset) -> (usize, usize, Vec<u8>) {
    let (w, h) = chk.dims();
    let pitch = w * 32;
    let mut img = vec![0u8; pitch * h * 32];
    for (i, &t) in chk.tiles().iter().enumerate() {
        let (tx, ty) = (i % w, i / w);
        let off = ty * 32 * pitch + tx * 32;
        ts.draw_megatile(ts.megatile(t), &mut img[off..], pitch);
    }
    (pitch, h * 32, img)
}

/// Walkability grid in 8x8 minitiles (BW's pathing resolution).
pub fn walk_grid(chk: &Chk, ts: &Tileset) -> (usize, usize, Vec<bool>) {
    let (w, h) = chk.dims();
    let tiles = chk.tiles();
    let mut g = vec![false; w * 4 * h * 4];
    for (i, &t) in tiles.iter().enumerate() {
        let mega = ts.megatile(t);
        for m in 0..16 {
            let (x, y) = ((i % w) * 4 + m % 4, (i / w) * 4 + m / 4);
            g[y * w * 4 + x] = ts.minitile_flags(mega, m) & 1 != 0;
        }
    }
    (w * 4, h * 4, g)
}

/// Buildable 32x32 tiles: cv5 says buildable and every minitile is walkable.
pub fn build_grid(chk: &Chk, ts: &Tileset) -> Vec<bool> {
    chk.tiles()
        .iter()
        .map(|&t| ts.tile_flags(t) & 0x80 == 0 && (0..16).all(|m| ts.minitile_flags(ts.megatile(t), m) & 1 != 0))
        .collect()
}
