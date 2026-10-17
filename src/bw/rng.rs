//! Synchronized RNG (0x14039aa10 `sync_rand(call_site)`).

#[derive(Clone, Debug)]
pub struct Rng {
    /// 0x1410936a0
    pub seed: u32,
    /// 0x1410936ac: random numbers are only drawn while the game logic runs.
    pub enabled: bool,
    /// 0x1410936a8 total draws, 0x1410936b0 per call-site counters (desync diagnostics).
    pub total: u32,
    pub per_site: Vec<u32>,
    /// Debug (SCBW_RNGLOG_AT): print each draw's site.
    pub log: bool,
}

impl Default for Rng {
    fn default() -> Rng {
        Rng { seed: 0, enabled: false, total: 0, per_site: vec![0; 256], log: false }
    }
}

impl Rng {
    /// 0x4dc540 set_rng_enabled: set the flag, return the old one.
    pub fn set_enabled(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.enabled, on)
    }

    /// 0x4dc4a0 sync_rand: returns 0 without advancing when disabled; otherwise the next
    /// 15-bit value.
    pub fn rand(&mut self, site: u32) -> u32 {
        if !self.enabled {
            return 0;
        }
        if let Some(c) = self.per_site.get_mut(site as usize) {
            *c += 1;
        }
        self.total += 1;
        if self.log {
            eprintln!("rng site {site:#x}");
        }
        self.seed = self.seed.wrapping_mul(0x015A_4E35).wrapping_add(1);
        (self.seed >> 16) & 0x7FFF
    }

    /// 1.16.1 0x4dc550: uniform in [lo, hi]; returns lo without advancing when disabled.
    pub fn rand_between(&mut self, site: u32, lo: i32, hi: i32) -> i32 {
        if !self.enabled {
            return lo;
        }
        let r = self.rand(site) as i32;
        (((hi - lo) + 1) * r >> 15) + lo
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lcg() {
        let mut r = super::Rng { enabled: true, ..Default::default() };
        assert_eq!(r.rand(0), 0);
        assert_eq!(r.rand(0), 0x015A);
        let mut off = super::Rng::default();
        assert_eq!(off.rand(0), 0);
        assert_eq!(off.seed, 0);
    }
}
