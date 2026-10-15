//! Loading the read-only game data the engine needs from the user's install.

use super::Data;
use crate::storage::Storage;
use crate::dat::{self, Dat};
use std::io;

pub struct Loaded {
    pub dat: Dat,
    pub iscript: Vec<u8>,
    /// (frames, width, height) per images.dat id, from the GRP header.
    pub grp: Vec<(u16, u16, u16)>,
    /// LO files: [attack, damage, special, landing, liftoff][image id].
    pub lo: [Vec<Option<Vec<u8>>>; 6],
    /// GRP of the psi field image (its frame is the pylon power shape).
    pub psi_grp: Vec<u8>,
    /// GRP files by images.dat grp index.
    pub grp_files: Vec<Vec<u8>>,
    /// 0x4313c0 strength table, computed before any scenario changes the dat tables.
    pub strength: [[u32; 228]; 2],
    /// 0x68c104 / 0x68c108: scripts\aiscript.bin and scripts\bwscript.bin (0x45b5f0).
    pub aiscript: Vec<u8>,
    pub bwscript: Vec<u8>,
}

impl Loaded {
    pub fn load(files: &Storage) -> io::Result<Loaded> {
        let dat = Dat::load(files)?;
        let iscript = files.read("scripts/iscript.bin")?;
        let names = dat::parse_tbl(&files.read("arr/images.tbl")?);
        let file = |idx: u32| -> Option<Vec<u8>> {
            let name = names.get((idx as usize).checked_sub(1)?)?;
            files.read(&format!("unit/{name}")).ok()
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
        let cols = [&im.attack_overlay, &im.damage_overlay, &im.special_overlay, &im.landing_dust, &im.liftoff_dust, &im.shield_overlay];
        let lo = cols.map(|c| (0..n).map(|i| if c[i] == 0 { None } else { file(c[i]) }).collect());
        let psi_grp = file(im.grp[super::psi::PSI_FIELD_IMAGE as usize]).unwrap_or_default();
        let max = im.grp.iter().copied().max().unwrap_or(0) as usize;
        let mut grp_files = vec![Vec::new(); max + 1];
        for &g in &im.grp {
            if grp_files[g as usize].is_empty() {
                grp_files[g as usize] = file(g).unwrap_or_default();
            }
        }
        let strength = super::ai::strength_table(&dat);
        let aiscript = files.read("scripts\\aiscript.bin").unwrap_or_default();
        let bwscript = files.read("scripts\\bwscript.bin").unwrap_or_default();
        Ok(Loaded { dat, iscript, grp, lo, psi_grp, grp_files, strength, aiscript, bwscript })
    }

    pub fn grp_size(&self, image: u16) -> (u16, u16, u16) {
        self.grp.get(image as usize).copied().unwrap_or((0, 0, 0))
    }
}

/// Build a `Data` view over loaded data. `grp` must be `|i| loaded.grp_size(i)`.
pub fn data<'a>(l: &'a Loaded, grp: &'a dyn Fn(u16) -> (u16, u16, u16)) -> Data<'a> {
    Data { dat: &l.dat, iscript: &l.iscript, grp, lo: &l.lo, grp_files: &l.grp_files, strength: &l.strength, aiscript: &l.aiscript, bwscript: &l.bwscript }
}
