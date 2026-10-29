//! Campaign mode (ours, not 1.16.1): a Risk-style world whose borders are live enhanced
//! games. agent/plans/campaign.md.

#[cfg(not(target_arch = "wasm32"))]
pub mod host;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;
#[cfg(not(target_arch = "wasm32"))]
pub mod web;
pub mod world;
