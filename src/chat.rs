//! The in-game chat line (rez\<race>textbox.bin, 0x4f37c0) and the single player cheat
//! matcher (0x4b23e0). The cheat hashes and the caret bytes are read from the user's
//! StarCraft.exe at runtime; none of them are stored here.

use crate::dlg::{Ctl, Dialog};
use crate::todo_port;

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

/// 0x4b23e0: the single player cheat check on `text`; `options` is 0x6d5a6c, `in_game`
/// is 0x596904 == 3, `cheats_on` is 0x596876 == 1 (template +0x11). True when a cheat
/// matched (the game then shows nothing).
pub fn cheat(text: &[u8], options: &mut u32, in_game: bool, cheats_on: bool) -> bool {
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
        todo_port!("0x4a62b0", "cheat: expansion campaign jump when 0x596904 == 3 && 0x6d11e4 && !0x57f1e2");
        return true;
    }
    todo_port!("0x4b1dc0", "cheat: campaign mission jump (0x200)");
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
