//! The mission briefing / ready room (rez\glurdyz.bin, glurdyt.bin, glurdyp.bin; event procs
//! 0x4f7150 / 0x4f74b0 / 0x4f7810): the briefing triggers of the map (CHK MBRF, run by the
//! 100 ms timer 0x46d1b0 through 0x427da0) show text, objectives and portraits and play wavs;
//! Start (id 13) tells the other players (here: the turn exchange of the single player
//! provider, 0x485bd0 / 0x485aa0 / 0x486040) and the idle proc 0x46d340 ends the dialog with
//! result 0x65 once everyone is ready.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. Timers
//! (0x416090 / 0x416000) are `Ready::timers`, run by `ready_tick`.

use super::{screen, AudioCmd, Glue, Open};
use crate::dlg::{flag, Ctl, Pic, Update};
use crate::portrait::Portraits;
use crate::smk::Smk;
use crate::storage::Storage;
use crate::todo_port;
use std::collections::VecDeque;

/// The controls of glurdy?.bin.
mod ids {
    /// The Start button (0x46d160 case 0xd).
    pub const START: i16 = 13;
    /// Cancel (case 0xe).
    pub const CANCEL: i16 = 14;
    /// The portraits 0xf ..= 0x12 (event proc 0x44efe0).
    pub const PORT_FIRST: i16 = 0xf;
    pub const PORT_LAST: i16 = 0x12;
    /// Replay (case 0x14: the briefing starts again).
    pub const REPLAY: i16 = 0x14;
    /// Skip Tutorial (0x13 ends the dialog), and the picture behind it (-0xd).
    pub const SKIP: i16 = 0x13;
    pub const SKIP_PIC: i16 = -0xd;
    /// ButProt.pcx (hidden in network games).
    pub const BUTTONS_PIC: i16 = 5;
    /// The message box (draw 0x46c6e0), the objectives text, "Waiting for other players",
    /// "No briefing data".
    pub const MESSAGE: i16 = -10;
    pub const OBJECTIVES: i16 = -11;
    pub const WAITING: i16 = -12;
    pub const NO_BRIEFING: i16 = -14;
}

/// Who a timer belongs to: a control of the dialog (by id) or the dialog (None).
type Owner = Option<i16>;

/// The procs the briefing's timers run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Proc {
    /// 0x46d1b0 (dialog timer 10, 100 ms): the briefing's trigger runner.
    Brief,
    /// 0x46c750 (dialog timer 0xb): a wait or a transmission ends.
    EndSpeak,
    /// 0x46c990 (message timer 1): the message is hidden.
    MsgEnd,
    /// 0x46c960 (message timer 3): the long text starts to scroll.
    MsgScrollStart,
    /// 0x46c940 (message timer 2): the text scrolls one row.
    MsgScroll,
    /// 0x44ee80 (portrait timers 1 and 2).
    Port,
}

struct Timer {
    owner: Owner,
    id: u16,
    interval: u32,
    left: u32,
    proc: Proc,
}

/// A portrait control's record (0x44ef00, 0x14 bytes) and its video.
#[derive(Default)]
struct Port {
    /// +8: the video playing; `fresh` its frame was not drawn yet (Ordinal_457's out value),
    /// `ended` it finished (Ordinal_457 returns 0).
    video: Option<Smk>,
    elapsed: u32,
    fresh: bool,
    ended: bool,
    /// +0xc: bit 0 talking, bit 1 shown with the first frame.
    flags: u8,
    /// +0xe: the portrait (portdata.dat entry).
    portrait: u16,
    /// +0x10: the last talking variant picked (minus one).
    last: u8,
    /// The remap of the video's palette to the screen's and the palette it was made for.
    remap: Option<([[u8; 3]; 256], [u8; 256])>,
}

/// The turn exchange 0x485aa0 / 0x486580 / 0x486530 with Storm's single player provider:
/// turns sent are received back (provider latency 1), the commands in them run for the
/// player (0x486040).
#[derive(Default)]
struct Net {
    /// 0x654880: the commands queued for the next turn (its length is Setup::g654aa0).
    buf: Vec<u8>,
    /// The turns sent and not yet received.
    turns: VecDeque<Vec<u8>>,
    /// 0x57f0b8: per player 0x10000 connected, 0x20000 a turn arrived; 0x654a80 its data.
    flags: [u32; 8],
    data: [Vec<u8>; 8],
    /// 0x6556d8: per player, the ready command (0x54) arrived.
    ready: [bool; 8],
    /// 0x57ee78: the last receive got a turn.
    rx: bool,
}

/// The ready room's globals.
pub(super) struct Ready {
    /// +0x26 of the dialog: the briefing trigger running (an index into Setup::briefing).
    node: Option<usize>,
    /// +0x4a of the dialog: the idle proc 0x46d340 is set.
    poll: bool,
    /// 0x6ca664: a wait or a transmission is going.
    speaking: bool,
    timers: Vec<Timer>,
    ports: [Port; 4],
    net: Net,
    /// 0x6d63d0 / 0x6d63d4: the clock of the last poll that got a turn.
    last_poll: u32,
    last_rx: u32,
    /// +0x26 of the message control: the ms between two scroll rows.
    msg_interval: u32,
    /// units.dat's portrait of each unit (0x662f88) and portdata.dat (0x44f0f0).
    unit_portrait: Vec<u16>,
    portraits: Option<Portraits>,
    /// 0x58d6c4: the mission objectives string per player.
    objectives: [u32; 8],
    /// 0x68f538 / 0x68f55c: the two frame images of each portrait (0x44f020; never drawn).
    frames: Vec<Vec<(usize, usize, Vec<u8>)>>,
    /// The C runtime's _holdrand (0x44eb90 / 0x44ec90 use rand()).
    holdrand: u32,
}

