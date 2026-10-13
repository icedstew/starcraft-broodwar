//! Minimal MPQ (v1) reader: enough for .scm/.scx maps and classic .mpq archives.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Mutex, OnceLock};

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn crypt_table() -> &'static [u32; 0x500] {
    static T: OnceLock<[u32; 0x500]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 0x500];
        let mut seed: u32 = 0x0010_0001;
        for i in 0..0x100 {
            for j in 0..5 {
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                let hi = (seed & 0xFFFF) << 16;
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                t[i + j * 0x100] = hi | (seed & 0xFFFF);
            }
        }
        t
    })
}

pub fn hash(s: &str, kind: u32) -> u32 {
    let t = crypt_table();
    let (mut s1, mut s2) = (0x7FED_7FEDu32, 0xEEEE_EEEEu32);
    for c in s.bytes() {
        let c = c.to_ascii_uppercase() as u32;
        s1 = t[(kind * 0x100 + c) as usize] ^ s1.wrapping_add(s2);
        s2 = c.wrapping_add(s1).wrapping_add(s2).wrapping_add(s2 << 5).wrapping_add(3);
    }
    s1
}

fn decrypt(data: &mut [u32], mut key: u32) {
    let t = crypt_table();
    let mut seed = 0xEEEE_EEEEu32;
    for v in data {
        seed = seed.wrapping_add(t[0x400 + (key & 0xFF) as usize]);
        let ch = *v ^ key.wrapping_add(seed);
        key = ((!key << 21).wrapping_add(0x1111_1111)) | (key >> 11);
        seed = ch.wrapping_add(seed).wrapping_add(seed << 5).wrapping_add(3);
        *v = ch;
    }
}

