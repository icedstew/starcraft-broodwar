//! Read-only access to a local CASC install (StarCraft: Remastered).
//!
//! SC:R uses the plain-text root ("path|ckey" per line), so the chain is
//! path -> ckey (root) -> ekey (encoding) -> archive/offset (.idx) -> BLTE.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

type Key = [u8; 16];

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |a, &x| (a << 8) | x as u64)
}

fn hex_key(s: &str) -> io::Result<Key> {
    let s = s.trim();
    if s.len() != 32 {
        return Err(err(format!("bad key {s}")));
    }
    let mut k = [0u8; 16];
    for i in 0..16 {
        k[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| err(format!("bad key {s}")))?;
    }
    Ok(k)
}

pub struct Casc {
    data_dir: PathBuf,
    index: HashMap<[u8; 9], (u32, u64, u32)>, // ekey prefix -> (archive, offset, size)
    encoding: HashMap<Key, Key>,              // ckey -> first ekey
    root: HashMap<String, Key>,               // lowercased path -> ckey
}

impl Casc {
    /// `install` is the game dir containing `.build.info` and `Data/`.
    pub fn open(install: &Path) -> io::Result<Casc> {
        let build_info = std::fs::read_to_string(install.join(".build.info"))?;
        let mut lines = build_info.lines();
        let hdr: Vec<&str> = lines.next().ok_or_else(|| err("empty .build.info"))?.split('|').collect();
        let col = hdr.iter().position(|h| h.starts_with("Build Key")).ok_or_else(|| err("no Build Key"))?;
        let active = hdr.iter().position(|h| h.starts_with("Active"));
        let row: Vec<&str> = lines
            .map(|l| l.split('|').collect::<Vec<_>>())
            .find(|r| active.is_none_or(|a| r.get(a) == Some(&"1")))
            .ok_or_else(|| err("no active build"))?;
        let bk = row[col];
        let data_dir = install.join("Data");
        let cfg = std::fs::read_to_string(data_dir.join("config").join(&bk[0..2]).join(&bk[2..4]).join(bk))?;
        let cfg_val = |name: &str| -> io::Result<Vec<&str>> {
            cfg.lines()
                .find_map(|l| l.strip_prefix(name)?.trim_start().strip_prefix('=').map(|v| v.split_whitespace().collect()))
                .ok_or_else(|| err(format!("build config missing {name}")))
        };
        let root_ckey = hex_key(cfg_val("root")?[0])?;
        let enc_ekey = hex_key(cfg_val("encoding")?[1])?;

        let mut casc = Casc { data_dir, index: HashMap::new(), encoding: HashMap::new(), root: HashMap::new() };
        casc.load_indices()?;
        let enc = casc.read_ekey(&enc_ekey)?;
        casc.encoding = parse_encoding(&enc)?;
        let root_ekey = *casc.encoding.get(&root_ckey).ok_or_else(|| err("root ckey not in encoding"))?;
        let root = casc.read_ekey(&root_ekey)?;
        for line in String::from_utf8_lossy(&root).lines() {
            if let Some((path, ck)) = line.rsplit_once('|') {
                if let Ok(k) = hex_key(ck) {
                    casc.root.insert(normalize(path), k);
                }
            }
        }
        Ok(casc)
    }