impl Default for Ready {
    fn default() -> Ready {
        Ready {
            node: None,
            poll: false,
            speaking: false,
            timers: Vec::new(),
            ports: Default::default(),
            net: Net::default(),
            // GetTickCount() - 0 at the first poll: long past 0xfa.
            last_poll: 0u32.wrapping_sub(0x10000),
            last_rx: 0,
            msg_interval: 0,
            unit_portrait: Vec::new(),
            portraits: None,
            objectives: [0; 8],
            frames: Vec::new(),
            holdrand: 1,
        }
    }
}

impl Glue {
    fn rctl(&mut self, id: i16) -> Option<&mut Ctl> {
        match self.dlg.as_mut() {
            Some((d, Open::Ready(_))) => d.get_mut(id),
            _ => None,
        }
    }

    /// 0x4186a0 (`on`) / 0x418700: show or hide the control.
    fn rshow(&mut self, id: i16, on: bool) {
        if let Some(c) = self.rctl(id) {
            if on {
                c.flags |= flag::VISIBLE;
            } else {
                c.flags &= !flag::VISIBLE;
            }
        }
    }

    /// 0x418640: disable the control.
    fn rdisable(&mut self, id: i16) {
        if let Some(c) = self.rctl(id) {
            if c.flags & flag::DISABLED == 0 {
                c.flags = (c.flags & !0x80) | flag::DISABLED;
            }
        }
    }

    /// 0x416090: the timer (`owner`, `id`) runs `proc` every `interval` ms (an old one of
    /// the same owner and id is replaced).
    fn rtimer_set(&mut self, owner: Owner, id: u16, interval: u32, proc: Proc) {
        let t = Timer { owner, id, interval, left: interval, proc };
        match self.ready.timers.iter_mut().find(|t| t.owner == owner && t.id == id) {
            Some(old) => *old = t,
            None => self.ready.timers.push(t),
        }
    }

    /// 0x416000.
    fn rtimer_kill(&mut self, owner: Owner, id: u16) {
        self.ready.timers.retain(|t| !(t.owner == owner && t.id == id));
    }

    /// The C runtime's rand().
    fn rrand(&mut self) -> u32 {
        let r = &mut self.ready.holdrand;
        *r = r.wrapping_mul(214013).wrapping_add(2531011);
        (*r >> 16) & 0x7fff
    }

    // ---- opening ----

    /// The events of a new ready room dialog (0x4f7150 / 0x4f74b0 / 0x4f7810): 10 (0x46d3c0,
    /// the slide-in 0x4dd5e0, the controls' own init: the portraits 0x44ef00), then 0x405 when
    /// the controls have slid in (at once here: 0x4f70f0 / 0x4f7450 / 0x4f77b0).
    pub(super) fn ready_open(&mut self, files: &Storage, race: u8) {
        self.ready = Ready::default();
        self.ready_46d3c0();
        todo_port!("0x4dd5e0", "slide-in of the buttons (lists 0x512a70 / 0x512a54 / 0x512a14) and its timer 0x4dd220");
        for id in ids::PORT_FIRST..=ids::PORT_LAST {
            self.ready_44ef00(id);
        }
        self.ready_405(files, race);
    }

    /// 0x46d3c0 (event 10): in a network game the buttons picture and Replay go away; the ready
    /// flags clear, the turn exchange starts (0x6554b0 = 1, 0x485aa0) and 0x46d340 is the
    /// dialog's idle proc.
    fn ready_46d3c0(&mut self) {
        if self.setup.multiplayer {
            self.rshow(ids::BUTTONS_PIC, false);
            self.rshow(ids::REPLAY, false);
        }
        self.ready.net.ready = [false; 8];
        self.setup.g6554b0 = 1;
        self.net_485aa0();
        self.ready.poll = true;
    }

    /// 0x44ef00 (event 10 of a portrait control, 0x44efe0): the video buffer (0x78 x 0x70) and
    /// its place in the control, the draw 0x44e9e0 and the 50 ms timer 1; hidden.
    fn ready_44ef00(&mut self, id: i16) {
        let Some(c) = self.rctl(id) else { return };
        let (w, h) = (c.rect.2 - c.rect.0, c.rect.3 - c.rect.1);
        c.update = Update::Portrait;
        c.pic = Some(Pic { w: 0x78, h: 0x70, px: vec![0; 0x78 * 0x70], scroll: 0, dx: (w - 0x77) / 2, dy: (h - 0x6f) / 2 });
        self.ready.ports[(id - ids::PORT_FIRST) as usize] = Port::default();
        self.rtimer_set(Some(id), 1, 0x32, Proc::Port);
        self.rshow(id, false);
    }

