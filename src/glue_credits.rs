//! The credits and mission text screens (rez\credits.bin on a rez\<name>.txt, 0x4d8df0):
//! the text file is a stream of tags (`</BACKGROUND pcx>`, `</FONTCOLOR pcx>`,
//! `</FADESPEED n>`, `</DISPLAYTIME ms>`, the six `</SCREEN...>` areas, `</PAGE>`) and of
//! text for the area last named; a page is shown when its `</PAGE>` is read (the palette
//! fades out and in when the page has a background) and the next one follows after the
//! display time, a key (Space, Enter) or a click; Escape ends it.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. The native
//! functions block (0x41a080, the fades); here the stages of a page are timed by
//! `credits_tick`.

use super::{state, screen, AudioCmd, Fade, Glue, Open};
use crate::dlg::{flag, Dialog, Update};
use crate::palette::{self, Pal};
use crate::storage::Storage;
use std::collections::VecDeque;

/// 0x4fff24 (8 bytes: control id, tag pointer): the six text areas of credits.bin.
const AREAS: [(i16, &[u8]); 6] = [
    (1, b"</SCREENLEFT>"),
    (2, b"</SCREENRIGHT>"),
    (3, b"</SCREENTOP>"),
    (4, b"</SCREENBOTTOM>"),
    (5, b"</SCREENCENTER>"),
    (6, b"</SCREENLOWERLEFT>"),
];

/// 0x4fff53 + id (0x4d8930): the text flags an area's label is drawn with (left, right,
/// centered; 0x20 centered vertically, 0x40 lower); another id 0x11.
fn area_flags(id: i16) -> u8 {
    match id as u16 {
        0 => 0,
        1 => 0x21,
        2 => 0x24,
        3..=5 => 0x22,
        6 => 0x41,
        _ => 0x11,
    }
}

/// What the screen's end goes on with.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Then {
    /// 0x4d8fd0 / 0x4d90c0: the music fades out and the top loop goes back to the glue.
    #[default]
    Top,
    /// 0x4f71f0 / 0x4f7550 / 0x4f78b0: the ready room (race) opens.
    Ready(u8),
    /// 0x4dc320: the campaign goes on.
    CampaignNext,
}

/// Where a page is in its show (milliseconds left, or elapsed for `Wait`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Idle,
    /// 0x41e9e0: the old page fades out.
    Out(u32),
    /// 0x41ea30: the new one fades in.
    In(u32),
    /// 0x4d8cd0: the display time since 0x51cec4 was set.
    Wait(u32),
}

/// The screen's globals (0x51cea4 ..).
pub(super) struct Credits {
    /// A credits run of the top loop (state 8 / 9) is going.
    pub(super) running: bool,
    /// 0x51cea8: the text file; 0x51cebc / 0x51ceb8: the read position (the rest is
    /// `text.len() - pos`).
    text: Vec<u8>,
    pos: usize,
    /// 0x51ceb0: the area (control id) the text goes to.
    sel: Option<i16>,
    /// 0x51cec8: the fade steps (1).
    fade_speed: u8,
    /// 0x51ceb4: the display time in ms (5000).
    display: u32,
    /// 0x51ceac: the page has a background.
    bg: bool,
    /// 0x5994e0: its palette; 0x6ceb40: the palette shown.
    new_pal: Pal,
    pal: Pal,
    /// 0x51cec0: Escape was pressed.
    esc: bool,
    stage: Stage,
    /// The dialog as it was before the page being read: shown while the old page fades out.
    snap: Option<Dialog>,
    old: Option<Dialog>,
    /// The texts still to show, with whether Escape skips them (0x51cec0).
    queue: VecDeque<(String, bool)>,
    then: Then,
    /// tFont.pcx's rows (0x6ce000) of the look `.0` before `</FONTCOLOR>` changed them.
    rows: Option<(usize, Vec<u8>)>,
}

impl Default for Credits {
    fn default() -> Credits {
        Credits {
            running: false,
            text: Vec::new(),
            pos: 0,
            sel: None,
            fade_speed: 1,
            display: 5000,
            bg: false,
            new_pal: [[0; 3]; 256],
            pal: [[0; 3]; 256],
            esc: false,
            stage: Stage::Idle,
            snap: None,
            old: None,
            queue: VecDeque::new(),
            then: Then::Top,
            rows: None,
        }
    }
}