    fn load_indices(&mut self) -> io::Result<()> {
        // Highest version per bucket wins: file name is BB + 8 hex digits version.
        let mut best: HashMap<String, String> = HashMap::new();
        for e in std::fs::read_dir(self.data_dir.join("data"))? {
            let name = e?.file_name().to_string_lossy().into_owned();
            if name.len() == 14 && name.ends_with(".idx") {
                let b = name[0..2].to_string();
                if best.get(&b).is_none_or(|cur| *cur < name) {
                    best.insert(b, name);
                }
            }
        }
        for name in best.values() {
            let d = std::fs::read(self.data_dir.join("data").join(name))?;
            let hdr_len = u32::from_le_bytes(d[0..4].try_into().unwrap()) as usize;
            let h = &d[8..8 + hdr_len];
            let (len_bytes, loc_bytes, key_bytes, off_bits) = (h[4] as usize, h[5] as usize, h[6] as usize, h[7] as u32);
            if key_bytes != 9 {
                return Err(err("unexpected idx key size"));
            }
            let ent_start = (8 + hdr_len + 0x0F) & !0x0F;
            let ent_len = u32::from_le_bytes(d[ent_start..ent_start + 4].try_into().unwrap()) as usize;
            let rec = key_bytes + loc_bytes + len_bytes;
            let entries = &d[ent_start + 8..(ent_start + 8 + ent_len).min(d.len())];
            for r in entries.chunks_exact(rec) {
                let key: [u8; 9] = r[0..9].try_into().unwrap();
                let loc = be(&r[9..9 + loc_bytes]);
                let size = u32::from_le_bytes(r[9 + loc_bytes..9 + loc_bytes + 4].try_into().unwrap());
                self.index.entry(key).or_insert(((loc >> off_bits) as u32, loc & ((1 << off_bits) - 1), size));
            }
        }
        Ok(())
    }

    fn read_ekey(&self, ekey: &Key) -> io::Result<Vec<u8>> {
        let k: [u8; 9] = ekey[0..9].try_into().unwrap();
        let &(arc, off, size) = self.index.get(&k).ok_or_else(|| err("ekey not in local index"))?;
        let mut f = File::open(self.data_dir.join("data").join(format!("data.{arc:03}")))?;
        f.seek(SeekFrom::Start(off + 30))?; // skip 30-byte local header
        let mut buf = vec![0u8; size as usize - 30];
        f.read_exact(&mut buf)?;
        blte_decode(&buf)
    }

    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let ck = self.root.get(&normalize(path)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))?;
        let ek = self.encoding.get(ck).ok_or_else(|| err(format!("{path}: ckey not in encoding")))?;
        self.read_ekey(ek)
    }

    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.root.keys().map(|s| s.as_str())
    }
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").to_ascii_lowercase()
}

fn parse_encoding(d: &[u8]) -> io::Result<HashMap<Key, Key>> {
    if &d[0..2] != b"EN" {
        return Err(err("bad encoding magic"));
    }
    let ck_size = d[3] as usize;
    let ek_size = d[4] as usize;
    let page_size = be(&d[5..7]) as usize * 1024;
    let page_count = be(&d[9..13]) as usize;
    let espec_size = be(&d[18..22]) as usize;
    let pages_start = 22 + espec_size + page_count * (ck_size + 16);
    let mut map = HashMap::new();
    for p in 0..page_count {
        let page = &d[pages_start + p * page_size..pages_start + (p + 1) * page_size];
        let mut i = 0;
        while i + 6 + ck_size <= page.len() {
            let n = page[i] as usize;
            if n == 0 {
                break;
            }
            let ck: Key = page[i + 6..i + 6 + 16].try_into().unwrap();
            let ek: Key = page[i + 6 + ck_size..i + 6 + ck_size + 16].try_into().unwrap();
            map.insert(ck, ek);
            i += 6 + ck_size + n * ek_size;
        }
    }
    Ok(map)
}

pub fn blte_decode(d: &[u8]) -> io::Result<Vec<u8>> {
    if &d[0..4] != b"BLTE" {
        return Err(err("bad BLTE magic"));
    }
    let hdr = be(&d[4..8]) as usize;
    let mut out = Vec::new();
    if hdr == 0 {
        decode_chunk(&d[8..], &mut out)?;
        return Ok(out);
    }
    let count = be(&d[9..12]) as usize;
    let mut pos = hdr;
    for c in 0..count {
        let e = &d[12 + c * 24..];
        let comp = be(&e[0..4]) as usize;
        decode_chunk(&d[pos..pos + comp], &mut out)?;
        pos += comp;
    }
    Ok(out)
}

fn decode_chunk(c: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
    match c[0] {
        b'N' => out.extend_from_slice(&c[1..]),
        b'Z' => {
            flate2::read::ZlibDecoder::new(&c[1..]).read_to_end(out)?;
        }
        b'F' => out.extend(blte_decode(&c[1..])?),
        m => return Err(err(format!("unsupported BLTE chunk mode {:?}", m as char))),
    }
    Ok(())
}
