//! The unit portrait in the console (statport.bin, 0x45e7a0 idle / 0x45e8a0 talking):
//! a Smacker video from portrait\ named by arr\portdata.dat (idle entries 0..110,
//! talking 110..220) for the status unit's units.dat portrait, played by Storm's SVid.

use crate::smk::Smk;
use crate::todo_port;
use crate::storage::Storage;

/// 0x45e320: the portrait id of unit type `t` from units.dat's portrait table (0x662f88),
/// with the campaign mission 0x57f244 (`mission`) swapping portrait 0x26 for the entry
/// 0x6630b8 (table[152]) above mission 0x1f and 0x3c for 0x663024 (table[78]) from mission 0x1c.
pub fn unit_portrait(table: &[u16], mission: u16, t: u16) -> u16 {
    let p = table[t as usize];
    if p == 0x26 {
        if mission > 0x1f {
            return table[152];
        }
    } else if p == 0x3c && mission >= 0x1c {
        return table[78];
    }
    p
}

pub struct Portraits {
    /// arr\portdata.dat file ids (portdata.tbl strings, 1-based) and the names.
    files: Vec<u32>,
    names: Vec<String>,
    /// portdata.dat sections 2..5 (0x68ac50..0x68ac5c): per portrait the percent weight of
    /// idle variant 0..3.
    pub(crate) weights: [Vec<u8>; 4],
    /// 0x51acb0..0x51acb8 as StarCraft.exe starts them (read from the install's exe).
    rng: [u32; 3],
}

impl Portraits {
    pub fn load(files: &Storage) -> Option<Portraits> {
        let d = files.read("arr/portdata.dat").ok()?;
        let ids = (0..220).map(|i| d.get(i * 4..i * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))).collect();
        let names = crate::dat::parse_tbl(&files.read("arr/portdata.tbl").ok()?);
        let weights = std::array::from_fn(|i| d.get(880 + i * 110..990 + i * 110).unwrap_or(&[0; 110]).to_vec());
        let rng = std::fs::read(crate::install_dir().join("StarCraft.exe")).ok().and_then(|e| {
            let b = e.get(0x11acb0..0x11acbc)?;
            Some(std::array::from_fn(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())))
        });
        // Without the exe: any nonzero state (only the variant choices differ).
        Some(Portraits { files: ids, names, weights, rng: rng.unwrap_or([1, 2, 3]) })
    }

    /// "portrait\<name><variant>.smk" for portrait `p`, idle or talking.
    pub(crate) fn path(&self, p: u16, talking: bool, variant: u32) -> Option<String> {
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
    /// 0x515224 == 2: the portraits animate (set by the caller each tick); otherwise
    /// only variant 0 is opened.
    pub animate: bool,
    /// 0x6d754c: the last talking pick minus one (the byte 0x45e8a0 leaves in BL).
    last_talk: i8,
    video: Option<Smk>,
    elapsed: u32,
    /// 0x51acb0..0x51acb8: 0x45e450's state (taken from `Portraits::rng` on first use).
    rng: Option<[u32; 3]>,
    /// The palette the translation was made for, and the translation.
    remap: Option<([[u8; 3]; 256], [u8; 256])>,
    /// 0x6d5ca8: frames the video still has to run although the mode is not 2 (set to 1 by
    /// `begin`, so a still portrait shows its first frame).
    pub wait: u32,
    /// 0x68ac98 + 0x26: the unit whose portrait is shown (0 none / a talking advisor).
    unit: u32,
    /// 0x68ac9c: the static's threshold (drawn while below 0x50); 0x6d5ca0: set by a new
    /// portrait until its first video frame arrives.
    static_level: u8,
    static_wait: bool,
    /// Time into the 50 ms frame timer (1) and the 30 ms static timer (2) of 0x45ec40.
    t1: u32,
    t2: u32,
}

impl Default for Player {
    fn default() -> Self {
        Player { portrait: None, talking: false, animate: true, last_talk: 0, video: None, elapsed: 0, rng: None, remap: None, wait: 0, unit: 0, static_level: 0, static_wait: false, t1: 0, t2: 0 }
    }
}