impl Credits {
    /// The palette the dialog is shown in.
    pub(super) fn palette(&self) -> &Pal {
        &self.pal
    }

    /// The dialog to draw instead of the screen's while the old page fades out.
    pub(super) fn old(&self) -> Option<&Dialog> {
        self.old.as_ref()
    }

    /// 0x4d86a0: the next token of the text: (is a tag, start, length); a length of 0 is
    /// the end. A tag is `<` .. `>` (a second `<` or a lone `>` ends the reading); text runs
    /// to the next `<` and is dropped when it is only white space.
    fn token(&mut self) -> (bool, usize, usize) {
        let (t, n) = (&self.text, self.text.len());
        let mut p = self.pos;
        let (mut tag, mut start, mut len, mut blank) = (false, p, 0usize, true);
        if p >= n {
            return (false, start, 0);
        }
        loop {
            match t[p] {
                b'<' => {
                    if len != 0 && !blank {
                        self.pos = p;
                        return (tag, start, len);
                    }
                    start = p;
                    len = 0;
                    if tag {
                        self.pos = p;
                        return (tag, start, 0);
                    }
                    tag = true;
                }
                b'>' => {
                    if !tag {
                        self.pos = p;
                        return (tag, start, 0);
                    }
                    self.pos = p + 1;
                    return (tag, start, len + 1);
                }
                b'\n' | b'\r' | b'\t' | b' ' | 0x1a => {}
                _ => blank = false,
            }
            p += 1;
            len += 1;
            if p >= n {
                break;
            }
        }
        self.pos = p;
        if tag || blank { (tag, start, 0) } else { (tag, start, len) }
    }
}

/// _strnicmp(a, b, n) == 0 (a NUL ends both).
fn nicmp(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0).to_ascii_lowercase(), b.get(i).copied().unwrap_or(0).to_ascii_lowercase());
        if x != y {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// The argument of a tag: the bytes up to the closing `>`.
fn arg(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == b'>').unwrap_or(s.len())]
}

/// _strtoul(s, &end, 10): white space, a sign, decimal digits (wrapping; a minus negates).
fn strtoul(s: &[u8]) -> u32 {
    let s = s.trim_ascii_start();
    let (neg, s) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let v = s.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |v, &c| v.wrapping_mul(10).wrapping_add((c - b'0') as u32));
    if neg { v.wrapping_neg() } else { v }
}

impl Glue {
    /// The credits dialog.
    fn crdlg(&mut self) -> Option<&mut Dialog> {
        match self.dlg.as_mut() {
            Some((d, Open::Credits)) => Some(d),
            _ => None,
        }
    }

    /// Whether the credits dialog is the open one.
    pub(super) fn credits_open_now(&self) -> bool {
        matches!(self.dlg, Some((_, Open::Credits)))
    }

    /// 0x4d8fd0 (state 8; `exp` false) / 0x4d90c0 (state 9; `exp` true): the top loop's
    /// credits. The glue loop's end (0x4de200) has stopped the sounds, faded the music and
    /// blanked the palette; then the music (0x4a6310: track 5 / 9) and the texts: state 8
    /// the Brood War credits (crdt_exp, when installed, 0x6d11e4) and the credits (crdt_lst,
    /// not after Escape); state 9 the epilog (epilog, epilogX with Brood War 0x58f440) and
    /// the credits. The music fades out (0x4a5f50) at the end.
    pub(super) fn credits_start(&mut self, files: &Storage, exp: bool) {
        self.cmds.push(AudioCmd::StopSounds);
        self.music_fade_out();
        if !self.pal_blank {
            self.pal_blank = true;
            self.fade = None;
        }
        self.cmds.push(AudioCmd::CreditsVolume(true));
        self.music_start(if exp { 9 } else { 5 });
        let cr = &mut self.credits;
        cr.running = true;
        cr.esc = false;
        cr.then = Then::Top;
        cr.queue.clear();
        if exp {
            let name = |bw: bool, a: &str, b: &str| (if bw { a } else { b }).to_string();
            cr.queue.push_back((name(self.expansion, "epilogX", "epilog"), false));
            cr.queue.push_back((name(self.expansion, "crdt_exp", "crdt_lst"), false));
        } else {
            if self.expansion_installed {
                cr.queue.push_back(("crdt_exp".into(), false));
            }
            cr.queue.push_back(("crdt_lst".into(), true));
        }
        self.credits_next(files);
    }