    /// Event 0x405: 0x46d220, then 0x44f020 with the race's two lists of frame images.
    fn ready_405(&mut self, files: &Storage, race: u8) {
        self.ready_46d220(files);
        // 0x4f70f0 (zerg) / 0x4f7450 (terran) / 0x4f77b0 (protoss): the strings of both lists.
        let (a, b): ([u32; 4], [u32; 4]) = match race {
            0 => ([0x5031f0, 0x5031d4, 0x5031b8, 0x50319c], [0x503254, 0x50323c, 0x503224, 0x50320c]),
            1 => ([0x5030f0, 0x5030d4, 0x5030b8, 0x50309c], [0x503154, 0x50313c, 0x503124, 0x50310c]),
            _ => ([0x502e9c, 0x502e80, 0x502e64, 0x502e48], [0x502f00, 0x502ee8, 0x502ed0, 0x502eb8]),
        };
        self.ready_44f020(files, &a, &b);
    }

    /// 0x44f020 (EAX = list `a`, stack = list `b`): per portrait the two images (Ordinal_325
    /// loads each .pcx), kept in 0x68f538 / 0x68f55c. Nothing draws them.
    fn ready_44f020(&mut self, files: &Storage, a: &[u32; 4], b: &[u32; 4]) {
        let path = |g: &Glue, va: u32| -> String { (0..).map(|k| g.exe_u8(va + k)).take_while(|&c| c != 0).map(|c| c as char).collect() };
        self.ready.frames.clear();
        for i in 0..4 {
            let mut pair = Vec::new();
            for va in [b[i], a[i]] {
                let p = path(self, va);
                let Ok(d) = files.read(&p.replace('\\', "/")) else { crate::bw::fatal_error(&p) };
                pair.push(crate::view::pcx(&d));
            }
            self.ready.frames.push(pair);
        }
    }

    /// 0x46d220: the map's briefing (0x4cc990), the units' and portraits' data (0x44f0f0), the
    /// message control's draw 0x46c6e0 and buffer, the 100 ms briefing timer 10, the first
    /// briefing trigger (0x46cbc0); without a briefing "No briefing data" shows and Replay is
    /// disabled (in a network game the ready command goes out at once: 0x46d090).
    fn ready_46d220(&mut self, files: &Storage) {
        self.setup.load_briefing(files, &self.network_raw);
        self.ready_44f0f0(files);
        if let Some(c) = self.rctl(ids::MESSAGE) {
            let (w, h) = (c.rect.2 - c.rect.0 + 1, (c.rect.3 - c.rect.1 + 1) * 20);
            c.update = Update::BriefText;
            c.pic = Some(Pic { w, h, px: vec![0; (w * h) as usize], scroll: 0, dx: 0, dy: 0 });
        }
        self.rtimer_set(None, 10, 100, Proc::Brief);
        self.ready_46cbc0();
        if self.ready.node.is_none() {
            self.rshow(ids::NO_BRIEFING, true);
            if self.setup.multiplayer {
                self.ready_46d090(false);
                return;
            }
            self.rdisable(ids::REPLAY);
        }
    }

    /// 0x44f0f0: units.dat (the portraits of the units) and portdata.dat / portdata.tbl.
    fn ready_44f0f0(&mut self, files: &Storage) {
        self.ready.unit_portrait = crate::dat::Units::load(files).map(|u| u.portrait).unwrap_or_default();
        self.ready.portraits = Portraits::load(files);
    }

    // ---- the briefing ----

    /// 0x46cbc0: the briefing starts again: the wavs stop, portraits and texts go, the timers
    /// stop, the running trigger is reset and the first one of the list runs.
    fn ready_46cbc0(&mut self) {
        self.ready_stop_wavs();
        for id in ids::PORT_FIRST..=ids::PORT_LAST {
            self.ready_port_close(id);
            self.rshow(id, false);
        }
        if let Some(c) = self.rctl(ids::MESSAGE) {
            c.text.clear();
        }
        self.rshow(ids::MESSAGE, false);
        for id in 1..=3 {
            self.rtimer_kill(Some(ids::MESSAGE), id);
        }
        self.rshow(ids::OBJECTIVES, false);
        self.ready.speaking = false;
        self.rtimer_kill(None, 0xb);
        if let Some(n) = self.ready.node {
            if let Some(rec) = self.setup.briefing.get_mut(n) {
                let o = 0x140 + rec[0x95f] as usize * 0x20 + 0x1c;
                rec[o] &= 0xfe;
                rec[0x95f] = 0;
                rec[0x940] &= 0xfe;
            }
        }
        self.ready.node = if self.setup.briefing.is_empty() { None } else { Some(0) };
    }

    /// 0x46ca90: the briefing stops for good (the dialog ends): its timers, wavs, texts and
    /// portraits.
    fn ready_46ca90(&mut self) {
        for id in 1..=3 {
            self.rtimer_kill(Some(ids::MESSAGE), id);
        }
        self.rtimer_kill(None, 0xa);
        self.rtimer_kill(None, 0xb);
        self.ready_stop_wavs();
        self.rshow(ids::MESSAGE, false);
        self.rshow(ids::OBJECTIVES, false);
        for id in ids::PORT_FIRST..=ids::PORT_LAST {
            self.ready_port_close(id);
            self.rshow(id, false);
        }
    }

