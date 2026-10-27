//! The game description (0x8d bytes, the game's 0x5967f8) as a Storm stat string: what a
//! created game advertises (0x472300, from 0x4d3910) and what the join list reads back
//! (0x472570, from 0x4d35e0).
//!
//! The description: +0 the Storm game id, +4 the name (0x18), then from +0x1c the fields the
//! stat string carries: +0x1c a dword, +0x20 / +0x22 the map size, +0x24 the players and +0x25
//! the most, +0x26 the game speed, +0x27, +0x28 the game type, +0x29, +0x2a the subtype (u16),
//! +0x2c a dword (0x4d9cd0's), +0x30 the tileset (u16), +0x32, +0x34 the host's name (0x19)
//! and +0x4d the map's (0x20); +0x6d the game type record (0x20).

use crate::setup::Setup;

/// Where the stat string's fields start in the description.
const BASE: usize = 0x1c;

/// 0x472300 (EBX = the description + 0x1c, the buffer of `size` bytes): each number as
/// "%x," (just "," when it is the field's default), then the host's and the map's names
/// (0x472210: a '\r' in them becomes '\n', each ends with '\r'), then a NUL.
pub fn build(d: &[u8], size: usize) -> Vec<u8> {
    let f = &d[BASE..];
    let u16_at = |o: usize| u16::from_le_bytes([f[o], f[o + 1]]) as u32;
    let u32_at = |o: usize| u32::from_le_bytes([f[o], f[o + 1], f[o + 2], f[o + 3]]);
    let fields = [
        (u32_at(0), 0),
        (u16_at(4) >> 1 & 0xf0 | (u16_at(6) & 0x1fe0) >> 5, 0x88),
        (((f[8] as u32) << 4 | f[9] as u32) & 0xff, 0x18),
        (f[0xa] as u32, 4),
        (f[0xb] as u32, 0),
        (f[0xc] as u32, 0),
        (f[0xd] as u32, 0),
        (u16_at(0xe), 0),
        (u32_at(0x10), 0),
        (u16_at(0x14), 0),
        (f[0x16] as u32, 0),
    ];
    // 0x41f1b0 (_snprintf into what is left; a cut-off write uses the rest up).
    let mut out = Vec::new();
    let mut left = size;
    for (v, default) in fields {
        let s = if v == default { ",".to_string() } else { format!("{v:x},") };
        let n = s.len().min(left);
        out.extend_from_slice(&s.as_bytes()[..n]);
        left -= n;
    }
    for (at, max) in [(0x18, 0x19), (0x31, 0x20)] {
        let name = &f[at..at + max];
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(max)];
        out.extend(name.iter().map(|&c| if c == b'\r' { b'\n' } else { c }));
        out.push(b'\r');
    }
    out
}

/// 0x472260 (ECX = the text, EAX = the default, EDI = the field): one hex number up to ','
/// (an empty one is the default). The rest of the text, or None when a character is not a
/// hex digit (the field keeps what was read so far).
fn field<'a>(s: Option<&'a [u8]>, default: u32, out: &mut u32) -> Option<&'a [u8]> {
    let Some(s) = s else {
        *out = default;
        return None;
    };
    if s.first() == Some(&b',') {
        *out = default;
        return Some(&s[1..]);
    }
    *out = 0;
    let mut i = 0;
    loop {
        // The text is NUL-terminated natively: past its end reads the NUL.
        let c = s.get(i).copied().unwrap_or(0);
        if c == b',' {
            return Some(&s[i + 1..]);
        }
        i += 1;
        let c = c.to_ascii_lowercase();
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => return None,
        };
        *out = out.wrapping_mul(16).wrapping_add(v as u32);
    }
}

