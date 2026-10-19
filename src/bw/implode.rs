//! PKWare DCL implode (0x410b30 with its helpers 0x410cd0, 0x411050, 0x411390, 0x411490,
//! 0x411510), binary mode with a 1 KB dictionary as the save packer 0x4c3200 calls it.
//! Positions are indices into `work` (the native work buffer at struct +0x27cc).

const LEN_EXTRA: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]; // 0x50dbc8
const LEN_BITS: [u8; 16] = [3, 2, 3, 3, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 7, 7]; // 0x50dbd8
const LEN_CODE: [u8; 16] = [5, 3, 1, 6, 10, 2, 12, 20, 4, 24, 8, 48, 16, 32, 64, 0]; // 0x50dbe8
const DIST_BITS: [u8; 64] = [
    2, 4, 4, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8,
]; // 0x50db48
const DIST_CODE: [u8; 64] = [
    3, 13, 5, 25, 9, 17, 1, 62, 30, 46, 14, 54, 22, 38, 6, 58, 26, 42, 10, 50, 18, 34, 66, 2, 124,
    60, 92, 28, 108, 44, 76, 12, 116, 52, 84, 20, 100, 36, 68, 4, 120, 56, 88, 24, 104, 40, 72, 8,
    240, 112, 176, 48, 208, 80, 144, 16, 224, 96, 160, 32, 192, 64, 128, 0,
]; // 0x50db88

const DSIZE: usize = 0x400;
const DBITS: u32 = 4;
const DMASK: u32 = 0xf;

struct Imp {
    dist: i32, // +0 distance of the last match
    out_bytes: usize,
    out_bits: u32,
    ch_bits: [u8; 0x306],
    ch_code: [u16; 0x306],
    kmp: [i16; 0x208],  // +0x9bc
    ph: [u16; 0x900],   // +0xdc8 hash counts
    out_buff: [u8; 0x802],
    work: [u8; 0x2204 + 8],
    po: [u16; 0x2204],  // +0x49d0 positions by hash
    out: Vec<u8>,
}

impl Imp {
    /// 0x411390: write `n` bits of `code`.
    fn bits(&mut self, mut n: u32, mut code: u32) {
        if n > 8 {
            n -= 8;
            self.bits(8, code);
            code >>= 8;
        }
        let old = self.out_bits;
        let nb = old + n;
        self.out_buff[self.out_bytes] |= (code << old) as u8;
        self.out_bits = nb;
        if nb < 9 {
            self.out_bits = nb & 7;
            if nb & 7 == 0 {
                self.out_bytes += 1;
            }
        } else {
            self.out_bytes += 1;
            self.out_buff[self.out_bytes] = (code >> (8 - old)) as u8;
            self.out_bits &= 7;
        }
        if self.out_bytes > 0x7ff {
            // 0x411490
            self.out.extend_from_slice(&self.out_buff[..0x800]);
            let (a, b) = (self.out_buff[0x800], self.out_buff[self.out_bytes]);
            self.out_bytes -= 0x800;
            self.out_buff.fill(0);
            if self.out_bytes != 0 {
                self.out_buff[0] = a;
            }
            if self.out_bits != 0 {
                self.out_buff[self.out_bytes] = b;
            }
        }
    }

    /// 0x411510: hash the byte pairs of work[start..end] by position.
    fn hash(&mut self, start: usize, end: usize) {
        self.ph.fill(0);
        let w = &self.work;
        let h = |p: usize| 4 * w[p] as usize + 5 * w[p + 1] as usize;
        let mut p = start;
        loop {
            self.ph[h(p)] += 1;
            p += 1;
            if p >= end {
                break;
            }
        }
        let mut s = 0u16;
        for c in self.ph.iter_mut() {
            s = s.wrapping_add(*c);
            *c = s;
        }
        let mut q = end - 1;
        loop {
            let i = h(q);
            self.ph[i] = self.ph[i].wrapping_sub(1);
            self.po[self.ph[i] as usize] = q as u16;
            if q == start {
                break;
            }
            q -= 1;
            if q < start {
                break;
            }
        }
    }