    /// 0x4d9200 / 0x4d91b0 -> 0x4d8df0 (rez\<name>.txt): a mission's text screen; `then` is
    /// what the caller does when it ends.
    pub(super) fn credits_mission(&mut self, files: &Storage, name: &str, then: Then) {
        let cr = &mut self.credits;
        cr.then = then;
        cr.queue.clear();
        cr.running = false;
        self.credits_open(files, name);
    }

    /// The next text of the queue, or the end of the screen.
    fn credits_next(&mut self, files: &Storage) {
        let cr = &mut self.credits;
        if let Some((name, skip)) = cr.queue.pop_front() {
            if !(skip && cr.esc) {
                self.credits_open(files, &name);
                return;
            }
            cr.queue.clear();
        }
        self.credits_finish(files);
    }

    /// The end of 0x4d8fd0 / 0x4d90c0 (the music fades out, back to the glue's main menu)
    /// or of a mission text (the caller goes on). The text colors are the glue's again.
    fn credits_finish(&mut self, files: &Storage) {
        let cr = &mut self.credits;
        let then = cr.then;
        cr.running = false;
        if let Some((p, rows)) = cr.rows.take() {
            if let Some(l) = self.looks[p].as_mut() {
                l.skin.text_rows = rows;
            }
        }
        // The next screen shows its palette (0x4de150) from the blank one.
        self.shown = -1;
        match then {
            Then::Top => {
                self.music_fade_out();
                self.cmds.push(AudioCmd::CreditsVolume(false));
                self.state = state::GLUE;
                self.screen = screen::MAIN;
            }
            Then::Ready(race) => self.open_ready_dialog(files, race),
            Then::CampaignNext => {
                self.campaign_next_tail(files);
            }
        }
    }

    /// 0x4d8df0 (up to 0x41a080): reads rez\<name>.txt and opens rez\credits.bin; its event
    /// proc 0x4d8d60 starts the first page (code 0).
    fn credits_open(&mut self, files: &Storage, name: &str) {
        let path = format!("rez/{name}.txt");
        let text = files.read(&path).unwrap_or_default();
        if text.is_empty() {
            crate::bw::fatal_error(&path);
        }
        let cr = &mut self.credits;
        cr.text = text;
        cr.pos = 0;
        cr.sel = None;
        cr.stage = Stage::Idle;
        let Ok(d) = files.read("rez/credits.bin") else { crate::bw::fatal_error("rez\\credits.bin") };
        let mut dlg = Dialog::parse(&d);
        dlg.this.flags |= 4;
        dlg.bind(files);
        let p = self.palette_id();
        self.look(files, p);
        self.dlg = Some((dlg, Open::Credits));
        self.credits_init(files);
    }

    /// 0x4d8c60 (event 0xe, code 0): the six areas lose their text and are drawn by 0x4d8930;
    /// fade steps 1, display time 5000 ms; the first page.
    fn credits_init(&mut self, files: &Storage) {
        if let Some(dlg) = self.crdlg() {
            for (id, _) in AREAS.iter().rev() {
                if let Some(c) = dlg.get_mut(*id) {
                    c.text.clear();
                    c.update = Update::Credits(area_flags(*id));
                }
            }
        }
        let cr = &mut self.credits;
        cr.fade_speed = 1;
        cr.display = 5000;
        self.credits_page(files);
    }

    /// 0x4d8bd0: reads tokens until a page ends (`</PAGE>`: true; its display time then
    /// runs); the end of the text closes the dialog (false). Text goes to the area last
    /// named (0x4d8840); tags are 0x4d8a80.
    fn credits_page(&mut self, files: &Storage) -> bool {
        self.credits.snap = self.crdlg().cloned();
        if self.credits.text.len() == self.credits.pos {
            self.credits_close(files);
            return false;
        }
        loop {
            let (tag, start, len) = self.credits.token();
            if len == 0 {
                self.credits_close(files);
                return false;
            }
            let tok = self.credits.text[start..start + len].to_vec();
            if !tag {
                self.credits_text(&tok);
            } else if self.credits_tag(files, &tok) {
                return true;
            }
        }
    }

