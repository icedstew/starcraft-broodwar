//! The command card's tooltip state (1.16.1 statcmd.cpp).

use crate::todo_port;

/// The tooltip globals: 0x68c1b4 (the button the tooltip is for, 0 = none), 0x68c1cc (the
/// cost icon count) and 0x68c1d0..0x68c1de (the eight cost slots).
#[derive(Default)]
#[allow(dead_code)]
pub struct CmdTip {
    pub button: u32,
    pub cost_count: u8,
    pub cost: [u16; 8],
}

#[allow(dead_code)]
impl CmdTip {
    /// 0x459360: hides the tooltip (0x481480) and clears the cost block.
    pub fn clear_cost(&mut self) {
        todo_port!("0x481480", "hide the tooltip");
        self.cost = [0; 8];
        self.cost_count = 0;
    }

    /// 0x4597c0: the selection update for the candidate button `esi` (its +0x32 value is
    /// `value`); `current` = 0x418340 (the control under the cursor), `paused` = 0x6509c4.
    /// Returns the argument for 0x4593a0 (the tooltip build) when the button changed.
    pub fn update_for(&mut self, esi: u32, value: u32, current: u32, paused: bool) -> Option<u32> {
        if paused {
            self.button = 0;
            self.clear_cost();
            return None;
        }
        if esi != current {
            self.button = 0;
            self.clear_cost();
            return None;
        }
        if esi != self.button {
            self.button = esi;
            return Some(value);
        }
        None
    }

    /// 0x459860: the same with the button taken from 0x418340 (`current`, or 0 when
    /// `paused`); `arg` = EAX, returned for 0x4593a0 when the button changed.
    pub fn update(&mut self, current: u32, paused: bool, arg: u32) -> Option<u32> {
        let b = if paused { 0 } else { current };
        if b != self.button {
            self.button = b;
            return Some(arg);
        }
        None
    }
}

/// 0x459fb0: true when the text after the last '.' of `name` is scx, scm or rep (any case).
#[allow(dead_code)]
pub fn is_map_or_replay(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| ["scx", "scm", "rep"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}
