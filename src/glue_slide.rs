//! The glue dialogs' button slide: 0x4dd5e0 slides a list of controls in (they start off the
//! screen, the 20 ms timer 7 = 0x4dd220 brings them back, a quarter of the way left each
//! step) and 0x4dd040 slides them out (timer 7 = 0x4dcd00, the distance doubling each step)
//! before the dialog ends. The list (0x51c428 ...) stays until the next slide-in.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state.

use super::{AudioCmd, Glue, Open};
use crate::dlg::flag;
use crate::storage::Storage;

/// 0x51c428 .. 0x51c4b4 and the timer 7 of the dialog.
#[derive(Default)]
pub(super) struct Slide {
    /// 0x51c428 / 0x51c42c: (control id, direction: 0 from the left, 1 top, 2 right, 3 bottom).
    list: Vec<(i16, i16)>,
    /// 0x51c430: the control still moves.
    active: Vec<bool>,
    /// Control +0x24 (0x51c480): the way left to go (in) or the step (out).
    dist: Vec<i16>,
    /// 0x51c4ac: 20 ms steps before the controls move.
    delay: u32,
    /// 0x51c4b0: the first control after the one with the highest id (in the dialog's order);
    /// from it on the visible controls are hidden during the slide.
    pub(super) tail: Option<usize>,
    /// 0x51c4b4: the controls hidden by the slide-in (they show when it ends).
    hidden: Vec<usize>,
    /// Timer 7: (out = 0x4dcd00 else 0x4dd220, ms since its last step).
    timer: Option<(bool, u32)>,
    /// 0x4dd040's result, given when the slide out has ended.
    pub(super) leaving: Option<i32>,
    /// 0x51a384 == 0: the slide-in's sound has played.
    snd_off: bool,
}

impl Glue {
    /// 0x4dd5e0 (ECX = list at `list` of `count` entries, stack = the dialog and `ms`): the
    /// list's controls move out of the screen (0x20000 set) and the timer 7 brings them back
    /// after `ms` / 20 steps; the controls after the highest id hide until then.
    pub(super) fn slide_in(&mut self, list: u32, count: u16, ms: u16) {
        let l: Vec<(i16, i16)> =
            (0..count as u32).map(|i| (self.exe_u16(list + 4 * i) as i16, self.exe_u16(list + 4 * i + 2) as i16)).collect();
        let mut s = Slide { snd_off: self.slide.snd_off, delay: ms as u32 / 0x14, ..Default::default() };
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        let mut top = 0i16;
        let mut tail = None;
        for &(id, dir) in &l {
            let Some(k) = dlg.controls.iter().position(|c| c.id == id) else {
                s.dist.push(0);
                s.active.push(false);
                continue;
            };
            let c = &mut dlg.controls[k];
            c.flags |= 0x20000;
            let d = match dir {
                0 => c.rect.2 + 1,
                1 => c.rect.3 + 1,
                2 => 0x280 - c.rect.0,
                3 => 0x1e0 - c.rect.1,
                _ => 0,
            };
            match dir {
                0 => (c.rect.0, c.rect.2) = (c.rect.0 - d, c.rect.2 - d),
                1 => (c.rect.1, c.rect.3) = (c.rect.1 - d, c.rect.3 - d),
                2 => (c.rect.0, c.rect.2) = (c.rect.0 + d, c.rect.2 + d),
                3 => (c.rect.1, c.rect.3) = (c.rect.1 + d, c.rect.3 + d),
                _ => {}
            }
            s.dist.push(d as i16);
            s.active.push(true);
            if top < id {
                top = id;
                tail = Some(k);
            }
        }
        s.tail = tail.map(|k| k + 1);
        if let Some(from) = s.tail {
            // 0x418700 on every visible control after it.
            for k in from..dlg.controls.len() {
                if dlg.controls[k].flags & flag::VISIBLE != 0 {
                    dlg.controls[k].flags &= !flag::VISIBLE;
                    s.hidden.push(k);
                }
            }
        }
        s.list = l;
        s.timer = Some((false, 0));
        self.slide = s;
    }

    /// The 20 ms timer 7 (0x4dd220 / 0x4dcd00) of the open dialog, `ms` passed.
    pub(super) fn slide_tick(&mut self, files: &Storage, ms: u32) {
        if self.dlg.is_none() {
            self.slide.timer = None;
            self.slide.leaving = None;
            return;
        }
        let Some((out, acc)) = self.slide.timer else { return };
        let acc = acc + ms;
        self.slide.timer = Some((out, acc % 20));
        for _ in 0..acc / 20 {
            if self.slide.timer.is_none() {
                break;
            }
            if out { self.slide_out_step(files) } else { self.slide_in_step(files) }
        }
    }

    /// The control of list entry `i`.
    fn slide_ctl(&mut self, i: usize) -> Option<&mut crate::dlg::Ctl> {
        let id = self.slide.list[i].0;
        self.dlg.as_mut()?.0.get_mut(id)
    }

