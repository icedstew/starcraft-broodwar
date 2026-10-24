//! The title screen (rez\titledlg.bin over glue\title\title.pcx): shown at startup after the
//! intro cinematics, while the startup loads its data, until the glue loop (0x4de200) has
//! slept 1500 ms and closes it with a fade out.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. The native
//! startup is one blocking thread (the dialog is opened by 0x4d49a0 and never run: nothing
//! reads the keys or the mouse); here the stages are timed by `title_tick`.

use super::{state, Fade, Glue, Open};
use crate::dlg::{flag, Dialog, Update};
use crate::palette::{self, Pal};
use crate::storage::Storage;
use crate::todo_port;

/// What the title screen is doing (milliseconds left).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Idle,
    /// 0x4d4870's fade in, then the load (0x4d7390: immediate here) and 0x4de200's
    /// Sleep(0x5dc).
    Hold(u32),
    /// 0x4d46a0's fade out.
    Out(u32),
}

/// The screen's globals (0x57f0dc, 0x5994e0).
pub(super) struct Title {
    /// title.pcx's palette (0x5994e0, shown from 0x6ceb40).
    pal: Pal,
    stage: Stage,
    /// The text rows of the look before tFont.pcx's replaced them (0x6ce000).
    rows: Option<(usize, Vec<u8>)>,
}

impl Default for Title {
    fn default() -> Title {
        Title { pal: [[0; 3]; 256], stage: Stage::Idle, rows: None }
    }
}

/// 0x4d4870's fade_in(3) takes 3 steps of 10 ms.
const FADE_MS: u32 = 30;
/// 0x4de200's Sleep(0x5dc) after the glue resources are loaded.
const SLEEP_MS: u32 = 0x5dc;

impl Glue {
    /// Whether the title dialog is the open one.
    pub(super) fn title_open_now(&self) -> bool {
        matches!(self.dlg, Some((_, Open::Title)))
    }

    /// The palette the title screen shows.
    pub(super) fn title_palette(&self) -> &Pal {
        &self.title.pal
    }

    /// 0x4d49a0: loads rez\titledlg.bin (flags |= 4, the images of 0x4d27a0) and opens it
    /// (0x419d20: the init event 0xe code 0 runs 0x4d4870 and a 100 ms timer starts). The
    /// dialog is not run: 0x4e0820 goes on loading (0x4d7390) and enters the glue loop.
    pub(super) fn title_start(&mut self, files: &Storage) {
        let Ok(d) = files.read("rez/titledlg.bin") else { crate::bw::fatal_error("rez\titledlg.bin") };
        let mut dlg = Dialog::parse(&d);
        dlg.this.flags |= 4;
        dlg.bind(files);
        self.title_init(files, &mut dlg);
        self.dlg = Some((dlg, Open::Title));
    }

    /// 0x4d4870 (event 0xe code 0): glue\title\title.pcx is the dialog's bitmap and its
    /// palette (0x5994e0), glue\title\tFont.pcx the text color rows (0x6ce000); the title
    /// music (0x4a6310, track 0x16, when 0x6d0f10 is 0); the dialog is redrawn, the cursor
    /// hidden (0x4be0b0) and the palette fades in (0x41ea30, 3 steps); then 0x4d46f0.
    fn title_init(&mut self, files: &Storage, dlg: &mut Dialog) {
        let Ok(d) = files.read("glue/title/title.pcx") else { crate::bw::fatal_error("glue\title\title.pcx") };
        let (w, h, px) = crate::view::pcx(&d);
        self.title.pal = super::pcx_palette(&d);
        dlg.bg = Some((w as i32, h as i32, px));
        let Ok(f) = files.read("glue/title/tFont.pcx") else { crate::bw::fatal_error("glue\title\tFont.pcx") };
        let rows = crate::view::pcx(&f).2;
        let p = self.palette_id();
        self.look(files, p);
        if let Some(look) = self.looks[p].as_mut() {
            let saved = look.skin.text_rows.clone();
            for (r, v) in look.skin.text_rows.iter_mut().zip(rows.iter().take(0xc0)) {
                *r = *v;
            }
            self.title.rows = Some((p, saved));
        }
        // 0x6d0f10 is 0 here (set by the startup's other paths only).
        self.music_start(0x16);
        self.flush_notice();
        // The palette is blank before the first screen (0x51a0e9 clear).
        self.pal_blank = true;
        self.fade_in(3);
        self.title_text(dlg);
        self.title.stage = Stage::Hold(FADE_MS + SLEEP_MS);
    }

    /// 0x4d46f0: the control with id -10 (-11 when 0x6d0f10 is set) of the dialog is the
    /// title text's box. Natively its rectangle is copied twice from the bitmap (0x57f0e4
    /// gets the control's text drawn with flags 0x22, color row 2, font 0x6ce0fc, the other
    /// 0x57f0ec stays clean) and shown (0x4186a0); here the control draws the text itself
    /// over the bitmap, which is the same picture.
    fn title_text(&mut self, dlg: &mut Dialog) {
        if let Some(c) = dlg.get_mut(-10) {
            c.update = Update::Align(0x22);
            c.flags = (c.flags & !(0x4c00 | 0xe000_0082)) | flag::FONT_LARGE | flag::VISIBLE;
        }
        // 0x4d4640 (the 500 ms timer set here) swaps the control's image between the text
        // and the clean copy; it needs the message loop, which the startup does not pump
        // before the dialog closes (the load and the sleep block), so it never fires.
        todo_port!("0x4d4640", "the title text blink (500 ms timer: never fires natively)");
    }

    /// 0x4d4550 (the text control's draw): the image of the control blitted opaque; the
    /// text of `title_text` here.
    ///
    /// 0x4d46a0 (the dialog's event 0xe code 1, from 0x419ea0 in 0x4de200): 0x4d45a0 stops
    /// the timer and frees the two images; the palette is blanked and faded out
    /// (0x41e8b0, 3 steps), as 0x4de200 does after its Sleep.
    fn title_close(&mut self) {
        self.title.stage = Stage::Out(FADE_MS);
        self.pal_blank = true;
        self.fade = Some(Fade { from: self.title.pal, to: Some([[0; 3]; 256]), ms: 0, steps: palette::fade_steps(3) });
    }

    /// Time passes: after the load and 0x4de200's sleep the screen fades out, then the
    /// glue loop (state 4) goes on with its main menu.
    pub(super) fn title_tick(&mut self, ms: u32) {
        if !self.title_open_now() {
            return;
        }
        match self.title.stage {
            Stage::Idle => {}
            Stage::Hold(left) if ms < left => self.title.stage = Stage::Hold(left - ms),
            Stage::Hold(_) => self.title_close(),
            Stage::Out(left) if ms < left => self.title.stage = Stage::Out(left - ms),
            Stage::Out(_) => {
                self.title.stage = Stage::Idle;
                self.dlg = None;
                if let Some((p, rows)) = self.title.rows.take() {
                    if let Some(l) = self.looks[p].as_mut() {
                        l.skin.text_rows = rows;
                    }
                }
                self.fade = None;
                self.shown = -1;
                self.state = state::GLUE;
            }
        }
    }
}