fn decrypt_bytes(data: &mut [u8], key: u32) {
    let n = data.len() / 4;
    let mut w: Vec<u32> = data[..n * 4].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    decrypt(&mut w, key);
    for (i, v) in w.iter().enumerate() {
        data[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

struct Block {
    pos: u32,
    csize: u32,
    size: u32,
    flags: u32,
}

/// Archive bytes: in memory (maps), or read on demand from a file (game archives,
/// which can be hundreds of MB).
enum Source {
    Mem(Vec<u8>),
    File(Mutex<File>, u64),
}

impl Source {
    fn len(&self) -> u64 {
        match self {
            Source::Mem(d) => d.len() as u64,
            Source::File(_, n) => *n,
        }
    }

    fn get(&self, pos: usize, len: usize) -> io::Result<Vec<u8>> {
        match self {
            Source::Mem(d) => d.get(pos..pos + len).map(|b| b.to_vec()).ok_or_else(|| err("read out of range")),
            Source::File(f, n) => {
                if (pos + len) as u64 > *n {
                    return Err(err("read out of range"));
                }
                let mut f = f.lock().map_err(|_| err("archive lock"))?;
                f.seek(SeekFrom::Start(pos as u64))?;
                let mut b = vec![0; len];
                f.read_exact(&mut b)?;
                Ok(b)
            }
        }
    }
}

pub struct Mpq {
    data: Source,
    base: usize,
    sector_size: usize,
    hashes: Vec<[u32; 4]>, // nameA, nameB, locale|platform, block index
    blocks: Vec<Block>,
}

const F_IMPLODE: u32 = 0x100;
const F_COMPRESS: u32 = 0x200;
const F_ENCRYPTED: u32 = 0x1_0000;
const F_FIX_KEY: u32 = 0x2_0000;
const F_SINGLE: u32 = 0x100_0000;
const F_EXISTS: u32 = 0x8000_0000;

impl Mpq {
    pub fn new(data: Vec<u8>) -> io::Result<Mpq> {
        Mpq::from_source(Source::Mem(data))
    }

    fn from_source(data: Source) -> io::Result<Mpq> {
        let base = (0..data.len().saturating_sub(32) as usize)
            .step_by(512)
            .find(|&o| data.get(o, 4).is_ok_and(|m| m == b"MPQ\x1A"))
            .ok_or_else(|| err("no MPQ header"))?;
        let hdr = data.get(base, 32)?;
        let rd = |o: usize| u32::from_le_bytes(hdr[o..o + 4].try_into().unwrap());
        let sector_size = 512usize << (rd(0x0E) & 0xFFFF);
        let (ht_pos, bt_pos, ht_n, bt_n) = (rd(0x10) as usize, rd(0x14) as usize, rd(0x18) as usize, rd(0x1C) as usize);
        // ponytail: trusts table sizes; map "protectors" that lie about them are not handled.
        let table = |pos: usize, n: usize, key: &str| -> io::Result<Vec<u32>> {
            let bytes = data.get(base + pos, n * 16)?;
            let mut w: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            decrypt(&mut w, hash(key, 3));
            Ok(w)
        };
        let ht = table(ht_pos, ht_n, "(hash table)")?;
        let bt = table(bt_pos, bt_n, "(block table)")?;
        let hashes = ht.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        let blocks = bt.chunks_exact(4).map(|c| Block { pos: c[0], csize: c[1], size: c[2], flags: c[3] }).collect();
        Ok(Mpq { data, base, sector_size, hashes, blocks })
    }

    /// Open an archive file, reading its files from disk on demand.
    pub fn open(path: &std::path::Path) -> io::Result<Mpq> {
        let f = File::open(path)?;
        let n = f.metadata()?.len();
        Mpq::from_source(Source::File(Mutex::new(f), n))
    }

    pub fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let n = self.hashes.len();
        let start = hash(name, 0) as usize % n;
        let (a, b) = (hash(name, 1), hash(name, 2));
        let mut bi = None;
        for i in 0..n {
            let h = &self.hashes[(start + i) % n];
            if h[3] == 0xFFFF_FFFF {
                break;
            }
            if h[0] == a && h[1] == b {
                bi = Some(h[3] as usize);
                break;
            }
        }
        let blk = bi.and_then(|i| self.blocks.get(i)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_string()))?;
        if blk.flags & F_EXISTS == 0 {
            return Err(io::Error::new(io::ErrorKind::NotFound, name.to_string()));
        }
        if std::env::var_os("SCBW_MPQ_DEBUG").is_some() {
            eprintln!("{name}: pos {:#x} csize {} size {} flags {:#x}", blk.pos, blk.csize, blk.size, blk.flags);
        }
        let raw = self.data.get(self.base + blk.pos as usize, blk.csize as usize)?;
        let raw = &raw[..];
        let mut key = 0;
        if blk.flags & F_ENCRYPTED != 0 {
            let base_name = name.rsplit(['\\', '/']).next().unwrap();
            key = hash(base_name, 3);
            if blk.flags & F_FIX_KEY != 0 {
                key = (key.wrapping_add(blk.pos)) ^ blk.size;
            }
        }
        let compressed = blk.flags & (F_IMPLODE | F_COMPRESS) != 0;
        let size = blk.size as usize;
        if blk.flags & F_SINGLE != 0 || !compressed {
            // Single-unit files are one "sector"; uncompressed multi-sector files are contiguous.
            let mut out = Vec::with_capacity(size);
            let ss = if blk.flags & F_SINGLE != 0 { raw.len().max(1) } else { self.sector_size };
            for (i, chunk) in raw.chunks(ss).enumerate() {
                let mut c = chunk.to_vec();
                if key != 0 {
                    decrypt_bytes(&mut c, key.wrapping_add(i as u32));
                }
                let want = (size - out.len()).min(if blk.flags & F_SINGLE != 0 { size } else { ss });
                out.extend(decompress(&c, want, blk.flags)?);
            }
            out.truncate(size);
            return Ok(out);
        }
        let nsec = size.div_ceil(self.sector_size);
        let raw_sectors = std::env::var_os("SCBW_MPQ_RAW");
        let mut offs = raw.get(..(nsec + 1) * 4).ok_or_else(|| err("bad sector table"))?.to_vec();
        if key != 0 {
            decrypt_bytes(&mut offs, key.wrapping_sub(1));
        }
        let offs: Vec<usize> = offs.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect();
        let mut out = Vec::with_capacity(size);
        for i in 0..nsec {
            let mut c = raw.get(offs[i]..offs[i + 1]).ok_or_else(|| err("bad sector"))?.to_vec();
            if key != 0 {
                decrypt_bytes(&mut c, key.wrapping_add(i as u32));
            }
            let want = (size - out.len()).min(self.sector_size);
            if let Some(dir) = &raw_sectors {
                // Debug: dump each compressed sector and its size for decompressor tests.
                std::fs::write(std::path::Path::new(dir).join(format!("{i}_{want}")), &c)?;
            }
            out.extend(decompress(&c, want, blk.flags)?);
        }
        Ok(out)
    }
}

fn decompress(c: &[u8], want: usize, flags: u32) -> io::Result<Vec<u8>> {
    if c.len() >= want {
        return Ok(c[..want].to_vec());
    }
    if flags & F_IMPLODE != 0 {
        return explode::explode(c).map_err(|e| err(format!("explode: {e:?}")));
    }
    if flags & F_COMPRESS == 0 {
        return Ok(c.to_vec());
    }
    let mask = c[0];
    let mut d = c[1..].to_vec();
    // storm.dll's SCompDecompress applies these in order: implode, Huffman, stereo
    // ADPCM, mono ADPCM (table at 0x15050b50); zlib comes from later archive formats.
    if mask & 0x02 != 0 {
        let mut out = Vec::with_capacity(want);
        flate2::read::ZlibDecoder::new(&d[..]).read_to_end(&mut out)?;
        d = out;
    }
    if mask & 0x08 != 0 {
        d = explode::explode(&d).map_err(|e| err(format!("explode: {e:?}")))?;
    }
    if mask & 0x01 != 0 {
        d = crate::storm::huffman_decompress(&d, want)?;
    }
    if mask & 0x80 != 0 {
        d = crate::storm::adpcm_decompress(&d, want, 2)?;
    }
    if mask & 0x40 != 0 {
        d = crate::storm::adpcm_decompress(&d, want, 1)?;
    }
    if mask & !0xcb != 0 {
        return Err(err(format!("unsupported MPQ compression 0x{mask:02x}")));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_known_values() {
        // Well-known constants used by every MPQ implementation.
        assert_eq!(super::hash("(hash table)", 3), 0xC3AF_3770);
        assert_eq!(super::hash("(block table)", 3), 0xEC83_B3A3);
    }
}