    /// 0x4dd220: after the delay each active control moves a quarter of its way back (all of
    /// it when 3 or less is left); when none has a way left the controls hidden show, the
    /// timer ends and the dialog gets event 0x405.
    fn slide_in_step(&mut self, files: &Storage) {
        if self.slide.delay != 0 {
            self.slide.delay -= 1;
            return;
        }
        if !self.slide.snd_off {
            // 0x4bc2c0.
            self.cmds.push(AudioCmd::Sound(0xac));
            self.slide.snd_off = true;
        }
        let mut done = true;
        for i in 0..self.slide.list.len() {
            if !self.slide.active[i] {
                continue;
            }
            let (d, dir) = (self.slide.dist[i] as u16, self.slide.list[i].1);
            if self.slide_ctl(i).is_none() || d == 0 {
                continue;
            }
            done = false;
            let step = if d > 3 { d >> 2 } else { d };
            self.slide.dist[i] = (d - step) as i16;
            let (s, c) = (step as i32, self.slide_ctl(i).unwrap());
            match dir {
                0 => (c.rect.0, c.rect.2) = (c.rect.0 + s, c.rect.2 + s),
                1 => (c.rect.1, c.rect.3) = (c.rect.1 + s, c.rect.3 + s),
                2 => (c.rect.0, c.rect.2) = (c.rect.0 - s, c.rect.2 - s),
                3 => (c.rect.1, c.rect.3) = (c.rect.1 - s, c.rect.3 - s),
                _ => {}
            }
        }
        if !done {
            return;
        }
        self.cmds.push(AudioCmd::Sound(0xab));
        // Event 0x405 to each hidden control (none handles it here), then 0xd: it shows.
        let hidden = std::mem::take(&mut self.slide.hidden);
        if let Some((dlg, _)) = self.dlg.as_mut() {
            for k in hidden {
                if let Some(c) = dlg.controls.get_mut(k) {
                    c.flags |= flag::VISIBLE;
                }
            }
        }
        self.slide.active.fill(false);
        self.slide.timer = None;
        self.slide.snd_off = false;
        // The dialog's event 0x405.
        match self.dlg.as_ref().map(|d| d.1) {
            Some(Open::Ready(race)) => self.ready_405(files, race),
            Some(Open::Create(_)) => self.create_405(),
            _ => {}
        }
    }

    /// 0x4dd040 (after 0x4be0b0 / 0x41ca00, the cursor): the controls after the highest id
    /// hide, every list control starts to slide out (step 1, doubling; sound 0xad when the
    /// list is not empty) and timer 7 (0x4dcd00) ends the dialog with `result` when they
    /// are gone.
    pub(super) fn slide_out(&mut self, result: i32) {
        let s = &mut self.slide;
        s.hidden.clear();
        s.leaving = Some(result);
        for i in 0..s.list.len() {
            s.dist[i] = 1;
            s.active[i] = true;
        }
        if !s.list.is_empty() {
            self.cmds.push(AudioCmd::Sound(0xad));
        }
        self.slide.timer = Some((true, 0));
        if let (Some((dlg, _)), Some(from)) = (self.dlg.as_mut(), self.slide.tail) {
            for c in dlg.controls.iter_mut().skip(from) {
                c.flags &= !flag::VISIBLE;
            }
        }
    }

    /// 0x4dcd00: each active control moves out by its step, which doubles; when all are off
    /// the screen the timer ends and the dialog is destroyed (0x419ea0). 0x4dcc00 (kills the
    /// type 0xe children's timers 0x48, and at the last count 0x51c4b8 the dialog's 0x18, clears
    /// 0x51c418) is the `slide.timer = None` below plus `dialog_done`: the light-ups' animation
    /// runs from `Dialog::tick`, and the single open dialog is `self.dlg`.
    fn slide_out_step(&mut self, files: &Storage) {
        let mut done = true;
        for i in 0..self.slide.list.len() {
            if !self.slide.active[i] {
                continue;
            }
            let (d, dir) = (self.slide.dist[i] as i32, self.slide.list[i].1);
            let Some(c) = self.slide_ctl(i) else { continue };
            match dir {
                0 => {
                    done &= c.rect.2 <= 0;
                    (c.rect.0, c.rect.2) = (c.rect.0 - d, c.rect.2 - d);
                }
                1 => {
                    done &= c.rect.3 <= 0;
                    (c.rect.1, c.rect.3) = (c.rect.1 - d, c.rect.3 - d);
                }
                2 => {
                    done &= c.rect.0 >= 0x280;
                    (c.rect.0, c.rect.2) = (c.rect.0 + d, c.rect.2 + d);
                }
                3 => {
                    done &= c.rect.1 >= 0x1e0;
                    (c.rect.1, c.rect.3) = (c.rect.1 + d, c.rect.3 + d);
                }
                _ => {}
            }
            self.slide.dist[i] = (d << 1) as i16;
        }
        if !done {
            return;
        }
        self.slide.timer = None;
        self.result = self.slide.leaving.take();
        self.slide.tail = None;
        self.dialog_done(files);
    }
}