    /// One step of the failure table build (shared by both places in 0x411050).
    fn kmp_step(&mut self, p: usize, i: &mut i16, j: &mut i16) {
        let w = &self.work;
        if w[p + *i as usize] == w[p + *j as usize] || {
            *j = self.kmp[*j as usize];
            *j == -1
        } {
            *i += 1;
            *j += 1;
            self.kmp[*i as usize] = *j;
        }
    }

    /// 0x411050: longest match for work[p..] (length; the distance goes to `dist`).
    fn find(&mut self, p: usize) -> i32 {
        let mut best = 1usize;
        let hi = 4 * self.work[p] as usize + 5 * self.work[p + 1] as usize;
        let mut pi = self.ph[hi] as usize;
        let thr = p + 1 - DSIZE;
        if (self.po[pi] as usize) < thr {
            while (self.po[pi] as usize) < thr {
                pi += 1;
            }
            self.ph[hi] = pi as u16;
        }
        let q = p - 1;
        let mut cand = self.po[pi] as usize;
        if q <= cand {
            return 0;
        }
        loop {
            let w = &self.work;
            if w[cand + best - 1] == w[p + best - 1] && w[cand] == w[p] {
                let mut c = cand + 1;
                let mut n = 2usize;
                let mut pp = p;
                loop {
                    c += 1;
                    if w[c] != w[pp + 2] {
                        break;
                    }
                    n += 1;
                    pp += 1;
                    if n >= 0x204 {
                        break;
                    }
                }
                if best <= n {
                    self.dist = (p + n) as i32 - c as i32 - 1;
                    best = n;
                    if n > 10 {
                        if n == 0x204 {
                            self.dist -= 1;
                            return 0x204;
                        }
                        if q <= self.po[pi + 1] as usize {
                            return n as i32;
                        }
                        let mut j: i16 = 0;
                        self.kmp[0] = -1;
                        let mut i: i16 = 1;
                        self.kmp[1] = 0;
                        while (i as usize) < n {
                            self.kmp_step(p, &mut i, &mut j);
                        }
                        let mut lim = self.po[pi] as usize + n;
                        loop {
                            let mut m = self.kmp[n] as i32;
                            if m == -1 {
                                m = 0;
                            }
                            let mut m = m as usize;
                            loop {
                                pi += 1;
                                cand = self.po[pi] as usize;
                                if q <= cand {
                                    return best as i32;
                                }
                                if cand + m >= lim {
                                    break;
                                }
                            }
                            let w = &self.work;
                            if w[cand + best - 2] == w[p + best - 2] {
                                if cand + m != lim {
                                    m = 0;
                                    lim = cand;
                                }
                            } else {
                                loop {
                                    pi += 1;
                                    cand = self.po[pi] as usize;
                                    if q <= cand {
                                        return best as i32;
                                    }
                                    let w = &self.work;
                                    if w[cand + best - 2] == w[p + best - 2] && w[p] == w[cand] {
                                        break;
                                    }
                                }
                                m = 2;
                                lim = cand + 2;
                            }
                            let w = &self.work;
                            if w[p + m] == w[lim] {
                                loop {
                                    m += 1;
                                    if m > 0x203 {
                                        break;
                                    }
                                    lim += 1;
                                    if w[p + m] != w[lim] {
                                        break;
                                    }
                                }
                            }
                            n = m;
                            if best <= n {
                                self.dist = (p - 1 - cand) as i32;
                                if best < n {
                                    if n == 0x204 {
                                        return 0x204;
                                    }
                                    loop {
                                        self.kmp_step(p, &mut i, &mut j);
                                        best = n;
                                        if (i as usize) >= n {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            pi += 1;
            cand = self.po[pi] as usize;
            if q <= cand {
                return if best < 2 { 0 } else { best as i32 };
            }
        }
    }

    /// 0x410cd0: the compression loop over `src`.
    fn run(&mut self, src: &[u8]) {
        self.out_buff[0] = 0; // ctype: binary
        self.out_buff[1] = DBITS as u8;
        self.out_bytes = 2;
        self.out_bits = 0;
        let mut p = DSIZE + 0x204;
        let mut state = 0;
        let mut pos = 0usize;
        let mut eof = false;
        loop {
            let mut n = 0usize;
            let mut want = 0x1000usize;
            loop {
                let got = want.min(src.len() - pos);
                if got == 0 {
                    if n == 0 && state == 0 {
                        self.end();
                        return;
                    }
                    eof = true;
                    break;
                }
                let d = DSIZE + 0x204 + n;
                self.work[d..d + got].copy_from_slice(&src[pos..pos + got]);
                pos += got;
                n += got;
                want -= got;
                if want == 0 {
                    break;
                }
            }
            let mut pend = DSIZE + n;
            if eof {
                pend += 0x204;
            }
            match state {
                0 => {
                    self.hash(p, pend + 1);
                    state = 1;
                    if DSIZE != 0x1000 {
                        state = 2;
                    }
                }
                1 => {
                    self.hash(p + 0x204 - DSIZE, pend + 1);
                    state = 2;
                }
                _ => self.hash(p - DSIZE, pend + 1),
            }
            while p < pend {
                let mut len = self.find(p) as usize;
                loop {
                    if len == 0 || (len == 2 && self.dist > 0xff) {
                        self.literal(p);
                        p += 1;
                        break;
                    }
                    if eof && pend < p + len {
                        len = pend - p;
                        if len > 1 && (len != 2 || self.dist < 0x100) {
                            self.mat(&mut p, len);
                        } else {
                            self.literal(p);
                            p += 1;
                        }
                        break;
                    }
                    if len > 7 || pend <= p + 1 {
                        self.mat(&mut p, len);
                        break;
                    }
                    let save = self.dist;
                    let l2 = self.find(p + 1) as usize;
                    if l2 <= len || (l2 <= len + 1 && save < 0x81) {
                        self.dist = save;
                        self.mat(&mut p, len);
                        break;
                    }
                    self.literal(p);
                    p += 1;
                    len = l2;
                }
            }
            if eof {
                self.end();
                return;
            }
            p -= 0x1000;
            self.work.copy_within(0x1000..0x1000 + DSIZE + 0x204, 0);
        }
    }

    fn literal(&mut self, p: usize) {
        let b = self.work[p] as usize;
        self.bits(self.ch_bits[b] as u32, self.ch_code[b] as u32);
    }

    /// The match code (length, then distance) at 0x410f6f.
    fn mat(&mut self, p: &mut usize, len: usize) {
        self.bits(self.ch_bits[0x100 + len - 2] as u32, self.ch_code[0x100 + len - 2] as u32);
        let d = self.dist as u32;
        if len == 2 {
            let i = (d >> 2) as usize;
            self.bits(DIST_BITS[i] as u32, DIST_CODE[i] as u32);
            *p += 2;
            self.bits(2, d & 3);
        } else {
            let i = (d >> DBITS) as usize;
            self.bits(DIST_BITS[i] as u32, DIST_CODE[i] as u32);
            *p += len;
            self.bits(DBITS, DMASK & d);
        }
    }

    /// End marker, then the final write.
    fn end(&mut self) {
        self.bits(self.ch_bits[0x305] as u32, self.ch_code[0x305] as u32);
        if self.out_bits != 0 {
            self.out_bytes += 1;
        }
        self.out.extend_from_slice(&self.out_buff[..self.out_bytes]);
    }
}

/// 0x410b30: PKWare implode of `src` (binary, 1 KB dictionary).
pub fn implode(src: &[u8]) -> Vec<u8> {
    let mut s = Box::new(Imp {
        dist: 0,
        out_bytes: 0,
        out_bits: 0,
        ch_bits: [0; 0x306],
        ch_code: [0; 0x306],
        kmp: [0; 0x208],
        ph: [0; 0x900],
        out_buff: [0; 0x802],
        work: [0; 0x2204 + 8],
        po: [0; 0x2204],
        out: Vec::new(),
    });
    for i in 0..0x100 {
        s.ch_bits[i] = 9;
        s.ch_code[i] = (i * 2) as u16;
    }
    let mut k = 0x100;
    for t in 0..16 {
        for j in 0..(1u32 << LEN_EXTRA[t]) {
            s.ch_bits[k] = LEN_EXTRA[t] + LEN_BITS[t] + 1;
            s.ch_code[k] = (j << (LEN_BITS[t] + 1)) as u16 | LEN_CODE[t] as u16 * 2 | 1;
            k += 1;
        }
    }
    s.run(src);
    s.out
}
