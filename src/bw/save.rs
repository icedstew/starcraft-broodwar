//! saveload.cpp: a saved game's game state. 0x4d02d0 writes the file (text, header 0x4cf160,
//! 0x57eee0, 0x57f0f0, then the state below); 0x4cfef0 reads the state back from game init
//! (0x4ef100 -> 0x4eed10) after 0x4cf5f0 has read the part before it.
//!
//! The parts are written in 1.16.1's layout (pointers as 1-based indices, as the game's own
//! savers do), so that 1.16.1 reads our saves and we read its. Each subsystem's writer and
//! reader pair lives in its own file under `save/`.

use super::{Data, State};

#[path = "save/images.rs"]
mod images;
#[path = "save/sprites.rs"]
mod sprites;
#[path = "save/thingies.rs"]
mod thingies;
#[path = "save/flingies.rs"]
mod flingies;
#[path = "save/units.rs"]
mod units;
#[path = "save/bullets.rs"]
mod bullets;
#[path = "save/orders.rs"]
mod orders;
#[path = "save/tiles.rs"]
mod tiles;
#[path = "save/triggers.rs"]
mod triggers;
#[path = "save/ai.rs"]
mod ai;
#[path = "save/gamedata.rs"]
mod gamedata;
#[path = "save/misc.rs"]
mod misc;

/// The standard CRC-32 table-driven loop of 0x410430 (seed 0xffffffff, no final xor).
pub fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in d {
        let mut t = (c ^ b as u32) & 0xff;
        for _ in 0..8 {
            t = if t & 1 != 0 { 0xedb8_8320 ^ (t >> 1) } else { t >> 1 };
        }
        c = t ^ (c >> 8);
    }
    c
}

/// A file being written (the FILE * of 0x4d02d0).
#[derive(Default)]
pub struct Out {
    pub buf: Vec<u8>,
}

impl Out {
    /// fwrite of a value.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// 0x4c3450: `data` as its crc (0x410430), the chunk count, then each 0x2000-byte
    /// chunk's stored size and bytes: imploded (0x4c3200) or, when that does not shrink
    /// it, as is. False for empty data (the game writes nothing more then either).
    pub fn packed(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        self.u32(crc32(data));
        self.u32(data.len().div_ceil(0x2000) as u32);
        for c in data.chunks(0x2000) {
            // 0x4c3200: binary mode, 1 KB dictionary; stored as is unless smaller.
            let z = super::implode::implode(c);
            let z = if z.len() < c.len() { &z[..] } else { c };
            self.u32(z.len() as u32);
            self.bytes(z);
        }
        true
    }
}

/// A file being read (the FILE * 0x6d1218).
pub struct In<'a> {
    pub f: &'a [u8],
    pub pos: usize,
    /// The save format version of the header (0x596bb8; 3 written); the unit reader 0x49e910
    /// takes the old record layout below 3. The caller sets it from the header.
    pub format: u16,
}

impl<'a> In<'a> {
    pub fn new(f: &'a [u8]) -> In<'a> {
        In { f, pos: 0, format: 3 }
    }
    /// fread of `n` bytes; None at the end of the file.
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let b = self.f.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(b)
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.bytes(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// 0x4c3280: read `len` bytes written by 0x4c3450: crc, chunk count, then per chunk
    /// (<= 0x2000 plain bytes) its stored size and bytes (PKWare imploded unless the stored
    /// size equals the plain size). None on a short file, a bad size or a crc mismatch.
    pub fn packed(&mut self, len: usize) -> Option<Vec<u8>> {
        if len == 0 {
            return None;
        }
        let (crc, n) = (self.u32()?, self.u32()?);
        let mut out = Vec::with_capacity(len);
        for i in 0..n as usize {
            let sz = self.u32()? as usize;
            if sz > len - out.len() {
                return None;
            }
            let stored = self.bytes(sz)?;
            let plain = len.wrapping_sub(0x2000 * i).min(0x2000);
            if sz == plain {
                out.extend_from_slice(stored);
            } else {
                let d = explode::explode(stored).ok()?;
                if d.is_empty() || d.len() > len - out.len() {
                    return None;
                }
                out.extend_from_slice(&d);
            }
        }
        (out.len() == len && crc32(&out) == crc).then_some(out)
    }
}

impl State {
    /// 0x4d02d0 from 0x4935f0 on: the game state after the header, the player table and
    /// the game data (`save_head`). False when a part failed (the game deletes the file).
    pub fn save_state(&mut self, d: &Data, o: &mut Out) -> bool {
        let mut ok = true;
        // 0x4935f0 / 0x4989a0: the sprites' and images' pointers become indices
        // (undone after the units below by 0x499a60 / 0x494030).
        self.save_prepare(d);
        ok &= self.save_images(d, o); // 0x4d64c0
        ok &= self.save_sprites(d, o); // 0x498740
        ok &= self.save_fog_thingies(d, o); // 0x487540, 0x487ec0 x2, 0x4875f0
        ok &= self.save_flingies(d, o); // 0x495b50
        ok &= self.save_units(d, o); // 0x4eaaf0
        ok &= self.save_bullets(d, o); // 0x48aeb0
        ok &= self.save_orders(d, o); // 0x48c910
        self.save_restore(d); // 0x499a60, 0x494030
        ok &= self.save_tiles(d, o); // 0x6d0c68 / 0x6d0e80 blocks, 0x47da80, 0x5993c4, 0x628494, 0x6d1260
        ok &= self.save_trig_state(d, o); // 0x4899d0
        ok &= self.save_chk(d, o); // 0x5994d8 / 0x5993d4, 0x4cee00, 0x6284e8, 0x4ceda0
        ok &= self.save_ai(d, o); // 0x46ec40, 0x4a10f0
        ok &= self.save_tail(d, o); // 0x4bf390, 0x62848c, 0x6284a8
        ok
    }

    /// 0x4cfef0: the state `save_state` wrote, then the fix-ups after it (0x4ce440,
    /// 0x4ead50, 0x4dbc00, 0x49c440). False when the file is short or bad (the game then
    /// shows gluAll error 0x62 and goes back to the menu).
    pub fn load_state(&mut self, d: &Data, i: &mut In) -> bool {
        let dbg = |n: &str, ok: bool| {
            if !ok && std::env::var_os("SCBW_SAVE_DEBUG").is_some() {
                eprintln!("load: {n} failed");
            }
            ok
        };
        dbg("images", self.load_images(d, i)) // 0x4d6220
            && dbg("sprites", self.load_sprites(d, i)) // 0x498570
            && dbg("fog", self.load_fog_thingies(d, i)) // 0x488100
            && dbg("flingies", self.load_flingies(d, i)) // 0x495a50
            && dbg("units", self.load_units(d, i)) // 0x49e910
            && dbg("bullets", self.load_bullets(d, i)) // 0x48ae40
            && dbg("orders", self.load_orders(d, i)) // 0x48c870
            && dbg("tiles", self.load_tiles(d, i)) // 0x413900 / 0x4138c0 blocks, 0x47d9d0, the three maps
            && dbg("triggers", self.load_trig_state(d, i)) // 0x4897e0
            && dbg("chk", self.load_chk(d, i)) // 0x5994d8 / 0x5993d4, 0x6284e8, 0x4ceda0
            && dbg("ai", self.load_ai(d, i)) // 0x46ea70, 0x4a1180
            && dbg("tail", self.load_tail(d, i)) // 0x4bf020, 0x62848c, 0x6284a8, 0x4ce440, 0x4ead50, 0x4dbc00, 0x49c440
    }
}
