//! The triggers (victory.cpp).

use super::{In, Out};
use crate::bw::triggers::{Trigger, TRIGGER_SIZE};
use crate::bw::{Data, State};

impl State {
    /// 0x4899d0: the trigger count (u16), then packed: 0x6509a4 / 0x6509a8 (who is in the
    /// game), each player's trigger count (u16 x 8) and every player's triggers' 0x960
    /// bytes (the list node's links left out); then packed the 0x500 bytes of 0x596cd8.
    pub(super) fn save_trig_state(&mut self, _d: &Data, o: &mut Out) -> bool {
        let t = &self.trig;
        let total: usize = t.lists.iter().map(|l| l.len()).sum();
        let mut b = Vec::with_capacity((total & 0xffff) * TRIGGER_SIZE + 0x18);
        b.extend_from_slice(&t.in_game);
        for l in &t.lists {
            b.extend_from_slice(&(l.len() as u16).to_le_bytes());
        }
        for l in &t.lists {
            for tr in l {
                b.extend_from_slice(&tr.0[..]);
            }
        }
        o.u16(total as u16);
        // The game sizes the buffer from the u16 count.
        b.resize((total & 0xffff) * TRIGGER_SIZE + 0x18, 0);
        if !o.packed(&b) {
            return false;
        }
        let mut p = Vec::with_capacity(0x500);
        for u in &t.unit_properties {
            p.extend_from_slice(u);
        }
        p.resize(0x500, 0);
        o.packed(&p)
    }

    /// 0x4897e0: reads what 0x4899d0 wrote; the lists are rebuilt in order (0x402330 empties
    /// them first only on the way, the game appends each trigger).
    pub(super) fn load_trig_state(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(n) = i.u16() else { return false };
        let Some(b) = i.packed(n as usize * TRIGGER_SIZE + 0x18) else { return false };
        let t = &mut self.trig;
        t.in_game.copy_from_slice(&b[0..8]);
        let mut pos = 0x18;
        for p in 0..8 {
            t.lists[p].clear(); // 0x402330
            let c = u16::from_le_bytes([b[8 + p * 2], b[9 + p * 2]]);
            for _ in 0..c {
                let Some(r) = b.get(pos..pos + TRIGGER_SIZE) else { return false };
                t.lists[p].push(Trigger::from_bytes(r));
                pos += TRIGGER_SIZE;
            }
        }
        let Some(p) = i.packed(0x500) else { return false };
        t.unit_properties = p.chunks_exact(0x14).map(|c| c.try_into().unwrap()).collect();
        true
    }
}
