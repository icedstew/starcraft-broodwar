//! Map terrain state: MTXM, megatile ids, per-tile flags (Gamemap.cpp) and the
//! minitile walkability map used by region creation (sai_ContoursCreate.cpp).

/// Tileset files the game reads (Tileset\<name>.vf4 / .cv5).
pub struct TilesetData {
    /// .vf4: 16 minitile flag words per megatile (0x5993d0).
    pub vf4: Vec<u16>,
    /// .cv5: 0x34-byte tile groups (0x6d5ec8).
    pub cv5: Vec<u8>,
}

impl TilesetData {
    /// 0x5998e0: megatile count = vf4 file size >> 5.
    pub fn megatile_count(&self) -> usize {
        self.vf4.len() / 16
    }

    fn cv5_u16(&self, off: usize) -> u16 {
        self.cv5.get(off..off + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// cv5 megatile index for a raw MTXM value.
    pub fn megatile(&self, raw: u16) -> u16 {
        let group = (raw as usize >> 4) & 0x7ff;
        self.cv5_u16(0x14 + ((raw as usize & 0xf) + group * 0x1a) * 2)
    }

    /// cv5 group flags (word at +2).
    pub fn group_flags(&self, raw: u16) -> u16 {
        self.cv5_u16(((raw as usize >> 4) & 0x7ff) * 0x34 + 2)
    }

    fn vf4(&self, mega: usize, mini: usize) -> u16 {
        self.vf4.get(mega * 16 + mini).copied().unwrap_or(0)
    }
}

pub struct GameMap {
    /// 0x57f1d4 / 0x57f1d6: size in tiles.
    pub width: u16,
    pub height: u16,
    /// 0x5993c4: raw MTXM.
    pub mtxm: Vec<u16>,
    /// 0x628494: per-tile megatile id (0x8000 = MTXM bit 0x8000).
    pub megatiles: Vec<u16>,
    /// 0x6d1260: per-tile flags, stride `width`.
    pub tile_flags: Vec<u32>,
    /// 0x5993cc: per-megatile flags derived from vf4 (0x4bcf50).
    pub megatile_flags: Vec<u32>,
    /// 0x5993d0: the tileset's vf4 (minitile flags per megatile).
    pub vf4: Vec<u16>,
    /// 0x6ca54c: 1024x1024 minitile map, 0x80 = unwalkable, low 4 bits = edge sides.
    pub minitiles: Vec<u8>,
}

impl GameMap {
    pub fn new(width: u16, height: u16, ts: &TilesetData) -> GameMap {
        GameMap {
            width,
            height,
            mtxm: vec![0; 0x10000],
            megatiles: vec![0; 0x10000],
            tile_flags: vec![0; 0x10000],
            megatile_flags: megatile_flags(ts),
            vf4: ts.vf4.clone(),
            minitiles: Vec::new(),
        }
    }

    /// MTXM section handler (0x4cd0b0) up to region creation: copy, derive tile flags,
    /// make the bottom edge and its corners unbuildable/unwalkable.
    pub fn load_mtxm(&mut self, ts: &TilesetData, section: &[u8]) {
        for (i, c) in section.chunks(2).take(0x10000).enumerate() {
            // Odd trailing byte overwrites only the low byte (rep movsb).
            self.mtxm[i] = if c.len() == 2 { u16::from_le_bytes([c[0], c[1]]) } else { (self.mtxm[i] & 0xff00) | c[0] as u16 };
        }
        self.update_tile_flags(ts);
        let (w, h) = (self.width as usize, self.height as usize);
        for &x0 in &[0, w - 5] {
            for y in h - 2..h {
                for x in x0..x0 + 5 {
                    self.tile_flags[y * w + x] &= 0xdfbe_ffff;
                }
            }
            for y in h - 2..h {
                for x in x0..x0 + 5 {
                    self.tile_flags[y * w + x] |= 0x80_0000;
                }
            }
        }
        self.clear_tile_flags(0, h - 1, w, 1, 0x2041_0000);
        self.set_tile_flags(0, h - 1, w, 1, 0x80_0000);
    }

    /// 0x4bcea0: megatile ids and tile flags from MTXM + cv5.
    fn update_tile_flags(&mut self, ts: &TilesetData) {
        for i in 0..self.width as usize * self.height as usize {
            let raw = self.mtxm[i];
            let mega = ts.megatile(raw);
            self.megatiles[i] = mega;
            let mut f = self.tile_flags[i] & 0xffff;
            f |= ((ts.group_flags(raw) as u32 & 0xffff_d8fa) << 16)
                | self.megatile_flags.get(mega as usize & 0x7fff).copied().unwrap_or(0);
            if raw & 0x8000 != 0 {
                self.megatiles[i] |= 0x8000;
                f |= 0x40_0000;
            }
            self.tile_flags[i] = f;
        }
    }

    /// 0x472e50: clear `mask` in a rect of tiles.
    pub fn clear_tile_flags(&mut self, x: usize, y: usize, w: usize, h: usize, mask: u32) {
        let stride = self.width as usize;
        for row in y..y + h {
            for f in &mut self.tile_flags[row * stride + x..row * stride + x + w] {
                *f &= !mask;
            }
        }
    }

    /// 0x472eb0: set `mask` in a rect of tiles.
    pub fn set_tile_flags(&mut self, x: usize, y: usize, w: usize, h: usize, mask: u32) {
        let stride = self.width as usize;
        for row in y..y + h {
            for f in &mut self.tile_flags[row * stride + x..row * stride + x + w] {
                *f |= mask;
            }
        }
    }

    /// 0x42bc30: build the minitile map (0x80 = unwalkable) with edge bits
    /// 1 up / 2 right / 4 down / 8 left set where a walkable minitile borders unwalkable.
    pub fn build_minitiles(&mut self, ts: &TilesetData) {
        let rows = minitile_rows(ts);
        let m = &mut self.minitiles;
        *m = vec![0; 0x100000];
        let (w, h) = (self.width as usize, self.height as usize);
        for ty in 0..h {
            for tx in 0..w {
                let mega = (self.megatiles[ty * w + tx] & 0x7fff) as usize;
                for r in 0..4 {
                    let o = (ty * 4 + r) * 0x400 + tx * 4;
                    m[o..o + 4].copy_from_slice(&rows.get(mega * 4 + r).copied().unwrap_or([0; 4]));
                }
            }
        }
        let (mw, mh) = (w as isize * 32 >> 3, h as isize * 32 >> 3);
        let at = |x: isize, y: isize| (y * 0x400 + x) as usize;
        for y in mh - 8..mh {
            for x in 0..20 {
                m[at(x, y)] = 0x80;
            }
            for x in mw - 20..mw {
                m[at(x, y)] = 0x80;
            }
            if y >= mh - 4 {
                for x in 0..mw {
                    m[at(x, y)] = 0x80;
                }
            }
        }
        let blocked = |m: &[u8], i: isize| (m[i as usize] as i8) < 0;
        // Top and bottom rows.
        let (top, bot) = (0isize, (mh - 1) * 0x400);
        for i in 0..mw {
            let n = mw - i;
            let (p, q) = (top + i, bot + i);
            let b = m[p as usize];
            if (b as i8) >= 0 {
                m[p as usize] = b | 1;
                if blocked(m, p + 0x400) {
                    m[p as usize] = b | 5;
                }
                if n == mw || blocked(m, p - 1) {
                    m[p as usize] |= 8;
                }
                if n == 1 || blocked(m, p + 1) {
                    m[p as usize] |= 2;
                }
            }
            let b = m[q as usize];
            if (b as i8) >= 0 {
                m[q as usize] = b | 4;
                if blocked(m, q - 0x400) {
                    m[q as usize] = b | 5;
                }
                if n == mw || blocked(m, q - 1) {
                    m[q as usize] |= 8;
                }
                if n == 1 || blocked(m, q + 1) {
                    m[q as usize] |= 2;
                }
            }
        }
        // Left and right columns.
        let (left, right) = (0isize, mw - 1);
        for j in 0..mh {
            let n = mh - j;
            let (p, q) = (left + j * 0x400, right + j * 0x400);
            let b = m[p as usize];
            if (b as i8) >= 0 {
                m[p as usize] = b | 8;
                if blocked(m, p + 1) {
                    m[p as usize] = b | 10;
                }
                if n == mh || blocked(m, p - 0x400) {
                    m[p as usize] |= 1;
                }
                if n == 1 || blocked(m, p + 0x400) {
                    m[p as usize] |= 4;
                }
            }
            let b = m[q as usize];
            if (b as i8) >= 0 {
                m[q as usize] = b | 2;
                if blocked(m, q - 1) {
                    m[q as usize] = b | 10;
                }
                if n == mh || blocked(m, q - 0x400) {
                    m[q as usize] |= 1;
                }
                if n == 1 || blocked(m, q + 0x400) {
                    m[q as usize] |= 4;
                }
            }
        }
        for y in 1..mh - 1 {
            for x in 1..mw - 1 {
                let p = at(x, y) as isize;
                let mut b = m[p as usize];
                if (b as i8) >= 0 {
                    if blocked(m, p - 0x400) {
                        b |= 1;
                    }
                    if blocked(m, p + 0x400) {
                        b |= 4;
                    }
                    if blocked(m, p - 1) {
                        b |= 8;
                    }
                    if blocked(m, p + 1) {
                        b |= 2;
                    }
                    m[p as usize] = b;
                }
            }
        }
    }
}

/// 0x4bcf50: per-megatile flags from its 16 minitiles.
fn megatile_flags(ts: &TilesetData) -> Vec<u32> {
    (0..ts.megatile_count())
        .map(|mega| {
            let mini = |i| ts.vf4(mega, i);
            let walk = (0..16).filter(|&i| mini(i) & 1 != 0).count();
            let mut f = 0u32;
            if walk > 0xc {
                f = 0x10000;
            }
            if walk != 0 && walk < 0x10 {
                f |= 0x2000_0000;
            }
            let high = (0..16).filter(|&i| mini(i) & 4 != 0).count();
            let mid = (0..16).filter(|&i| mini(i) & 2 != 0).count();
            let view = (0..16).filter(|&i| mini(i) & 8 != 0).count();
            if high < 0xc {
                if high + mid > 0xb {
                    f |= 0x200_0000;
                }
            } else {
                f |= 0x400_0000;
            }
            if view != 0 {
                f |= 0x100_0000;
            }
            if f & 0x10000 == 0 {
                f |= 0x4_0000;
            }
            f
        })
        .collect()
}

/// 0x42b620: per megatile, 4 rows of 4 minitile bytes (0x80 = not walkable).
fn minitile_rows(ts: &TilesetData) -> Vec<[u8; 4]> {
    (0..ts.megatile_count() * 4)
        .map(|r| core::array::from_fn(|i| if ts.vf4(r / 4, (r % 4) * 4 + i) & 1 == 0 { 0x80 } else { 0 }))
        .collect()
}

impl super::State {
    /// Map terrain and pathing regions (MTXM handler 0x4cd0b0 through 0x484290).
    pub fn load_terrain(&mut self, ts: &TilesetData, width: u16, height: u16, mtxm: &[u8]) -> bool {
        let mut m = GameMap::new(width, height, ts);
        m.load_mtxm(ts, mtxm);
        let sai = super::sai::create(&mut m, ts);
        self.map_px = (width * 32, height * 32);
        self.map_tiles = (width as usize, height as usize);
        self.terrain = m;
        match sai {
            Some(s) => {
                self.sai = s;
                true
            }
            None => false,
        }
    }
}