    /// 0x4d8840: the text of the area (`sel`): the old text goes (the label hides, 0x418700),
    /// the new one is shown (0x4186a0).
    fn credits_text(&mut self, text: &[u8]) {
        let Some(id) = self.credits.sel else { return };
        let Some(c) = self.crdlg().and_then(|d| d.get_mut(id)) else { return };
        if !c.text.is_empty() {
            c.text.clear();
            c.flags &= !flag::VISIBLE;
        }
        if !text.is_empty() {
            c.text = text.to_vec();
            c.flags |= flag::VISIBLE;
        }
    }

    /// 0x4d8a80 (`tok` the tag): true when it ended the page.
    fn credits_tag(&mut self, files: &Storage, tok: &[u8]) -> bool {
        let len = tok.len();
        let is = |tag: &[u8], n: usize| nicmp(tok, tag, n);
        if len > 0xe && is(b"</BACKGROUND ", 0xd) {
            self.credits_background(files, arg(&tok[0xd..]));
            return false;
        }
        if len > 0xd {
            if is(b"</FONTCOLOR ", 0xc) {
                self.credits_font_color(files, arg(&tok[0xc..]));
                return false;
            }
            if is(b"</FADESPEED ", 0xc) {
                // 0x4d8660
                let v = strtoul(arg(&tok[0xc..]));
                if v != 0 {
                    self.credits.fade_speed = v as u8;
                }
                return false;
            }
        }
        if len > 0xf && is(b"</DISPLAYTIME ", 0xe) {
            // 0x4d8620
            let v = strtoul(arg(&tok[0xe..]));
            if v != 0 {
                self.credits.display = v;
            }
            return false;
        }
        if is(b"</PAGE>", len) {
            self.credits_page_end();
            return true;
        }
        for (id, tag) in AREAS.iter().rev() {
            if is(tag, len) {
                // The old area hides (0x418700); the new one is the dialog's control.
                let old = self.credits.sel;
                let dlg = self.crdlg();
                if let (Some(dlg), Some(old)) = (dlg, old) {
                    if let Some(c) = dlg.get_mut(old) {
                        c.flags &= !flag::VISIBLE;
                    }
                }
                let found = self.crdlg().and_then(|d| d.get(*id)).map(|c| c.id);
                self.credits.sel = found;
                return false;
            }
        }
        false
    }

    /// 0x4d8970: `</BACKGROUND name>`: the page's picture (the dialog's bitmap) and palette.
    fn credits_background(&mut self, files: &Storage, name: &[u8]) {
        let path = String::from_utf8_lossy(name).into_owned();
        let Ok(d) = files.read(&path) else { crate::bw::fatal_error(&path) };
        let (w, h, px) = crate::view::pcx(&d);
        self.credits.new_pal = super::pcx_palette(&d);
        self.credits.bg = true;
        if let Some(dlg) = self.crdlg() {
            dlg.bg = Some((w as i32, h as i32, px));
        }
    }

    /// 0x4d8a10: `</FONTCOLOR name>`: the 0xc0 bytes of the picture are the text color rows
    /// (0x6ce000).
    fn credits_font_color(&mut self, files: &Storage, name: &[u8]) {
        let path = String::from_utf8_lossy(name).into_owned();
        let Ok(d) = files.read(&path) else { crate::bw::fatal_error(&path) };
        let px = crate::view::pcx(&d).2;
        let p = self.palette_id();
        let Some(look) = self.looks[p].as_mut() else { return };
        let rows = &mut look.skin.text_rows;
        let saved = rows.clone();
        for (r, v) in rows.iter_mut().zip(px.iter().take(0xc0)) {
            *r = *v;
        }
        self.credits.rows.get_or_insert((p, saved));
    }

