//! Scenario (.chk) parsing and terrain rendering.

use crate::casc::Casc;
use std::collections::HashMap;
use std::io;

pub const TILESETS: [&str; 8] = ["badlands", "platform", "install", "ashworld", "jungle", "desert", "ice", "twilight"];

pub struct Chk {
    sections: HashMap<[u8; 4], Vec<u8>>,
}

impl Chk {
    pub fn parse(d: &[u8]) -> Chk {
        let mut sections = HashMap::new();
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
            sections.insert(tag, d[i..end].to_vec()); // ponytail: last-wins; BW overlays MTXM partially
            i = end;
        }
        Chk { sections }
    }

    pub fn section(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.sections.get(tag).map(Vec::as_slice)
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

    /// Megatile ids, row-major, padded to width*height.
    pub fn tiles(&self) -> Vec<u16> {
        let (w, h) = self.dims();
        let mut t = self.u16s(b"MTXM");
        t.resize(w * h, 0);
        t
    }
}

pub struct Tileset {
    pub palette: [[u8; 3]; 256],
    cv5: Vec<u8>,
    vx4: Vec<u32>,
    vr4: Vec<u8>,
}

impl Tileset {
    pub fn load(casc: &Casc, id: usize) -> io::Result<Tileset> {
        let name = TILESETS[id];
        let rd = |ext: &str| casc.read(&format!("tileset/{name}.{ext}"));
        let wpe = rd("wpe")?;
        let mut palette = [[0u8; 3]; 256];
        for (i, p) in palette.iter_mut().enumerate() {
            p.copy_from_slice(&wpe[i * 4..i * 4 + 3]);
        }
        let vx4 = rd("vx4ex")?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        Ok(Tileset { palette, cv5: rd("cv5")?, vx4, vr4: rd("vr4")? })
    }

    pub fn megatile(&self, tile: u16) -> usize {
        let (group, idx) = (tile as usize >> 4, tile as usize & 15);
        let o = group * 52 + 20 + idx * 2;
        self.cv5.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize)
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
