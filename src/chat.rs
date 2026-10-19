//! The in-game chat line (rez\<race>textbox.bin, 0x4f37c0) and the single player cheat
//! matcher (0x4b23e0). The cheat hashes and the caret bytes are read from the user's
//! StarCraft.exe at runtime; none of them are stored here.

use crate::dlg::{Ctl, Dialog};

/// File offset of a .rdata address (section VA 0x4fe000, raw 0xfe000).
fn rdata(va: usize) -> usize {
    va - 0x4fe000 + 0xfe000
}

/// 0x4b1ee0's key schedule 0x44e420: the 8 key words expanded to 52 subkeys.
fn key_schedule(key: [u16; 8]) -> [u16; 52] {
    let mut k = [0u16; 52];
    k[..8].copy_from_slice(&key);
    for n in 0..44 {
        let (base, i) = (n / 8 * 8, n % 8);
        k[base + i + 8] = k[base + (i + 2) % 8] >> 7 | k[base + (i + 1) % 8] << 9;
    }
    k
}

/// The multiply of 0x44e550: mod 65537 with 0 as 65536; the result is left unmasked
/// like the original's registers.
fn mul(a: u32, b: u32) -> u32 {
    if a == 0 {
        1u32.wrapping_sub(b)
    } else if b == 0 {
        1u32.wrapping_sub(a)
    } else {
        let p = a.wrapping_mul(b);
        let (lo, hi) = (p & 0xffff, p >> 16);
        ((lo < hi) as u32).wrapping_sub(hi).wrapping_add(lo)
    }
}

/// 0x44e550: encrypt one 8-byte block `w` with the subkeys `k`.
fn encrypt(w: [u16; 4], k: &[u16; 52]) -> [u16; 4] {
    let (mut x0, mut x1, mut x2, mut x3) = (w[0] as u32, w[1] as u32, w[2] as u32, w[3] as u32);
    let mut p = 0;
    for _ in 0..8 {
        let kk = |i: usize| k[p + i] as u32;
        x0 = mul(x0 & 0xffff, kk(0));
        let t4 = x2.wrapping_add(kk(2));
        x3 = mul(x3 & 0xffff, kk(3));
        let m = mul((t4 ^ x0) & 0xffff, kk(4));
        let s = x1.wrapping_add(kk(1));
        let y = (x3 ^ s).wrapping_add(m) & 0xffff;
        let t = mul(y, kk(5));
        x3 ^= m.wrapping_add(t);
        x2 = m.wrapping_add(t) ^ s;
        x0 ^= t;
        x1 = t ^ t4;
        p += 6;
    }
    let kk = |i: usize| k[p + i] as u32;
    [mul(x0 & 0xffff, kk(0)) as u16, kk(1).wrapping_add(x2) as u16, kk(2).wrapping_add(x1) as u16, mul(x3 & 0xffff, kk(3)) as u16]
}

/// 0x4b1ee0: the 64-bit hash of a lowercased line (at most 255 characters).
fn hash(text: &[u8], k: &[u16; 52]) -> [u32; 2] {
    let line: Vec<u8> = text.iter().take(0xff).map(|c| c.to_ascii_lowercase()).collect();
    let mut st = [0u32; 2];
    for blk in line.chunks(8) {
        let mut b = [0u8; 8];
        b[..blk.len()].copy_from_slice(blk);
        let v = [u32::from_le_bytes([b[0], b[1], b[2], b[3]]) ^ st[0], u32::from_le_bytes([b[4], b[5], b[6], b[7]]) ^ st[1]];
        let w = [v[0] as u16, (v[0] >> 16) as u16, v[1] as u16, (v[1] >> 16) as u16];
        let o = encrypt(w, k);
        st = [o[0] as u32 | (o[1] as u32) << 16, o[2] as u32 | (o[3] as u32) << 16];
    }
    st
}

/// 0x51c410 / 0x51c608 / 0x51c4bc: the campaign mission the jump cheat 0x4b1dc0 chose
/// (mission number, race, expansion); the glue's first pass 0x4de200 starts it.
pub struct Jump {
    pub mission: u32,
    pub race: u32,
    pub expansion: u32,
}

static JUMP: std::sync::Mutex<Option<Jump>> = std::sync::Mutex::new(None);

/// The jump 0x4b1dc0 set (0x51bff8 = 1), taken by the glue after the game ended.
pub fn take_jump() -> Option<Jump> {
    JUMP.lock().unwrap().take()
}

