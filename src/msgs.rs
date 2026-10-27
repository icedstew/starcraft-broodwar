//! The in-game text lines (0x640b60): 11 message lines above the console, shown oldest
//! first, a right-aligned line at the top (slot 11, 0x6414be) and the centered error line
//! (slot 12, 0x641598). Each line has a color code (0x641674), flags (0x641684; 1 = a
//! trigger's text) and the tick it expires (0x640b24).
//!
//! The game times them with GetTickCount; scbw passes its own millisecond clock.

use crate::ui::{Font, TextDst, TextOpts};

const LINES: usize = 11;
/// Each line's buffer is 0xda bytes, NUL included.
const LINE_MAX: usize = 0xd9;

#[derive(Clone, Default)]
pub struct Messages {
    text: [Vec<u8>; 13],
    color: [u8; 13],
    flags: [u8; 13],
    expire: [u32; 13],
    /// 0x640b58: the next line written (the oldest shown).
    next: usize,
    /// 0x64096c: font10's height plus the 3 px line gap.
    line_h: i32,
    /// 0x640b20: the line pitch, stretched to fill 0xb7 rows.
    pitch: i32,
}

impl Messages {
    /// Whether 0x48ce90 has run.
    pub fn ready(&self) -> bool {
        self.line_h != 0
    }

    /// 0x48ce90
    pub fn reset(&mut self, font10: &Font) {
        let h = font10.height + 3;
        self.line_h = h;
        self.pitch = h;
        if h * 11 < 0xb7 {
            self.pitch = h + (0xb7 - h * 11) / 11;
        }
        self.next = 10;
    }

    /// 0x48cd30: add `s` as message lines, word wrapped to 0x26c pixels (a word wider
    /// than that breaks where it overflows), each in color code `color` until `expire`.
    pub fn add(&mut self, font10: &Font, s: &[u8], color: u8, expire: u32, flags: u8) {
        let o = TextOpts { clip: (0, 0, 0x26c, 0x7fff), ..TextOpts::new() };
        let mut i = 0;
        while s.get(i).is_some_and(|&c| c != 0) {
            let mut end = font10.wrap(s, i, 0, &o, true);
            if end == i {
                end = font10.wrap(s, i, 0, &o, false);
            }
            // ponytail: a glyph wider than the line would loop forever in 1.16.1.
            let end = end.max(i + 1);
            let k = self.next;
            self.next = if self.next + 1 > 10 { 0 } else { self.next + 1 };
            self.color[k] = color;
            self.flags[k] = flags;
            self.expire[k] = expire;
            self.text[k] = s[i..end.min(i + LINE_MAX)].to_vec();
            i = end;
        }
    }

    /// 0x48d0c0: a message for `ms` milliseconds (7000 when 0) in color 3 and sound 0x17
    /// (Transmission) in `sounds`; 0x48d140 is the same for 7 s with sound 2.
    pub fn print(&mut self, font10: &Font, s: &[u8], now: u32, ms: u32, sound: u16, sounds: &mut Vec<u16>) {
        let ms = if ms == 0 { 7000 } else { ms };
        self.add(font10, s, 3, now.wrapping_add(ms), 0);
        sounds.push(sound);
    }

    /// 0x48cf20: a trigger's text in color 2 for `ms` (cleared by 0x48ccf0).
    pub fn trigger_text(&mut self, font10: &Font, s: &[u8], now: u32, ms: u32) {
        self.add(font10, s, 2, now.wrapping_add(ms), 1);
    }

    /// 0x48ccb0: the error line, color 4, for 7 seconds.
    pub fn error(&mut self, s: &[u8], now: u32) {
        self.text[12] = s[..s.len().min(LINE_MAX)].to_vec();
        self.color[12] = 4;
        self.expire[12] = now.wrapping_add(7000);
    }

    /// 0x48cc70: lines past their time go blank.
    pub fn expire(&mut self, now: u32) {
        for k in 0..13 {
            if self.text[k].first().is_some_and(|&c| c != 0) && now.wrapping_sub(self.expire[k]) as i32 >= 0 {
                self.text[k].clear();
            }
        }
    }

