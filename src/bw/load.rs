//! Loading the read-only game data the engine needs from the user's install.

use super::Data;
use crate::casc::Casc;
use crate::dat::{self, Dat};
use std::io;

pub struct Loaded {
    pub dat: Dat,
    pub iscript: Vec<u8>,
    /// (frames, width, height) per images.dat id, from the GRP header.
    pub grp: Vec<(u16, u16, u16)>,
    /// LO files: [attack, damage, special, landing, liftoff][image id].
    pub lo: [Vec<Option<Vec<u8>>>; 5],
}

impl Loaded {
    pub fn load(casc: &Casc) -> io::Result<Loaded> {
        let dat = Dat::load(casc)?;
        let iscript = casc.read("scripts/iscript.bin")?;
        let names = dat::parse_tbl(&casc.read("arr/images.tbl")?);
        let file = |idx: u32| -> Option<Vec<u8>> {
            let name = names.get((idx as usize).checked_sub(1)?)?;
            casc.read(&format!("unit/{name}")).ok()
        };
        let n = dat::Images::COUNT;
        let im = &dat.images;
        let grp = (0..n)
            .map(|i| {
                file(im.grp[i]).map_or((0, 0, 0), |g| {
                    let r = |o: usize| u16::from_le_bytes([g.get(o).copied().unwrap_or(0), g.get(o + 1).copied().unwrap_or(0)]);
                    (r(0), r(2), r(4))
                })
            })
            .collect();
        let cols = [&im.attack_overlay, &im.damage_overlay, &im.special_overlay, &im.landing_dust, &im.liftoff_dust];
        let lo = cols.map(|c| (0..n).map(|i| if c[i] == 0 { None } else { file(c[i]) }).collect());
        Ok(Loaded { dat, iscript, grp, lo })
    }

    pub fn grp_size(&self, image: u16) -> (u16, u16, u16) {
        self.grp.get(image as usize).copied().unwrap_or((0, 0, 0))
    }
}

/// Build a `Data` view over loaded data. `grp` must be `|i| loaded.grp_size(i)`.
pub fn data<'a>(l: &'a Loaded, grp: &'a dyn Fn(u16) -> (u16, u16, u16)) -> Data<'a> {
    Data { dat: &l.dat, iscript: &l.iscript, grp, lo: &l.lo }
}