    /// 0x4d88d0 (`</PAGE>`): a page with a background (0x51ceac) fades the old one out
    /// (0x41e9e0), takes the new palette and fades in (0x41ea30); the display time starts
    /// after that.
    fn credits_page_end(&mut self) {
        let cr = &mut self.credits;
        if !cr.bg {
            cr.stage = Stage::Wait(0);
            return;
        }
        cr.bg = false;
        if self.pal_blank {
            self.credits_swap();
            return;
        }
        let steps = palette::fade_steps(cr.fade_speed as u32);
        let ms = steps.len() as u32 * 10;
        cr.old = cr.snap.take();
        cr.stage = Stage::Out(ms);
        self.pal_blank = true;
        self.fade = Some(Fade { from: cr.pal, to: Some([[0; 3]; 256]), ms: 0, steps });
    }

    /// 0x4d88d0 after the fade out: the page's palette (0x5994e0 -> 0x6ceb40), the dialog
    /// redrawn (0x41c400) and the fade in.
    fn credits_swap(&mut self) {
        let cr = &mut self.credits;
        cr.old = None;
        cr.pal = cr.new_pal;
        let steps = palette::fade_steps(cr.fade_speed as u32);
        cr.stage = Stage::In(steps.len() as u32 * 10);
        self.pal_blank = false;
        self.fade = Some(Fade { from: [[0; 3]; 256], to: None, ms: 0, steps });
    }

    /// 0x419ea0 ends the dialog: its event 0xe code 1 (0x4d8790) frees the area texts and
    /// blanks the palette; then 0x4d8df0 frees the text and the next one follows.
    fn credits_close(&mut self, files: &Storage) {
        self.dlg = None;
        let cr = &mut self.credits;
        cr.stage = Stage::Idle;
        cr.old = None;
        cr.snap = None;
        cr.sel = None;
        cr.text = Vec::new();
        cr.pos = 0;
        if !self.pal_blank {
            self.pal_blank = true;
            self.fade = None;
        }
        self.credits_next(files);
    }

    /// The event proc's events 4 and 7 (a mouse button) and 0 with Space or Enter: 0x4d8bd0
    /// (the next page); Escape (0x4d8d20) ends the screen and sets 0x51cec0. Not while a
    /// fade runs (natively the call blocks).
    fn credits_advance(&mut self, files: &Storage) {
        if matches!(self.credits.stage, Stage::Wait(_)) {
            self.credits.stage = Stage::Idle;
            self.credits_page(files);
        }
    }

    /// Event 0 (a key down, 0x4d8d20).
    pub(super) fn credits_key(&mut self, files: &Storage, vk: u16) {
        match vk {
            0x20 | 0x0d => self.credits_advance(files),
            0x1b if matches!(self.credits.stage, Stage::Wait(_)) => {
                self.credits.esc = true;
                self.credits_close(files);
            }
            _ => {}
        }
    }

    /// Events 4 / 7 (the left / right button went down, 0x4d8bd0).
    pub(super) fn credits_click(&mut self, files: &Storage) {
        self.credits_advance(files);
    }

    /// Event 0xd (0x4d8cd0, every pass of the dialog loop) and the fades of `ms`
    /// milliseconds: the state 2 (quit) ends the screen; after the display time (0x51cec4,
    /// 0x596a20 is 0) the next page.
    pub(super) fn credits_tick(&mut self, files: &Storage, ms: u32) {
        let mut ms = ms;
        while self.credits_open_now() {
            match self.credits.stage {
                Stage::Idle => return,
                Stage::Out(left) => {
                    if ms < left {
                        self.credits.stage = Stage::Out(left - ms);
                        return;
                    }
                    ms -= left;
                    self.credits_swap();
                }
                Stage::In(left) => {
                    if ms < left {
                        self.credits.stage = Stage::In(left - ms);
                        return;
                    }
                    ms -= left;
                    self.credits.stage = Stage::Wait(0);
                }
                Stage::Wait(elapsed) => {
                    if self.state == 2 {
                        self.credits.esc = true;
                        self.credits_close(files);
                        return;
                    }
                    let e = elapsed + ms;
                    if e <= self.credits.display {
                        self.credits.stage = Stage::Wait(e);
                        return;
                    }
                    ms = e - self.credits.display;
                    self.credits.stage = Stage::Idle;
                    self.credits_page(files);
                }
            }
        }
    }
}