    /// 0x4015f0 (list 0x51a208): the wavs the briefing started stop.
    fn ready_stop_wavs(&mut self) {
        todo_port!("0x4015f0", "stop the briefing's playing wavs (AudioCmd::Wav plays detached)");
    }

    /// 0x46d090 (`leave`): the briefing ends (no trigger runs, Start is disabled, in a network
    /// game "Waiting for other players" shows); with `leave` the turn exchange ends too (0x4c3d20)
    /// and the idle proc goes, else the ready command (0x54) goes out (0x485bd0) and 0x6554b0 = 2.
    fn ready_46d090(&mut self, leave: bool) {
        self.ready.node = None;
        self.rdisable(ids::START);
        if self.setup.multiplayer {
            self.rshow(ids::WAITING, true);
        }
        if leave {
            self.net_4c3d20(3);
        }
        if self.setup.g6554b0 == 1 {
            if !leave {
                self.net_485bd0(&[0x54]);
                self.setup.g6554b0 = 2;
                return;
            }
            self.setup.g6554b0 = 0;
        }
        if leave {
            self.ready.poll = false;
        }
    }

    /// 0x46d160 (the dialog's event 0xe code 2, `self.activated` the control): Start sends
    /// the ready command, Cancel leaves, Replay restarts the briefing, anything else (Skip
    /// Tutorial, the poll's 100 / 0x65) ends the dialog with its id.
    pub(super) fn ready_46d160(&mut self) {
        match self.activated {
            0xd => self.ready_46d090(false),
            0xe => {
                self.ready_46d090(true);
                self.ready_46ca90();
                self.end_dialog(self.activated as i32);
            }
            0x14 => self.ready_46cbc0(),
            _ => {
                self.ready_46ca90();
                self.end_dialog(self.activated as i32);
            }
        }
    }

    /// 0x46cfd0 (event 1): the dialog goes: its buffers, the briefing list (0x402330) and the
    /// map archive (0x596bc4).
    pub(super) fn ready_46cfd0(&mut self) {
        self.ready_46ca90();
        // 0x44eaa0 frees the portrait data and the frame images.
        self.setup.close_map_archive();
        self.setup.briefing.clear();
        self.ready = Ready::default();
    }

    /// 0x427da0 (ESI = the trigger): runs a briefing trigger's actions up to one that waits;
    /// true when it is done (or not a briefing trigger).
    fn ready_427da0(&mut self, files: &Storage, n: usize) -> bool {
        if self.setup.briefing[n][0xf] != 0xd {
            return true;
        }
        self.setup.briefing[n][0x940] |= 1;
        loop {
            let idx = self.setup.briefing[n][0x95f];
            if idx > 0x3f {
                break;
            }
            let a = 0x140 + idx as usize * 0x20;
            let (op, fl) = (self.setup.briefing[n][a + 0x1a], self.setup.briefing[n][a + 0x1c]);
            if fl & 2 == 0 {
                if op == 0 {
                    self.setup.briefing[n][0x95f] = 0x40;
                    break;
                }
                if !self.ready_action(files, n, a, op) {
                    break;
                }
                self.setup.briefing[n][0x95f] += 1;
            } else {
                self.setup.briefing[n][0x95f] = idx + 1;
            }
        }
        if self.setup.briefing[n][0x95f] < 0x40 {
            return false;
        }
        self.setup.briefing[n][0x95f] = 0;
        self.setup.briefing[n][0x940] &= 0xfe;
        true
    }

    /// 0x519e50: the action table of the briefing (opcode 1 .. 9); false when the trigger
    /// waits at this action.
    fn ready_action(&mut self, files: &Storage, n: usize, a: usize, op: u8) -> bool {
        let rec = &self.setup.briefing[n];
        let u32_at = |o: usize| u32::from_le_bytes(rec[a + o..a + o + 4].try_into().unwrap());
        let (string, time, player, number) = (u32_at(4), u32_at(0xc), rec[a + 0x10], u32_at(0x14));
        let (unit, modifier, flags) = (u16::from_le_bytes([rec[a + 0x18], rec[a + 0x19]]), rec[a + 0x1b], rec[a + 0x1c]);
        match op {
            // 0x427e50: wait.
            1 => {
                if self.ready.speaking {
                    return false;
                }
                if flags & 1 != 0 {
                    self.setup.briefing[n][a + 0x1c] &= 0xfe;
                    return true;
                }
                self.rtimer_set(None, 0xb, time, Proc::EndSpeak);
                self.setup.briefing[n][a + 0x1c] |= 1;
                self.ready.speaking = true;
                false
            }
            // 0x427fa0: play wav.
            2 => {
                self.ready_427fa0(files, n, a);
                true
            }
            // 0x427f10: text message.
            3 => {
                let text = self.ready_string(string as u16);
                self.ready_46cdd0(files, text, time);
                true
            }
            // 0x427ec0: mission objectives.
            4 => {
                let local = self.setup.local_slot[0] as usize & 7;
                self.ready.objectives[local] = string;
                self.ready_46cd30();
                true
            }
            // 0x427ef0: show portrait.
            5 => {
                self.ready_46cf30(files, player, unit);
                true
            }
            // 0x427ee0: hide portrait.
            6 => {
                self.ready_46ca30(player);
                true
            }
            // 0x427f80: display speaking portrait.
            7 => {
                self.ready_46d040(player, time);
                true
            }
            // 0x4280e0: transmission.
            8 => {
                if self.ready.speaking {
                    return false;
                }
                if flags & 1 != 0 {
                    self.setup.briefing[n][a + 0x1c] &= 0xfe;
                    return true;
                }
                let ms = match modifier {
                    7 => number,
                    8 => time.wrapping_add(number),
                    9 if number > time => 0,
                    9 => time - number,
                    _ => time,
                };
                self.ready_427fa0(files, n, a);
                let text = self.ready_string(string as u16);
                self.ready_46cdd0(files, text, time);
                self.ready_46d040(player, ms);
                self.rtimer_set(None, 0xb, ms, Proc::EndSpeak);
                self.setup.briefing[n][a + 0x1c] |= 1;
                self.ready.speaking = true;
                self.ready_4bbe50();
                false
            }
            // 0x427ea0: skip tutorial enabled.
            9 => {
                if !self.setup.multiplayer && self.setup.map_dir_flag != 0 {
                    // 0x46c9c0
                    self.rshow(ids::SKIP, true);
                    self.rshow(ids::SKIP_PIC, true);
                }
                true
            }
            _ => true,
        }
    }