impl Player {
    /// 0x45e450: a random number min..=max from the portrait generator; `mask1` (ECX) is
    /// the power of two above the range, it retries until the masked value fits.
    fn rand(&mut self, ps: &Portraits, mask1: u32, max: u32, min: u32) -> u32 {
        let st = self.rng.get_or_insert(ps.rng);
        loop {
            let u = st[2].wrapping_add(st[1]);
            st[0] = st[0].wrapping_add(u);
            st[1] = st[1].wrapping_add(0x71823402) ^ st[0];
            st[2] = u >> 1;
            let r = (st[1] ^ st[2]) & mask1.wrapping_sub(1);
            if r < max.wrapping_sub(min).wrapping_add(1) {
                return r + min;
            }
        }
    }

    /// 0x45e510: start a video. The out handle (`video`) is cleared; `mode` is 0x515224
    /// (0: no portraits, returns false), else 0x6d5ca8 = 1 and the SVid begin (Ordinal_454,
    /// done by `Smk::parse` in `open`) follows.
    pub fn begin(&mut self, mode: u8) -> bool {
        self.video = None;
        if mode == 0 {
            return false;
        }
        self.wait = 1;
        true
    }

    /// 0x45e4c0: the video step. `mode` is 0x515224; `stepped` is the result of the SVid
    /// step (Ordinal_457) and is only consulted when the mode is 2 or `wait` is set.
    /// Returns false when disabled or the step failed.
    pub fn step(&mut self, mode: u8, stepped: bool) -> bool {
        if mode == 0 {
            return false;
        }
        if mode == 2 || self.wait != 0 {
            if !stepped {
                return false;
            }
            if self.wait != 0 && self.video.is_some() {
                self.wait -= 1;
            }
        }
        true
    }

    /// 0x45e710: teardown. Frees the video (Ordinal_458) and the portrait buffer
    /// 0x6d5c9c (Rust ownership); the control 0x68ac8c is destroyed by 0x419ea0.
    pub fn close(&mut self) {
        self.video = None;
        todo_port!("0x419ea0", "destroy the portrait control 0x68ac8c");
    }

    /// 0x45e570: the static drawn into the portrait rect `[x1, y1, x2, y2]` of the 8-bit
    /// screen `out` (`sw` wide): each pixel whose roll (0..=100) is at least `threshold`
    /// takes a random one of the first 16 interface colors (0x6ceb20, `colors`).
    pub fn draw_static(&mut self, ps: &Portraits, threshold: u8, rect: [i16; 4], colors: &[u8], out: &mut [u8], sw: i32) {
        let w = rect[2] as i32 - rect[0] as i32 + 1;
        let h = rect[3] as i32 - rect[1] as i32 + 1;
        let mut at = rect[1] as i32 * sw + rect[0] as i32;
        for _ in 0..h.max(0) {
            for _ in 0..w.max(0) {
                if self.rand(ps, 128, 100, 0) >= threshold as u32 {
                    let c = colors[self.rand(ps, 16, 15, 0) as usize];
                    if let Some(px) = usize::try_from(at).ok().and_then(|i| out.get_mut(i)) {
                        *px = c;
                    }
                }
                at += 1;
            }
            at += sw - w;
        }
    }

    /// 0x45e7a0: the idle variant (a percent roll against portdata.dat's weights), or
    /// 0x45e8a0: the talking variant (0..2, not the last one), then its video, and on
    /// failure the lower variants down to 0. 0x45ea30 clears the screen and the static
    /// counter before; only the opening is here.
    fn open(&mut self, ps: &Portraits, files: &Storage) {
        self.video = None;
        self.elapsed = 0;
        let Some(p) = self.portrait else { return };
        let mut c: i32 = 0;
        if self.animate {
            if self.talking {
                c = self.rand(ps, 4, 2, 0) as i32;
                if c == self.last_talk as i32 {
                    c = (c + 2) % 3;
                }
            } else {
                let r = self.rand(ps, 128, 100, 0) as u8;
                let w = |i: usize| ps.weights[i].get(p as usize).copied().unwrap_or(0);
                let mut sum = w(0);
                c = 0;
                // Index 4 would read through 0x68ac60 (the talking flag); the data's
                // weights add to 100 so it is never reached.
                while r > sum {
                    c += 1;
                    if c == 4 {
                        c = 0;
                        break;
                    }
                    sum = sum.wrapping_add(w(c as usize));
                }
            }
        }
        while c >= 0 {
            if let Some(path) = ps.path(p, self.talking, c as u32) {
                self.video = files.read(&path).ok().and_then(Smk::parse);
                if let Some(s) = self.video.as_mut() {
                    s.next_frame();
                }
            }
            c -= 1;
            if self.video.is_some() {
                break;
            }
        }
        if self.talking {
            self.last_talk = c as i8;
        }
    }

