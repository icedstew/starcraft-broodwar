//! The game's files, read from a StarCraft: Brood War 1.16.1 install: the patch and
//! data archives in the order the game searches them, then the CD archives (music).

use crate::mpq::Mpq;
use std::io;
use std::path::Path;

pub struct Storage {
    archives: Vec<Mpq>,
}

/// Archives by priority; the later two are the CD images (optional).
const ARCHIVES: [&str; 5] = ["Patch_rt.mpq", "BrooDat.mpq", "StarDat.mpq", "BroodWar.mpq", "Starcraft.mpq"];

impl Storage {
    pub fn open(dir: &Path) -> io::Result<Storage> {
        crate::storm::load(&std::fs::read(dir.join("storm.dll"))?)?;
        let mut archives = Vec::new();
        for name in ARCHIVES {
            // File names differ in case between releases.
            let found = std::fs::read_dir(dir)?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name));
            if let Some(e) = found {
                archives.push(Mpq::open(&e.path())?);
            }
        }
        if archives.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("no StarCraft archives in {}", dir.display())));
        }
        Ok(Storage { archives })
    }

    /// Read `name` ("unit/terran/marine.grp" or with backslashes) from the first
    /// archive that has it.
    pub fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let name = name.replace('/', "\\");
        for a in &self.archives {
            match a.read(&name) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                r => return r,
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, name))
    }
}