    /// The map's string `id` as the briefing's actions read it: None without a table or for
    /// id 0 (a null text), "" past the table's end.
    fn ready_string(&self, id: u16) -> Option<Vec<u8>> {
        self.setup.map_string(id).map(|s| s.to_vec())
    }

    /// 0x427fa0: the action's wav (a map string) is played (0x4bc490): from the campaign's own
    /// directory (0x59c080 of the mission, 0x57f244 set) or the map archive.
    fn ready_427fa0(&mut self, files: &Storage, n: usize, a: usize) {
        let rec = &self.setup.briefing[n];
        if u32::from_le_bytes(rec[a + 8..a + 12].try_into().unwrap()) == 0 {
            return;
        }
        let id = u16::from_le_bytes([rec[a + 8], rec[a + 9]]);
        let name = self.ready_string(id).unwrap_or_default();
        let name = String::from_utf8_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]).into_owned();
        let data = if self.setup.map_dir_flag == 0 {
            self.setup.map_archive.as_ref().and_then(|m| m.read(&name).ok())
        } else {
            let dir = self.map_names.get(self.setup.map_dir_flag as usize).cloned().unwrap_or_default();
            files.read(&format!("{dir}\\{name}")).ok()
        };
        // 0x4bc490 (digital sound off: nothing; the window loop knows).
        if let Some(d) = data {
            self.cmds.push(AudioCmd::Wav(d));
        }
    }

    /// 0x4bbe50: the finished wavs leave the list 0x51a208 (a wav here plays detached).
    fn ready_4bbe50(&mut self) {}

    /// 0x46cd30: the objectives control shows the local player's objectives string.
    fn ready_46cd30(&mut self) {
        let id = self.ready.objectives[self.setup.local_slot[0] as usize & 7];
        let text = self.ready_string(id as u16).unwrap_or_default();
        if let Some(c) = self.rctl(ids::OBJECTIVES) {
            c.text = text;
        }
        self.rshow(ids::OBJECTIVES, true);
    }

    /// 0x46cdd0 (EAX = text, `ms` the time it stays): the text is drawn into the message
    /// buffer (font 0x6ce0f8, left top, color 2), the control shows it, hides it after `ms`
    /// (timer 1) and, when it is taller than the control, scrolls it (timers 3 and 2).
    fn ready_46cdd0(&mut self, files: &Storage, text: Option<Vec<u8>>, ms: u32) {
        let Some(text) = text else { return };
        let p = self.palette_id();
        self.look(files, p);
        let skin = &self.looks[p].as_ref().unwrap().skin;
        let font = skin.fonts[1].clone();
        let rows = skin.text_rows.clone();
        let Some(c) = self.rctl(ids::MESSAGE) else { return };
        let Some(pic) = c.pic.as_mut() else { return };
        pic.px.fill(0);
        let o = crate::ui::TextOpts::new();
        let ext = font.draw_bytes(&text, 0, 0, 2, &o, crate::ui::TextDst { rows: &rows, out: &mut pic.px, w: pic.w, h: pic.h });
        pic.scroll = 0;
        let height = c.rect.3 - c.rect.1;
        self.rshow(ids::MESSAGE, true);
        self.rtimer_set(Some(ids::MESSAGE), 1, ms, Proc::MsgEnd);
        self.rtimer_kill(Some(ids::MESSAGE), 2);
        if ext.b > height {
            let half = (height + 1) / 2;
            let mut rows = ext.b - half;
            if (half as u32) > rows as u32 {
                rows = half;
            }
            let rows = rows as u32;
            if ms > 4000 {
                self.ready.msg_interval = (ms - 4000) / rows;
                self.rtimer_set(Some(ids::MESSAGE), 3, 4000, Proc::MsgScrollStart);
            } else {
                self.ready.msg_interval = ms / rows;
                self.rtimer_set(Some(ids::MESSAGE), 2, ms / rows, Proc::MsgScroll);
            }
        }
    }

    // ---- portraits ----

    fn ready_port(&mut self, id: i16) -> Option<&mut Port> {
        self.ready.ports.get_mut(id.wrapping_sub(ids::PORT_FIRST) as usize)
    }

    /// Closes the portrait's video (Ordinal_458) and clears its flags (0x46ca30 / 0x46cbc0).
    fn ready_port_close(&mut self, id: i16) {
        if let Some(p) = self.ready_port(id) {
            p.video = None;
            p.flags = 0;
        }
    }

    /// 0x46ca30 (CL = the slot): the portrait goes.
    fn ready_46ca30(&mut self, slot: u8) {
        let id = ids::PORT_FIRST + slot as i16;
        self.ready_port_close(id);
        self.rshow(id, false);
    }

    /// 0x46cf30 (CL = the slot, `unit`): the portrait of the unit (units.dat; two campaign
    /// replacements) shows with its first frame.
    fn ready_46cf30(&mut self, files: &Storage, slot: u8, unit: u16) {
        let id = ids::PORT_FIRST + slot as i16;
        let table = |g: &Glue, u: usize| g.ready.unit_portrait.get(u).copied().unwrap_or(0);
        let mut p = table(self, unit as usize);
        let mission = self.setup.map_dir_flag;
        if p == 0x26 {
            if mission > 0x1f {
                p = table(self, 152);
            }
        } else if p == 0x3c && mission > 0x1b {
            p = table(self, 78);
        }
        if let Some(st) = self.ready_port(id) {
            st.video = None;
            st.flags = 2;
            st.portrait = p;
        }
        self.ready_44edb0(files, id);
    }

    /// 0x46d040 (CL = the slot): the portrait talks for `ms` (timer 2 of its control ends it).
    fn ready_46d040(&mut self, slot: u8, ms: u32) {
        let id = ids::PORT_FIRST + slot as i16;
        if let Some(st) = self.ready_port(id) {
            st.flags |= 1;
        }
        self.rtimer_set(Some(id), 2, ms, Proc::Port);
    }

    /// 0x44ee80 (timers 1 and 2 of a portrait control).
    fn ready_44ee80(&mut self, files: &Storage, id: i16, timer: u16) {
        let visible = self.rctl(id).is_some_and(|c| c.flags & flag::VISIBLE != 0);
        let Some(st) = self.ready_port(id) else { return };
        if !visible && st.flags & 2 == 0 {
            return;
        }
        if timer != 1 {
            if timer != 2 {
                return;
            }
            st.flags &= !1;
            st.video = None;
            self.rtimer_kill(Some(id), 2);
        }
        self.ready_44edb0(files, id);
    }

    /// 0x44edb0: the portrait's video moves on: a new frame shows (the control appears with
    /// the first one) or, when it has ended, the next one starts (0x44eb90 idle, 0x44ec90 talking).
    fn ready_44edb0(&mut self, files: &Storage, id: i16) {
        let Some(st) = self.ready_port(id) else { return };
        if st.video.is_some() && !st.ended {
            if !st.fresh {
                return;
            }
            st.fresh = false;
            let show = st.flags & 2 != 0;
            st.flags &= !2;
            if show {
                self.rshow(id, true);
            }
            self.ready_port_frame(id);
            return;
        }
        st.video = None;
        let talking = st.flags & 1 != 0;
        if talking {
            self.ready_44ec90(files, id);
        } else {
            self.ready_44eb90(files, id);
        }
    }

    /// 0x44eb90: an idle video of the portrait: the variant by a percent roll against portdata.dat's
    /// weights, then down to variant 0 until one opens.
    fn ready_44eb90(&mut self, files: &Storage, id: i16) {
        let r = ((self.rrand() >> 8) % 101) as u8;
        let Some(st) = self.ready_port(id) else { return };
        let p = st.portrait;
        let w = |g: &Glue, c: usize| g.ready.portraits.as_ref().and_then(|ps| ps.weights[c].get(p as usize).copied()).unwrap_or(0);
        let mut c: i32 = 0;
        let mut sum = w(self, 0);
        loop {
            if r <= sum {
                break;
            }
            c += 1;
            if c >= 4 {
                c = 0;
                break;
            }
            sum = sum.wrapping_add(w(self, c as usize));
        }
        self.ready_port_open(files, id, false, c);
    }

    /// 0x44ec90: a talking video: variant 0 .. 2 (not the last one's), then lower ones.
    fn ready_44ec90(&mut self, files: &Storage, id: i16) {
        let mut c = ((self.rrand() >> 8) % 3) as u8;
        let Some(st) = self.ready_port(id) else { return };
        if c as i32 == st.last as i32 {
            c = ((c as u32 + 2) % 3) as u8;
        }
        let last = self.ready_port_open(files, id, true, c as i32);
        if let Some(st) = self.ready_port(id) {
            st.last = last as u8;
        }
    }

    /// Ordinal_454 for the portrait: variant `c`, then `c - 1` .. 0 while it fails. Returns the
    /// variant the last try left behind (one less than the one opened).
    fn ready_port_open(&mut self, files: &Storage, id: i16, talking: bool, mut c: i32) -> i32 {
        let mut open = None;
        let paths: Vec<Option<String>> = {
            let p = self.ready_port(id).map(|s| s.portrait).unwrap_or(0);
            (0..=c.max(0)).rev().map(|v| self.ready.portraits.as_ref().and_then(|ps| ps.path(p, talking, v as u32))).collect()
        };
        for path in paths {
            if c < 0 || open.is_some() {
                break;
            }
            open = path.and_then(|p| files.read(&p).ok()).and_then(Smk::parse);
            c -= 1;
        }
        if let Some(mut s) = open {
            s.next_frame();
            if let Some(st) = self.ready_port(id) {
                st.video = Some(s);
                st.elapsed = 0;
                st.fresh = true;
                st.ended = false;
            }
        }
        c
    }

    /// The portrait's current video frame goes into its buffer, in the screen's palette.
    fn ready_port_frame(&mut self, id: i16) {
        let p = self.palette_id();
        let pal = self.looks[p].as_ref().map(|l| l.pal);
        let Some(pal) = pal else { return };
        let Some(i) = (id - ids::PORT_FIRST).try_into().ok().filter(|&i: &usize| i < 4) else { return };
        let st = &mut self.ready.ports[i];
        let Some(v) = st.video.as_ref() else { return };
        let fresh = matches!(&st.remap, Some((vp, _)) if *vp == v.palette);
        if !fresh {
            let mut t = [0u8; 256];
            for (k, c) in v.palette.iter().enumerate() {
                t[k] = (1..256)
                    .min_by_key(|&j| (0..3).map(|q| (pal[j][q] as i32 - c[q] as i32).pow(2)).sum::<i32>())
                    .unwrap_or(0) as u8;
            }
            st.remap = Some((v.palette, t));
        }
        let Some((_, t)) = &st.remap else { return };
        let (vw, vh) = (v.w, v.h);
        let Some(c) = self.dlg.as_mut().and_then(|(d, _)| d.get_mut(id)) else { return };
        let Some(pic) = c.pic.as_mut() else { return };
        // Ordinal_454's flag 0x200: the video is played at double size into the 0x78 x 0x70 buffer.
        for y in 0..pic.h as usize {
            for x in 0..pic.w as usize {
                if x / 2 < vw && y / 2 < vh {
                    pic.px[y * pic.w as usize + x] = t[v.pixels[y / 2 * vw + x / 2] as usize];
                }
            }
        }
    }

    // ---- the turn exchange (single player provider) ----

    /// 0x485bd0 (ECX = the bytes): a command joins the next turn.
    fn net_485bd0(&mut self, cmd: &[u8]) {
        if (self.setup.g654aa0 as usize) + cmd.len() > 0x200 {
            todo_port!("0x485a40", "the command buffer is full: send it first");
        }
        self.ready.net.buf.extend_from_slice(cmd);
        self.setup.g654aa0 += cmd.len() as u32;
    }

    /// 0x4c3d20 (`leave`): the turn exchange ends: the tables clear and the game is left (Storm's
    /// SNetLeaveGame has nothing to leave with the single player provider).
    fn net_4c3d20(&mut self, _leave: u32) {
        self.setup.g654a80 = [0; 8];
        self.setup.g6556e4 = 0;
        self.setup.g6554b0 = 0;
        self.setup.g6556e0 = 0;
        self.setup.g57eebc = 0;
        self.setup.g654aa0 = 0;
        self.ready.net.buf.clear();
        if self.setup.multiplayer {
            todo_port!("0x4c3d20", "Sleep(1000) and Ordinal_119 (leave the network game)");
        }
    }

    /// 0x485aa0: turns are sent until the provider holds its latency's worth (1 for the single
    /// player provider); an empty buffer sends the no-op command 5.
    fn net_485aa0(&mut self) {
        if self.setup.net_provider != 0 {
            todo_port!("0x485aa0", "turns of a network provider (Storm's SNetSendTurn)");
            return;
        }
        while self.ready.net.turns.is_empty() {
            if self.setup.g654aa0 == 0 {
                self.ready.net.buf = vec![5];
                self.setup.g654aa0 = 1;
            }
            let turn = std::mem::take(&mut self.ready.net.buf);
            self.ready.net.turns.push_back(turn);
            self.setup.g654aa0 = 0;
        }
    }

    /// 0x4bb530 (Ordinal_122, SNetReceiveTurns): the oldest turn comes back; the connected player's
    /// flags are 0x10000, and 0x20000 with a turn.
    fn net_4bb530(&mut self) -> bool {
        let me = (self.setup.local_storm_id as usize).min(7);
        self.ready.net.flags[me] = 0x10000;
        match self.ready.net.turns.pop_front() {
            Some(t) => {
                self.ready.net.data[me] = t;
                self.ready.net.flags[me] |= 0x20000;
                true
            }
            None => false,
        }
    }

    /// 0x486580: receive; true when a turn came or the last one was less than 2.5 s ago.
    fn net_486580(&mut self) -> bool {
        let now = self.clock_ms;
        if self.net_4bb530() {
            self.ready.last_rx = now;
            self.ready.net.rx = true;
            return true;
        }
        self.ready.net.rx = false;
        now.wrapping_sub(self.ready.last_rx) < 0x9c4
    }

    /// 0x486530: the commands of every player whose turn arrived run (0x486040: 5 is a no-op,
    /// 0x54 marks the player ready).
    fn net_486530(&mut self) {
        for p in (0..8).rev() {
            if self.ready.net.flags[p] & 0x20000 == 0 {
                continue;
            }
            let data = self.ready.net.data[p].clone();
            let mut i = 0;
            while i < data.len() {
                match data[i] {
                    5 => i += 1,
                    0x54 => {
                        self.ready.net.ready[p] = true;
                        i += 1;
                    }
                    _ => {
                        todo_port!("0x486040", "the turn's other commands");
                        break;
                    }
                }
            }
        }
    }

    /// 0x487070: receive, then (a turn came) send the next one and run the commands.
    fn net_487070(&mut self) {
        if self.screen == screen::EXIT {
            return;
        }
        let got = self.net_486580();
        if !got && self.setup.g6556e0 != 0 {
            todo_port!("0x4a3380", "network timeout handling");
        }
        if self.ready.net.rx && self.screen != screen::EXIT {
            self.net_485aa0();
            if self.screen != screen::EXIT {
                if self.setup.g6556e0 == 0 {
                    self.net_486530();
                } else {
                    todo_port!("0x486f50", "replay / observer turns");
                }
                if self.screen != screen::EXIT {
                    self.setup.g57eebc = self.setup.g57eebc.wrapping_add(1);
                    if self.setup.g6556e0 != 0 {
                        todo_port!("0x47cfc0", "turn bookkeeping of a network game");
                    }
                }
            }
        }
    }

    /// 0x485820: with 0x6554b0 == 2, 1 once every connected player is ready (0x6554b0 = 0).
    fn net_485820(&mut self) -> bool {
        if self.setup.g6554b0 == 2 {
            if (0..8).any(|p| self.ready.net.flags[p] & 0x10000 != 0 && !self.ready.net.ready[p]) {
                return false;
            }
            self.setup.g6554b0 = 0;
            return true;
        }
        false
    }

    /// 0x487250: the poll (at most every 250 ms): (false when the connection is gone, whether
    /// everyone is ready).
    fn net_487250(&mut self) -> (bool, bool) {
        let now = self.clock_ms;
        if now.wrapping_sub(self.ready.last_poll) < 0xfa {
            return (true, false);
        }
        self.net_487070();
        if self.ready.net.rx {
            self.ready.last_poll = now;
        }
        let all = self.net_485820();
        (self.screen != screen::EXIT, all)
    }

    /// 0x46d340 (the dialog's idle proc): the dialog ends with 100 when the connection is gone,
    /// with 0x65 when everyone is ready.
    fn ready_46d340(&mut self) {
        let (ok, all) = self.net_487250();
        if !ok {
            self.activated = 100;
        } else if !all {
            return;
        } else {
            self.activated = 0x65;
        }
        self.ready_46d160();
        self.ready.poll = false;
    }

    // ---- time ----

    /// One step of the dialog's loop: the portraits' videos play on, the due timers run, then the
    /// idle proc.
    pub(super) fn ready_tick(&mut self, files: &Storage, ms: u32) {
        if !matches!(self.dlg, Some((_, Open::Ready(_)))) {
            return;
        }
        for st in self.ready.ports.iter_mut() {
            if let Some(v) = st.video.as_mut().filter(|_| !st.ended) {
                st.elapsed += ms;
                while st.elapsed >= v.ms.max(1) {
                    st.elapsed -= v.ms.max(1);
                    if v.next_frame() {
                        st.fresh = true;
                    } else {
                        st.ended = true;
                        break;
                    }
                }
            }
        }
        let mut due = Vec::new();
        for t in self.ready.timers.iter_mut() {
            if ms >= t.left {
                t.left = t.interval;
                due.push((t.owner, t.id, t.proc));
            } else {
                t.left -= ms;
            }
        }
        for (owner, id, proc) in due {
            if !self.ready.timers.iter().any(|t| t.owner == owner && t.id == id) {
                continue;
            }
            self.ready_timer(files, owner, id, proc);
        }
        if self.ready.poll {
            self.ready_46d340();
        }
    }

    /// A timer's proc (called with the dialog or control and the timer's id).
    fn ready_timer(&mut self, files: &Storage, owner: Owner, id: u16, proc: Proc) {
        match proc {
            // 0x46d1b0: the running trigger; the next one (or the end of the briefing) when done.
            Proc::Brief => {
                let Some(n) = self.ready.node else { return };
                if self.ready_427da0(files, n) {
                    if n + 1 < self.setup.briefing.len() {
                        self.ready.node = Some(n + 1);
                    } else {
                        self.ready.node = None;
                        if self.setup.multiplayer {
                            self.ready_46d090(false);
                        }
                    }
                }
            }
            Proc::EndSpeak => {
                self.ready.speaking = false;
                self.rtimer_kill(owner, id);
            }
            Proc::MsgEnd => {
                self.rshow(ids::MESSAGE, false);
                for k in [id, 2, 3] {
                    self.rtimer_kill(owner, k);
                }
            }
            Proc::MsgScrollStart => {
                let iv = self.ready.msg_interval;
                self.rtimer_set(owner, 2, iv, Proc::MsgScroll);
                self.rtimer_kill(owner, id);
            }
            Proc::MsgScroll => {
                if let Some(p) = self.rctl(ids::MESSAGE).and_then(|c| c.pic.as_mut()) {
                    p.scroll += 1;
                }
            }
            Proc::Port => {
                if let Some(c) = owner {
                    self.ready_44ee80(files, c, id);
                }
            }
        }
    }
}
