//! Storm's SNet calls as the 1.16.1 glue and game make them, over our own networking
//! (agent/areas/net.md, phase 3c). Storm.dll exports them by ordinal only; each function
//! notes the ordinal and the call sites it was read from. One provider exists: 'UDPN'
//! ("Local Area Network (UDP)"), backed by the QUIC transport (transport.rs); Battle.net,
//! IPX, modem and direct cable are not offered.

use std::cell::RefCell;

/// 'UDPN': the provider id 0x4bade0 / 0x4d3cc0 receive (EBX).
pub const UDPN: u32 = 0x5544_504e;

/// A provider as Ordinal 105's callback (0x4ba610) gets it: `id`, its name and description
/// (both <= 0x80 bytes) and its caps (+4 flags, +0x10 the most players).
pub struct Provider {
    pub id: u32,
    pub name: &'static str,
    pub desc: &'static str,
    pub flags: u32,
    pub max_players: u32,
}

/// Storm's provider caps (Ordinal 114 fills the 0x24-byte struct 0x4d3cc0 reads).
#[derive(Clone, Copy)]
pub struct Caps {
    /// +0x08: the most bytes of a turn (0x57f0d8 = min(this, 0x200); < 0x80 is refused).
    pub max_message: u32,
    /// +0x1c: turns per second (0x4d92a0's L).
    pub turns_per_sec: u32,
    /// +0x20: turns in flight (0x57f090, clamped to 2..=8).
    pub turns_in_flight: u32,
}

const UDPN_CAPS: Caps = Caps { max_message: 0x200, turns_per_sec: 8, turns_in_flight: 2 };

#[derive(Default)]
struct Net {
    /// The initialized provider (0 none).
    provider: u32,
}

thread_local! {
    static NET: RefCell<Net> = RefCell::new(Net::default());
}

/// Ordinal 105 (0x4bab10 with the callback 0x4ba610): the providers to list.
pub fn enum_providers() -> Vec<Provider> {
    vec![Provider { id: UDPN, name: "Local Area Network (UDP)", desc: "", flags: 0, max_players: 8 }]
}

/// Ordinal 117 (0x4d3cc0): make `id` the provider; false for one we do not have.
pub fn initialize_provider(id: u32) -> bool {
    if !enum_providers().iter().any(|p| p.id == id) {
        return false;
    }
    NET.with(|n| n.borrow_mut().provider = id);
    true
}

/// Ordinal 114: the caps of the initialized provider.
pub fn provider_caps() -> Option<Caps> {
    NET.with(|n| (n.borrow().provider == UDPN).then_some(UDPN_CAPS))
}

/// Ordinal 102 (0x4bab10, 0x4d3cc0's failure path): leave any game and drop the provider.
pub fn destroy() {
    NET.with(|n| *n.borrow_mut() = Net::default());
}