/// 0x472570 (ECX = the stat string, at most 0x80 bytes with its NUL; EAX = the description +
/// 0x1c): the fields back into `d`, then 0x472500's check. False when the text is too long,
/// a name is missing or not '\r'-terminated, or the check fails.
pub fn parse(stat: &[u8], d: &mut [u8]) -> bool {
    let len = stat.iter().position(|&c| c == 0).unwrap_or(stat.len());
    if len >= 0x80 {
        return false;
    }
    let mut s = Some(&stat[..len]);
    let f = &mut d[BASE..];
    let mut v = 0u32;
    s = field(s, 0, &mut v);
    f[0..4].copy_from_slice(&v.to_le_bytes());
    let mut v = 0;
    s = field(s, 0x88, &mut v);
    f[6..8].copy_from_slice(&(((v & 0xf) << 5) as u16).to_le_bytes());
    f[4..6].copy_from_slice(&((((v >> 4) & 0xf) << 5) as u16).to_le_bytes());
    let mut v = 0;
    s = field(s, 0x18, &mut v);
    f[8] = (v >> 4 & 0xf) as u8;
    f[9] = (v & 0xf) as u8;
    for (at, default) in [(0xa, 4), (0xb, 0), (0xc, 0), (0xd, 0)] {
        let mut v = 0;
        s = field(s, default, &mut v);
        f[at] = v as u8;
    }
    let mut v = 0;
    s = field(s, 0, &mut v);
    f[0xe..0x10].copy_from_slice(&(v as u16).to_le_bytes());
    let mut v = 0;
    s = field(s, 0, &mut v);
    f[0x10..0x14].copy_from_slice(&v.to_le_bytes());
    let mut v = 0;
    s = field(s, 0, &mut v);
    f[0x14..0x16].copy_from_slice(&(v as u16).to_le_bytes());
    let mut v = 0;
    s = field(s, 0, &mut v);
    f[0x16] = v as u8;
    // The host's name (at most 0x18 characters) and the map's (0x1f), each up to '\r'; the
    // map's '\r' must end the text.
    let Some(rest) = s.and_then(|s| name(s, &mut f[0x18..0x31], 0x19)) else { return false };
    match name(rest, &mut f[0x31..0x51], 0x20) {
        Some(r) if r.is_empty() => check(f),
        _ => false,
    }
}

/// One '\r'-terminated name (fewer than `max` characters, no NUL) into `out`, NUL-terminated;
/// the text after the '\r'.
fn name<'a>(s: &'a [u8], out: &mut [u8], max: usize) -> Option<&'a [u8]> {
    let n = s.iter().position(|&c| c == b'\r')?;
    if n >= max || s[..n].contains(&0) {
        return None;
    }
    out[..n].copy_from_slice(&s[..n]);
    out[n] = 0;
    Some(&s[n + 1..])
}

/// 0x472500 (ESI = the description + 0x1c): sizes at most 256, players <= most <= 8, speed
/// < 7, both names set, the map's a valid text (0x485980: 0x20, color codes allowed), +0x27 < 7,
/// the tileset < 8.
fn check(f: &[u8]) -> bool {
    let w = u16::from_le_bytes([f[4], f[5]]);
    let h = u16::from_le_bytes([f[6], f[7]]);
    w < 0x101
        && h < 0x101
        && f[8] <= f[9]
        && f[9] < 9
        && f[0xa] < 7
        && f[0x18] != 0
        && f[0x31] != 0
        && Setup::text_length(&f[0x31..0x51], 0x20, 0x20, false, true) != 0
        && f[0xb] < 7
        && u16::from_le_bytes([f[0x14], f[0x15]]) < 8
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let mut d = vec![0u8; 0x8d];
        d[0x20..0x22].copy_from_slice(&128u16.to_le_bytes());
        d[0x22..0x24].copy_from_slice(&96u16.to_le_bytes());
        d[0x24] = 1;
        d[0x25] = 4;
        d[0x26] = 6;
        d[0x28] = 2;
        d[0x29] = 0;
        d[0x2a] = 1;
        d[0x30] = 3;
        d[0x34..0x37].copy_from_slice(b"Bob");
        d[0x4d..0x55].copy_from_slice(b"Big Game");
        let s = super::build(&d, 0x80);
        assert_eq!(s, b",43,14,6,,2,,1,,3,,Bob\rBig Game\r");
        let mut e = vec![0u8; 0x8d];
        assert!(super::parse(&s, &mut e));
        assert_eq!(&e[0x1c..0x6d], &d[0x1c..0x6d]);
    }
}