/// What 0x4b23e0 did besides toggling `options`.
#[derive(Default)]
pub struct Act {
    /// 0x500b70 (radio free zerg) matched: 0x4b2350 runs 0x4a62b0 when 0x596904 == 3,
    /// 0x6d11e4 (Brood War installed) and 0x57f1e2 == 0 (the local race is zerg).
    pub radio: bool,
    /// 0x4b1dc0 chose a mission (`take_jump`): the game ends (0x51ce90 = 4).
    pub jump: bool,
}

/// 0x4dbc60 (ECX = `rest`, the text after the race name; stack: `race`, `off`, `exp`):
/// the mission number named by "<number><letter>" in the campaign tables 0x5122a0
/// (`exp` 0) / 0x5122ac, read from the install's `exe`: the `number - off`th record
/// without a scenario (+4) and byte +7, then the letter's records further on.
fn campaign_mission(exe: &[u8], rest: &[u8], race: usize, off: u32, exp: u32) -> Option<u32> {
    // .rdata / .data are mapped at file offset = VA - 0x400000.
    let u8_at = |va: u32| exe.get(va as usize - 0x40_0000).copied().unwrap_or(0);
    let u16_at = |va: u32| u16::from_le_bytes([u8_at(va), u8_at(va + 1)]);
    let u32_at = |va: u32| u16_at(va) as u32 | (u16_at(va + 2) as u32) << 16;
    // _strtoul(rest, &end, 10)
    let mut i = rest.iter().take_while(|c| c.is_ascii_whitespace()).count();
    let neg = rest.get(i) == Some(&b'-');
    if matches!(rest.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let digits = rest[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    let (n, end) = if digits == 0 {
        (0u32, 0)
    } else {
        let n = rest[i..i + digits].iter().fold(0u32, |n, &c| n.wrapping_mul(10).wrapping_add((c - b'0') as u32));
        (if neg { n.wrapping_neg() } else { n }, i + digits)
    };
    if (n.wrapping_sub(off) as i32) < 0 {
        return None;
    }
    let mut r = u32_at(if exp == 0 { 0x5122a0 } else { 0x5122ac } + 4 * race as u32);
    let mut left = n.wrapping_sub(off) as i32;
    let mut s = u16_at(r + 2);
    loop {
        if s == 0 {
            return None;
        }
        let mut next = left;
        if u16_at(r + 4) == 0 && u8_at(r + 7) == 0 {
            next = left - 1;
            if left == 0 {
                break;
            }
        }
        s = u16_at(r + 10);
        left = next;
        r += 8;
    }
    let letter = rest.get(end).copied().unwrap_or(0);
    if letter != 0 {
        let mut k = letter.to_ascii_lowercase() as i32 - 0x61;
        while k != 0 {
            k -= 1;
            if u16_at(r + 2) == 0 {
                return None;
            }
            r += 8;
            if u8_at(r + 7) == 0 {
                return None;
            }
        }
    }
    if u16_at(r + 2) == 0 {
        return None;
    }
    Some(u16_at(r + 2) as u32)
}

/// 0x4b23e0: the single player cheat check on `text`; `options` is 0x6d5a6c, `in_game`
/// is 0x596904 == 3, `cheats_on` is 0x596876 == 1 (template +0x11). True when a cheat
/// matched (the game then shows nothing). `act`: what the caller must do (`Act`).
pub fn cheat(text: &[u8], options: &mut u32, in_game: bool, cheats_on: bool, act: &mut Act) -> bool {
    // 0x40c7b0 / 0x40c7bd: MSVC rand seeded 0x75 gives the four key dwords.
    let mut seed = 0x75u32;
    let mut key = [0u16; 8];
    for i in 0..4 {
        seed = seed.wrapping_mul(214013).wrapping_add(2531011);
        let r = seed >> 16 & 0x7fff;
        key[i * 2] = r as u16;
        key[i * 2 + 1] = (r >> 16) as u16;
    }
    let h = hash(text, &key_schedule(key));
    let Ok(exe) = std::fs::read(crate::install_dir().join("StarCraft.exe")) else { return false };
    let pair = |va: usize| -> Option<[u32; 2]> {
        let b = exe.get(rdata(va)..rdata(va) + 8)?;
        Some([u32::from_le_bytes(b[..4].try_into().ok()?), u32::from_le_bytes(b[4..].try_into().ok()?)])
    };
    if in_game && !cheats_on {
        return false;
    }
    // 0x4b1b50: (hash pair address, option bit toggled), in its order.
    const BITS: [(usize, u32); 16] = [
        (0x500b78, 0x200), (0x500b10, 0x10), (0x500b18, 0x40000), (0x500b20, 0x80000),
        (0x500b28, 8), (0x500b48, 0x1000), (0x500b50, 0x2000), (0x500b60, 0x20000),
        (0x500b68, 0x800), (0x500b30, 1), (0x500b38, 2), (0x500b08, 4),
        (0x500b58, 0x4000), (0x500af8, 0x80), (0x500b00, 0x40), (0x500b40, 0x100),
    ];
    for (va, bit) in BITS {
        if pair(va) == Some(h) {
            *options ^= bit;
            return true;
        }
    }
    // 0x4b2350
    if pair(0x500b80) == Some(h) {
        *options ^= 0x20000000;
        return true;
    }
    if pair(0x500b70) == Some(h) {
        act.radio = true;
        return true;
    }
    // 0x4b1dc0 (single player, option 0x200): "terran12b" and the like, by the 6 records of
    // 0x503580 (name, race, number offset, expansion), compared with the start of the text.
    if *options & 0x200 == 0 {
        return false;
    }
    let u32_at = |va: usize| exe.get(rdata(va)..rdata(va) + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    for k in 0..6 {
        let rec = 0x503580 + k * 16;
        let (Some(name), Some(race), Some(off), Some(exp)) = (u32_at(rec), u32_at(rec + 4), u32_at(rec + 8), u32_at(rec + 12)) else { return false };
        let name = &exe[rdata(name as usize)..];
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(0)];
        if !text.get(..name.len()).is_some_and(|t| t.eq_ignore_ascii_case(name)) {
            continue;
        }
        let rest = &text[name.len()..];
        let rest = &rest[..rest.iter().position(|&c| c == 0).unwrap_or(rest.len())];
        if let Some(mission) = campaign_mission(&exe, rest, race as usize, off, exp).filter(|&m| m != 0x3f) {
            *JUMP.lock().unwrap() = Some(Jump { mission, race, expansion: exp });
            act.jump = in_game;
        }
        return true;
    }
    false
}

/// The chat line's state: the textbox dialog and its edit control (0x4f3540, 0x416980).
#[derive(Default)]
pub struct Chat {
    pub open: bool,
    dlg: Option<Dialog>,
    text: Vec<u8>,
    caret: usize,
    /// 0x36 & 0x10: Insert shifts the text instead of overwriting.
    insert: bool,
    /// The caret bytes 0x50612c..0x50612e read from the exe.
    caret_bytes: [u8; 3],
    opened_ms: u32,
}

/// What a key did.
pub enum Done {
    Nothing,
    /// 0x4f32d0(edit, 1): the submitted line.
    Submit(Vec<u8>),
}

impl Chat {
    /// 0x4f32d0 (closed): open the line; single player mode 1 ("Message:").
    pub fn open(&mut self, dlg: Dialog, now: u32) {
        self.dlg = Some(dlg);
        self.open = true;
        self.text.clear();
        self.caret = 0;
        self.opened_ms = now;
        self.caret_bytes = std::fs::read(crate::install_dir().join("StarCraft.exe")).ok()
            .and_then(|e| e.get(rdata(0x50612c)..rdata(0x50612c) + 3).map(|b| [b[0], b[1], b[2]]))
            .unwrap_or([b'_', 0, 0]);
    }

    /// Creation of the textbox dialog 0x68c140 at game start: kept loaded so the
    /// prompt (control 5) can be drawn while the line is closed.
    pub fn load(&mut self, dlg: Dialog) {
        self.dlg = Some(dlg);
    }

    /// 0x4f31d0: the placement / targeting prompt, control 5 of the textbox dialog 0x68c140
    /// with its text set (stat_txt 0x3e targeting / 0x3f placement; shown only while the line
    /// is closed, 0x68c144 == 0). Drawn like the label in `draw` (`up`: replays 24 px higher).
    pub fn draw_prompt(&self, skin: &crate::dlg::Skin, text: &[u8], up: bool, out: &mut [u8], w: i32, h: i32) {
        let Some(d) = self.dlg.as_ref().filter(|_| !self.open) else { return };
        let Some(c) = d.get(5) else { return };
        let dy = if up { -24 } else { 0 };
        let (ox, oy) = d.origin();
        let mut s = crate::dlg::Surf { px: out, w, h };
        let r = (ox + c.rect.0, oy + c.rect.1 + dy, ox + c.rect.2, oy + c.rect.3 + dy);
        skin.ctl_text(c, text, r, 0, 0, 0x11, &mut s);
    }

    /// 0x4f32d0 (open): close; the text is cleared (0x501b7d is "").
    pub fn close(&mut self) {
        self.open = false;
        self.text.clear();
        self.caret = 0;
    }

    /// 0x4f36fa: Enter submits, Esc cancels.
    pub fn enter(&mut self) -> Done {
        let t = std::mem::take(&mut self.text);
        self.close();
        Done::Submit(t)
    }

    /// 0x4f3240: a typed character; at most 78 characters (len > 0x4d rejects).
    /// 0x416710 inserts it at the caret.
    pub fn char(&mut self, c: u8) {
        if c < 0x20 || self.text.len() > 0x4d {
            return;
        }
        if self.caret == self.text.len() {
            self.text.push(c);
        } else if self.insert {
            self.text.insert(self.caret, c);
        } else {
            self.text[self.caret] = c;
        }
        self.caret += 1;
    }

    /// 0x416980: Backspace, Delete, Left, Right, Home, End and Insert.
    pub fn key(&mut self, k: minifb::Key) {
        use minifb::Key::*;
        match k {
            Backspace if self.caret > 0 => {
                self.caret -= 1;
                self.text.remove(self.caret);
            }
            Delete if self.caret < self.text.len() => {
                self.text.remove(self.caret);
            }
            Left => self.caret = self.caret.saturating_sub(1),
            Right => self.caret = (self.caret + 1).min(self.text.len()),
            Home => self.caret = 0,
            End => self.caret = self.text.len(),
            Insert => self.insert = !self.insert,
            _ => {}
        }
    }

    /// 0x4f3040: the dialog's label and the edit text with the blinking caret
    /// (500 ms, 0x416600). `up`: replays draw the box 24 px higher.
    /// 0x4167f0 is the edit control's part of the draw.
    pub fn draw(&self, skin: &crate::dlg::Skin, now: u32, up: bool, out: &mut [u8], w: i32, h: i32) {
        let Some(d) = self.dlg.as_ref().filter(|_| self.open) else { return };
        let dy = if up { -24 } else { 0 };
        let (ox, oy) = d.origin();
        let mut s = crate::dlg::Surf { px: out, w, h };
        let Some(label) = d.get(1) else { return };
        let lr = (ox + label.rect.0, oy + label.rect.1 + dy, ox + label.rect.0 + 279, oy + label.rect.3 + dy);
        let Some(edit) = d.get(6) else { return };
        let lw = skin.fonts[1].width_bytes(&label.text);
        let mut l: Ctl = label.clone();
        l.flags |= 0;
        skin.ctl_text(&l, &label.text, lr, 0, 0, 0x11, &mut s);
        // 0x4f3040: the edit starts after the label and one glyph gap.
        let left = ox + lw + 1;
        let mut line = self.text.clone();
        if (now.wrapping_sub(self.opened_ms) / 500) % 2 == 0 {
            let at = self.caret.min(line.len());
            line.splice(at..at, self.caret_bytes);
        }
        let er = (left, oy + edit.rect.1 + dy, ox + edit.rect.2, oy + edit.rect.3 + dy);
        skin.ctl_text(edit, &line, er, 0, 0, 0x11, &mut s);
    }
}

#[cfg(test)]
mod tests {
    /// 0x4dbc60 on the install's tables: "terran1", "zerg2b", "xprotoss3", bad numbers.
    #[test]
    fn campaign_jump_lookup() {
        let Ok(exe) = std::fs::read(crate::install_dir().join("StarCraft.exe")) else { return };
        let m = |rest: &str, race, off, exp| super::campaign_mission(&exe, rest.as_bytes(), race, off, exp);
        assert!(m("1", 1, 0, 0).is_some());
        assert!(m("99", 1, 0, 0).is_none());
        assert!(m("1", 0, 1, 0).is_some());
        assert!(m("1", 2, 1, 1).is_some());
        assert!(m("1a", 1, 0, 0) == m("1", 1, 0, 0));
        eprintln!("{:?} {:?} {:?}", m("1", 1, 0, 0), m("2", 1, 0, 0), m("1b", 1, 0, 0));
    }
}
