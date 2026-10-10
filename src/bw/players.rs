//! Per-player game tables (1.16.1 addresses).

#[derive(Clone, Debug)]
pub struct Players {
    /// 0x58d2b0: upgrade levels 0..46, [player][upgrade].
    pub upgrade_sc: [[u8; 46]; 12],
    /// 0x58f32c: Brood War upgrade levels 46..61.
    pub upgrade_bw: [[u8; 15]; 12],
    /// 0x58f3e0: upgrade-in-progress bits, 8 bytes per player.
    pub upgrade_in_progress: [[u8; 8]; 12],
    /// 0x582324: unit counts [unit type][player].
    pub unit_counts: Vec<[i32; 12]>,
    /// 0x582174 / 0x582204 / 0x582294: supply used by zerg / terran / protoss units.
    pub supply_used: [[i32; 12]; 3],
    /// 0x582084: factory count, 0x581f34: building count, 0x581de4: men count.
    pub factories: [i32; 12],
    pub buildings: [i32; 12],
    pub men: [i32; 12],
}

impl Default for Players {
    fn default() -> Players {
        Players {
            upgrade_sc: [[0; 46]; 12],
            upgrade_bw: [[0; 15]; 12],
            upgrade_in_progress: [[0; 8]; 12],
            unit_counts: vec![[0; 12]; 228],
            supply_used: [[0; 12]; 3],
            factories: [0; 12],
            buildings: [0; 12],
            men: [0; 12],
        }
    }
}

impl Players {
    pub fn upgrade_level(&self, player: u8, upgrade: usize) -> u8 {
        let p = player as usize;
        if upgrade < 46 {
            self.upgrade_sc[p][upgrade]
        } else {
            self.upgrade_bw[p][upgrade - 46]
        }
    }
}