    /// 0x48ccf0: the triggers' lines go blank (skipping a cinematic, 0x489da0).
    pub fn clear_trigger_text(&mut self) {
        for k in 0..LINES {
            if self.flags[k] & 1 != 0 {
                self.text[k].clear();
            }
        }
    }

    /// 0x48cf60: the lines in font10, from y 0x70 down (the oldest first), then the
    /// error line centered at 0x127 and slot 11 right-aligned at the top.
    /// 0x48cb80 (a changed line marks its screen rows for redraw) is this too: scbw
    /// redraws the whole screen every frame.
    pub fn draw(&self, font10: &Font, dst: TextDst) {
        let TextDst { rows, out, w, h } = dst;
        let mut line = |k: usize, flags: u8, l: i32, r: i32, y: i32| {
            if self.text[k].first().is_some_and(|&c| c != 0) {
                let o = TextOpts { flags, clip: (l, y, r, y + self.line_h), ..TextOpts::new() };
                font10.draw_bytes(&self.text[k], l, y, self.color[k], &o, TextDst { rows, out: &mut *out, w, h });
            }
        };
        let mut k = self.next;
        let mut y = 0x70;
        for _ in 0..LINES {
            line(k, 0x11, 10, 0x276, y);
            y += self.pitch;
            k = (k + 1) % LINES;
        }
        line(12, 0x12, 10, 0x276, 0x127);
        line(11, 0x14, 0x1a4, 0x26c, 0x18);
    }
}

/// 0x4c51d0: how long a trigger's text stays up: 50 ms a character, at least 4 s.
pub fn reading_ms(s: &[u8]) -> u32 {
    (s.len() as u32 * 50).max(4000)
}

/// 0x48d1c0: the chat line for `text` (NUL-terminated) from slot `p` and its color code:
/// "name:<color> text" (the sender's name keeps only characters >= 0x20 and \n \r \t,
/// at most 0x50), "[tbl]name:<color> text" for a sender of player type 9 (`tbl` is
/// network.tbl string 0x1b); color 7 for the local player else 2; slots >= 8 show the
/// text as is. The line is cut to 0xff bytes (the 0x100-byte buffer).
// ponytail: the 0x6cdfec & 0x1000 team-color choice (0x49b0e0, table 0x581dd6) and the
// 0x484f50 team-name "[%s]" form (0x4850f0) are never set here; add with team games.
pub fn chat_line(p: u8, text: &[u8], local: u8, ptype: u8, name: &str, tbl: &str) -> (Vec<u8>, u8) {
    let text = &text[..text.iter().position(|&c| c == 0).unwrap_or(text.len())];
    let mut o = Vec::new();
    let mut color = 2;
    // Ours: a human past 8 (sharing a slot) is named as the slot players are.
    if p < 8 || ptype == 2 {
        let color_c = if p == local { 7 } else { 2 };
        color = color_c;
        let nm: Vec<u8> = name.bytes().take_while(|&c| c != 0).filter(|&c| c > 0x1f || matches!(c, 9 | 10 | 13)).take(0x50).collect();
        if ptype == 9 {
            o.extend_from_slice(b"[");
            o.extend_from_slice(tbl.as_bytes());
            o.extend_from_slice(b"]");
        }
        o.extend_from_slice(&nm);
        o.extend_from_slice(&[b':', color_c, b' ']);
    }
    o.extend_from_slice(text);
    o.truncate(0xff);
    (o, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_order_and_expiry() {
        let mut m = Messages { line_h: 13, pitch: 16, next: 10, ..Default::default() };
        // A font without glyphs: every character is 0 wide, each text one line.
        let f = Font::parse(vec![0, 0, 0, 0, 1, 0, 8, 10]);
        m.add(&f, b"a", 3, 100, 0);
        m.add(&f, b"b", 3, 200, 1);
        assert_eq!((m.text[10].as_slice(), m.text[0].as_slice(), m.next), (&b"a"[..], &b"b"[..], 1));
        m.expire(150);
        assert!(m.text[10].is_empty() && !m.text[0].is_empty());
        m.clear_trigger_text();
        assert!(m.text[0].is_empty());
    }
}
