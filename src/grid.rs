//! 1.16.1 file encryption (grid.cpp) used for the fonts: a keystream from IDEA (Colin
//! Plumb's code on native little-endian words) and a Blizzard SHA-0 whose rotates use
//! arithmetic shifts, chained over 64-byte blocks.

/// MSVC `rand` (0x40c7b0 seeds it).
struct Rand(u32);

impl Rand {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        (self.0 >> 16) as u8
    }
}

const IV: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];

/// 0x42ae90: one SHA-0 transform of `block` (read as little-endian dwords) into `h`.
/// Rotates are built from `sar`, so they smear the sign bit.
fn sha_transform(h: &mut [u32; 5], block: &[u8]) {
    let rol5 = |x: u32| ((x as i32 >> 27) as u32) | x << 5;
    let rol30 = |x: u32| x << 30 | ((x as i32 >> 2) as u32);
    let mut w = [0u32; 80];
    for (i, c) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_le_bytes(c.try_into().unwrap());
    }
    for i in 16..80 {
        w[i] = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i / 20 {
            0 => (!b & d | c & b, 0x5a827999),
            1 => (b ^ c ^ d, 0x6ed9eba1),
            2 => ((b | c) & d | b & c, 0x8f1bbcdc),
            _ => (b ^ c ^ d, 0xca62c1d6),
        };
        let t = rol5(a).wrapping_add(f).wrapping_add(e).wrapping_add(wi).wrapping_add(k);
        e = d;
        d = c;
        c = rol30(b);
        b = a;
        a = t;
    }
    for (x, v) in h.iter_mut().zip([a, b, c, d, e]) {
        *x = x.wrapping_add(v);
    }
}

fn h_bytes(h: &[u32; 5]) -> [u8; 20] {
    let mut out = [0; 20];
    for (o, x) in out.chunks_exact_mut(4).zip(h) {
        o.copy_from_slice(&x.to_le_bytes());
    }
    out
}

/// IDEA multiply mod 65537, 0 standing for 65536.
fn mul(a: u16, b: u16) -> u16 {
    let a = if a == 0 { 0x10000 } else { a as u64 };
    let b = if b == 0 { 0x10000 } else { b as u64 };
    (a * b % 0x10001) as u16
}

/// 0x44e4a0
fn mul_inv(mut x: u32) -> u16 {
    if x < 2 {
        return x as u16;
    }
    let mut t1 = 0x10001 / x;
    let mut y = 0x10001 % x;
    if y == 1 {
        return 1u32.wrapping_sub(t1) as u16;
    }
    let mut t0 = 1u32;
    loop {
        t0 = t0.wrapping_add((x / y).wrapping_mul(t1));
        x %= y;
        if x == 1 {
            return t0 as u16;
        }
        t1 = t1.wrapping_add((y / x).wrapping_mul(t0));
        y %= x;
        if y == 1 {
            return 1u32.wrapping_sub(t1) as u16;
        }
    }
}

/// 0x44e420: expand a 16-byte key into 52 subkeys.
fn expand(key: &[u8]) -> [u16; 52] {
    let mut ek = [0u16; 52];
    for j in 0..8 {
        ek[j] = u16::from_le_bytes([key[2 * j], key[2 * j + 1]]);
    }
    let (mut base, mut i) = (0, 0);
    for _ in 0..44 {
        i += 1;
        ek[base + i + 7] = ek[base + (i & 7)] << 9 | ek[base + ((i + 1) & 7)] >> 7;
        base += i & 8;
        i &= 7;
    }
    ek
}

/// 0x44e780: the decryption schedule.
fn invert(ek: &[u16; 52]) -> [u16; 52] {
    let mut dk = [0u16; 52];
    let mut p = 52;
    let mut e = ek.iter().copied();
    let mut push = |v: u16| {
        p -= 1;
        dk[p] = v;
    };
    let quad = |e: &mut dyn Iterator<Item = u16>, push: &mut dyn FnMut(u16), swap: bool| {
        let t1 = mul_inv(e.next().unwrap() as u32);
        let t2 = e.next().unwrap().wrapping_neg();
        let t3 = e.next().unwrap().wrapping_neg();
        push(mul_inv(e.next().unwrap() as u32));
        if swap {
            push(t2);
            push(t3);
        } else {
            push(t3);
            push(t2);
        }
        push(t1);
    };
    quad(&mut e, &mut push, false);
    for i in 0..8 {
        let t1 = e.next().unwrap();
        push(e.next().unwrap());
        push(t1);
        quad(&mut e, &mut push, i < 7);
    }
    dk
}

