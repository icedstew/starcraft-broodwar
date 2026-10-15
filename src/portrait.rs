//! The unit portrait in the console (statport.bin, 0x45e7a0 idle / 0x45e8a0 talking):
//! a Smacker video from portrait\ named by arr\portdata.dat (idle entries 0..110,
//! talking 110..220) for the status unit's units.dat portrait, played by Storm's SVid.

use crate::smk::Smk;
use crate::storage::Storage;

pub struct Portraits {
    /// arr\portdata.dat file ids (portdata.tbl strings, 1-based) and the names.
    files: Vec<u32>,
    names: Vec<String>,
}

impl Portraits {
    pub fn load(files: &Storage) -> Option<Portraits> {
        let d = files.read("arr/portdata.dat").ok()?;
        let ids = (0..220).map(|i| d.get(i * 4..i * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))).collect();
        let names = crate::dat::parse_tbl(&files.read("arr/portdata.tbl").ok()?);
        Some(Portraits { files: ids, names })
    }

    /// "portrait\<name><variant>.smk" for portrait `p`, idle or talking.
    fn path(&self, p: u16, talking: bool, variant: u32) -> Option<String> {
        let id = *self.files.get(p as usize + if talking { 110 } else { 0 })?;
        let name = self.names.get((id as usize).checked_sub(1)?)?.split('\0').next()?.to_string();
        Some(format!("portrait\\{name}{variant}.smk"))
    }
}

/// The video playing: which portrait, idle or talking, the decoder and the time into
/// the current frame.
pub struct Player {
    pub portrait: Option<u16>,
    talking: bool,
    variant: u32,
    video: Option<Smk>,
    elapsed: u32,
    /// The idle / talking variant picks (0x45e450's generator stands in as an LCG).
    seed: u32,
    /// The palette the translation was made for, and the translation.
    remap: Option<([[u8; 3]; 256], [u8; 256])>,
}

impl Default for Player {
    fn default() -> Self {
        Player { portrait: None, talking: false, variant: 0, video: None, elapsed: 0, seed: 1, remap: None }
    }
}

impl Player {
    fn rand(&mut self, n: u32) -> u32 {
        self.seed = self.seed.wrapping_mul(214013).wrapping_add(2531011);
        (self.seed >> 16 & 0x7fff) % n.max(1)
    }

    fn open(&mut self, ps: &Portraits, files: &Storage) {
        self.video = None;
        self.elapsed = 0;
        let Some(p) = self.portrait else { return };
        // ponytail: 0x45e7a0 weights the idle variants by tables at 0x68ac50 and
        // 0x45e8a0 avoids repeating the last talking one; both pick uniformly among the
        // files that exist, falling back to variant 0.
        let mut tries: Vec<u32> = (0..4).filter(|&v| files.read(&ps.path(p, self.talking, v).unwrap_or_default()).is_ok()).collect();
        if tries.is_empty() {
            return;
        }
        if self.talking && tries.len() > 1 {
            tries.retain(|&v| v != self.variant);
        }
        let v = tries[self.rand(tries.len() as u32) as usize];
        self.variant = v;
        let Some(path) = ps.path(p, self.talking, v) else { return };
        self.video = files.read(&path).ok().and_then(Smk::parse);
        if let Some(s) = self.video.as_mut() {
            s.next_frame();
        }
    }

    /// The status unit's portrait (None for no selection or a unit without one); a new
    /// one starts idling.
    pub fn show(&mut self, ps: &Portraits, files: &Storage, portrait: Option<u16>) {
        if portrait != self.portrait {
            self.portrait = portrait;
            self.talking = false;
            self.open(ps, files);
        }
    }

    /// The unit speaks: its talking video plays once.
    pub fn talk(&mut self, ps: &Portraits, files: &Storage) {
        if self.portrait.is_some() {
            self.talking = true;
            self.open(ps, files);
        }
    }

    /// Advance `ms`; `animate` false (still portraits) keeps the first frame. A video
    /// that ends goes back to idling with a new pick.
    pub fn tick(&mut self, ps: &Portraits, files: &Storage, ms: u32, animate: bool) {
        if !animate {
            return;
        }
        let Some(v) = self.video.as_mut() else { return };
        self.elapsed += ms;
        while self.elapsed >= v.ms.max(1) {
            self.elapsed -= v.ms.max(1);
            if !v.next_frame() {
                self.talking = false;
                self.open(ps, files);
                return;
            }
        }
    }

    /// The current frame at (x, y) clipped to w x h, each video color as the nearest
    /// game palette color (SVid translating to the 8-bit screen).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, pal: &[[u8; 3]; 256], x: i32, y: i32, w: i32, h: i32, out: &mut [u8], sw: i32, sh: i32) {
        let Some(v) = self.video.as_ref() else { return };
        let fresh = matches!(&self.remap, Some((p, _)) if *p == v.palette);
        if !fresh {
            let mut t = [0u8; 256];
            for (i, c) in v.palette.iter().enumerate() {
                let near = (1..256).min_by_key(|&j| {
                    let p = pal[j];
                    (0..3).map(|k| (p[k] as i32 - c[k] as i32).pow(2)).sum::<i32>()
                });
                t[i] = near.unwrap_or(0) as u8;
            }
            self.remap = Some((v.palette, t));
        }
        let Some((_, t)) = &self.remap else { return };
        for yy in 0..h.min(v.h as i32) {
            for xx in 0..w.min(v.w as i32) {
                let (px, py) = (x + xx, y + yy);
                if (0..sw).contains(&px) && (0..sh).contains(&py) {
                    out[(py * sw + px) as usize] = t[v.pixels[yy as usize * v.w + xx as usize] as usize];
                }
            }
        }
    }
}