    /// 0x45ebc0: the selection's portrait update. `unit` is the status unit (0x597248, 0 for
    /// none), `portrait` its portrait (None hides). Another unit restarts the portrait even
    /// with the same portrait id; the same unit keeps it (its wire_type compared with
    /// itself is always equal).
    pub fn show_unit(&mut self, ps: &Portraits, files: &Storage, unit: u32, portrait: Option<u16>) {
        if unit == 0 || portrait.is_none() {
            self.unit = 0;
            self.hide();
            return;
        }
        if unit == self.unit && self.portrait == portrait {
            return;
        }
        self.unit = unit;
        self.talking = false;
        self.portrait = portrait;
        self.static_level = 0;
        self.static_wait = true;
        self.open(ps, files);
    }

    /// The status unit's portrait (None for no selection or a unit without one); a new
    /// one starts idling. Prefer `show_unit` (0x45ebc0 restarts per unit).
    pub fn show(&mut self, ps: &Portraits, files: &Storage, portrait: Option<u16>) {
        if portrait != self.portrait {
            self.portrait = portrait;
            self.talking = false;
            self.open(ps, files);
        }
    }

    /// 0x45ea30 (mode 2) -> 0x45e8a0: another unit's portrait `p` shown talking.
    pub fn show_talking(&mut self, ps: &Portraits, files: &Storage, p: u16) {
        self.portrait = Some(p);
        self.unit = 0;
        self.talking = true;
        self.static_level = 0;
        self.static_wait = true;
        self.open(ps, files);
    }

    /// 0x418700 on the portrait control: nothing shown until the next `show*`.
    pub fn hide(&mut self) {
        self.portrait = None;
        self.video = None;
        self.unit = 0;
    }

    /// 0x45e650: after the video, the static while 0x68ac9c < 0x50 (the caller blits
    /// conover.pcx over it). `rect` is the portrait control's `[l, t, r, b]`.
    pub fn draw_static_overlay(&mut self, ps: &Portraits, rect: [i16; 4], colors: &[u8], out: &mut [u8], sw: i32) {
        if self.static_level < 0x50 {
            self.draw_static(ps, self.static_level, rect, colors, out, sw);
        }
    }

    /// Advance `ms`; `animate` false (still portraits) keeps the first frame. A video
    /// that ends is replaced by a new pick, idle or talking as before (0x45ec40 timer 1
    /// calls 0x45ea30 again for the same unit).
    pub fn tick(&mut self, ps: &Portraits, files: &Storage, ms: u32, animate: bool) {
        // 0x45ec40 timer 1 (50 ms): the first video frame ends the full static (0x32);
        // timer 2 (30 ms): the static fades in steps of 10 up to 0x50.
        if self.video.is_some() {
            self.t1 += ms;
            self.t2 += ms;
            if self.t1 >= 50 {
                self.t1 %= 50;
                if self.static_wait {
                    self.static_wait = false;
                    self.static_level = 0x32;
                }
            }
            while self.t2 >= 30 {
                self.t2 -= 30;
                if !self.static_wait && self.static_level < 0x50 {
                    self.static_level += 10;
                }
            }
        }
        if !animate {
            return;
        }
        let Some(v) = self.video.as_mut() else { return };
        self.elapsed += ms;
        while self.elapsed >= v.ms.max(1) {
            self.elapsed -= v.ms.max(1);
            if !v.next_frame() {
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