/// 0x44e550: one 8-byte block.
fn cipher(input: &[u8], out: &mut [u8], key: &[u16; 52]) {
    let w = |i: usize| u16::from_le_bytes([input[2 * i], input[2 * i + 1]]);
    let (mut x1, mut x2, mut x3, mut x4) = (w(0), w(1), w(2), w(3));
    for k in key[..48].chunks_exact(6) {
        x1 = mul(x1, k[0]);
        x2 = x2.wrapping_add(k[1]);
        x3 = x3.wrapping_add(k[2]);
        x4 = mul(x4, k[3]);
        let s3 = x3;
        x3 = mul(x3 ^ x1, k[4]);
        let s2 = x2;
        x2 = mul((x2 ^ x4).wrapping_add(x3), k[5]);
        x3 = x3.wrapping_add(x2);
        x1 ^= x2;
        x4 ^= x3;
        x2 ^= s3;
        x3 ^= s2;
    }
    let o = [mul(x1, key[48]), x3.wrapping_add(key[49]), x2.wrapping_add(key[50]), mul(x4, key[51])];
    for (c, v) in out.chunks_exact_mut(2).zip(o) {
        c.copy_from_slice(&v.to_le_bytes());
    }
}

/// 0x4816b0 (decrypt mode): the first IDEA schedule and hash state for `key` (up to
/// its NUL).
fn init(key: &[u8]) -> ([u16; 52], [u32; 5]) {
    let mut rng = Rand(0x4fa7);
    let mut r = [0u8; 0x70];
    r.iter_mut().for_each(|b| *b = rng.next());
    let mut k64 = [0u8; 64];
    let mut j = 0;
    for b in k64.iter_mut() {
        if key.get(j).copied().unwrap_or(0) == 0 {
            j = 0;
        }
        *b = key.get(j).copied().unwrap_or(0);
        j += 1;
    }
    let mut h = IV;
    sha_transform(&mut h, &k64);
    let hb = h_bytes(&h);
    for (i, b) in r.iter_mut().enumerate() {
        *b ^= hb[i % 20];
    }
    // Schedules 1 and 2 and their states are made too but decryption only uses the first.
    let mut state = IV;
    sha_transform(&mut state, &r[0x30..0x70]);
    (invert(&expand(&r[..16])), state)
}

/// 0x4818c0: decrypt `data` in place with `key`; returns the plaintext length, or None
/// if the size or the trailer's check value is wrong.
pub fn decrypt(data: &mut [u8], key: &[u8]) -> Option<usize> {
    let len = data.len();
    if len <= 8 || (len - 8) & 0x3f != 0 {
        return None;
    }
    let (schedule, mut state) = init(key);
    let blocks = (len - 8) / 64;
    for j in 0..blocks {
        let block = &mut data[j * 64..j * 64 + 64];
        let hb = h_bytes(&state);
        let mut ks = [0u8; 64];
        if j & 7 == 0 {
            for i in (0..64).step_by(8) {
                cipher(&block[i..i + 8], &mut ks[i..i + 8], &schedule);
            }
        } else {
            ks.copy_from_slice(block);
        }
        for i in 0..64 {
            block[i] = ks[i].wrapping_sub(hb[(0x3f - i) % 20]);
        }
        if j & 0xf == 0 {
            sha_transform(&mut state, block);
        }
    }
    let t = &data[len - 8..];
    (t[4] == 0 && u32::from_le_bytes(t[..4].try_into().unwrap()) == state[0]).then(|| len - 0x48 + t[5] as usize)
}

/// 0x480a40 (EBX = out, `n` = size): n - 1 nonzero bytes of the CRT rand seeded with
/// 0x150b. (The original then reseeds the CRT rand with time(); scbw has no other CRT
/// rand user.)
fn rand_key(n: usize) -> Vec<u8> {
    let mut rng = Rand(0x150b);
    let mut key = Vec::with_capacity(n);
    while key.len() < n - 1 {
        let b = rng.next();
        if b != 0 {
            key.push(b);
        }
    }
    key
}

/// 0x4d9ba0: the font key, decrypted from `font\font.gid` with the key 0x480a40 makes.
pub fn font_key(gid: &[u8]) -> Option<Vec<u8>> {
    let key1 = rand_key(0x14);
    let mut blob = gid.to_vec();
    let n = decrypt(&mut blob, &key1)?;
    (n < blob.len()).then(|| blob[..n].to_vec())
}

#[cfg(test)]
mod tests {
    /// Against the 1.16.1 code run under the emulator: SCBW_GRID_TEST="font.gid enc.fnt dec.fnt".
    #[test]
    fn font_matches_original() {
        let Ok(v) = std::env::var("SCBW_GRID_TEST") else { return };
        let p: Vec<_> = v.split(' ').map(|f| std::fs::read(f).unwrap()).collect();
        let key = super::font_key(&p[0]).unwrap();
        let mut d = p[1].clone();
        let n = super::decrypt(&mut d, &key).unwrap();
        assert_eq!(&d[..n], &p[2][..]);
    }
}
