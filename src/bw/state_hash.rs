//! A hash of the synced simulation state, for comparing two simulations (rollback netcode,
//! agent/areas/net.md).

use super::list::NIL;
use super::State;

/// Cap on the entries one list is walked for, against corrupt links.
const MAX_LIST_ENTRIES: u32 = 0x10000;

/// FxHasher64-style mixer over fixed-width little-endian words (never usize, so wasm32 and
/// 64-bit native agree).
struct Mix(u64);

impl Mix {
    fn w(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517cc1b727220a95);
    }
    fn u8(&mut self, v: u8) {
        self.w(v as u64)
    }
    fn u16(&mut self, v: u16) {
        self.w(v as u64)
    }
    fn i16(&mut self, v: i16) {
        self.w(v as u16 as u64)
    }
    fn u32(&mut self, v: u32) {
        self.w(v as u64)
    }
    fn i32(&mut self, v: i32) {
        self.w(v as u32 as u64)
    }
}

impl State {
    /// A unit by slot index and unique id (0x47b1d0's handle), 0 for none; not an address.
    fn hash_unit_ref(&self, u: u32) -> u32 {
        match self.units.get(u as usize) {
            Some(_) if u != NIL => self.unit_handle(u),
            _ => 0,
        }
    }

    fn hash_flingy(&self, m: &mut Mix, hp: i32, fl: &crate::game::flingy::Flingy, move_unit: u32) {
        m.i32(hp);
        m.i32(fl.exact.0);
        m.i32(fl.exact.1);
        m.i32(fl.speed);
        m.u8(fl.heading);
        m.u8(fl.velocity_dir);
        m.i16(fl.move_target.0);
        m.i16(fl.move_target.1);
        m.u32(self.hash_unit_ref(move_unit));
    }

    /// Hash of the synced simulation state, after ShieldBattery's `state_hash.rs` (MIT,
    /// game/src/rollback/): frame count, RNG seed, every player's minerals / gas, supplies
    /// (provided / used / max per race), upgrade and tech levels, then every unit of the
    /// visible and hidden lists in list order (hp, exact position, speed, facing / movement
    /// direction, move target, unique id, type, player, shields, energy, flags, order, state,
    /// timer, secondary order + state, cooldowns, build time and queue, order target,
    /// subunit) and the active bullets (player, state, timers, weapon, flags, bounces,
    /// target, parent). Other units are identified by slot and unique id. Nothing
    /// viewer-local (camera, local player, vision, selection, sounds) is included.
    pub fn state_hash(&self) -> u64 {
        let mut m = Mix(0);
        m.u32(self.frame);
        m.u32(self.rng.seed);
        let p = &self.players;
        for v in [&p.minerals, &p.gas] {
            for &x in v {
                m.i32(x);
            }
        }
        for r in 0..3 {
            for t in [&p.supply_provided, &p.supply_used, &p.supply_max] {
                for &x in &t[r] {
                    m.i32(x);
                }
            }
        }
        for pl in 0..12 {
            for &x in p.upgrade_sc[pl].iter().chain(&p.upgrade_bw[pl]).chain(&p.tech_researched[pl]) {
                m.u8(x);
            }
        }
        for first in [self.visible_units.head, self.hidden_units.head] {
            let (mut u, mut n) = (first, 0u32);
            while u != NIL && n < MAX_LIST_ENTRIES {
                let Some(un) = self.units.get(u as usize) else { break };
                self.hash_flingy(&mut m, un.hp, &un.fl, un.move_target_unit);
                m.u8(un.unique_id);
                m.u16(un.unit_type);
                m.u8(un.player);
                m.i32(un.shields);
                m.u16(un.energy);
                m.u32(un.status);
                m.u8(un.order);
                m.u8(un.order_state);
                m.u8(un.main_order_timer);
                m.u8(un.secondary_order);
                m.u8(un.secondary_order_state);
                m.u8(un.ground_cooldown);
                m.u8(un.air_cooldown);
                m.u8(un.spell_cooldown);
                m.u16(un.remaining_build_time);
                for q in un.build_queue {
                    m.u16(q);
                }
                m.i16(un.order_target_pos.0);
                m.i16(un.order_target_pos.1);
                m.u32(self.hash_unit_ref(un.order_target_unit));
                m.u32(self.hash_unit_ref(un.subunit));
                u = un.link.next;
                n += 1;
            }
            m.u32(n);
        }
        let (mut b, mut n) = (self.bullets.active.head, 0u32);
        while b != NIL && n < MAX_LIST_ENTRIES {
            let Some(bu) = self.bullets.pool.get(b as usize) else { break };
            self.hash_flingy(&mut m, bu.hp, &bu.fl, bu.move_target_unit);
            m.u8(bu.player);
            m.u8(bu.state);
            m.u8(bu.order_state);
            m.u8(bu.weapon);
            m.u8(bu.remaining_time);
            m.u8(bu.flags);
            m.u8(bu.bounces);
            m.i16(bu.target_pos.0);
            m.i16(bu.target_pos.1);
            m.u32(self.hash_unit_ref(bu.target));
            m.u32(self.hash_unit_ref(bu.source));
            b = bu.link.next;
            n += 1;
        }
        m.u32(n);
        m.0
    }
}
