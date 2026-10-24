//! Restoring a snapshot for the rollback session (src/net.rs, agent/areas/net.md).

use super::State;

/// Output queues the front end drains after a frame; a rollback's re-simulation must not
/// add to them again (`out_lens` before, `truncate_outs` after).
macro_rules! outs {
    ($m:ident) => {
        $m!(ui_sounds, trigger_wavs, speech_mute, speech, unit_sounds, talked, portrait_dropped, transmissions, shown_text, notices, chat)
    };
}

impl State {
    /// Become `snap` (cloned: snapshots stay in their ring) but keep this state's
    /// viewer-local fields: the local player and replay flags, the UI selection and its
    /// helpers, error / alert / text queues, the local vision mask, the creep display tile
    /// and the outgoing turn. Not kept although they look UI-only: the UI image pools and
    /// `marker_thingy` / `marker_shown` (indices into the image / thingy pools the snapshot
    /// restores, and sprites in it link UI images, so they must stay consistent with it).
    pub fn restore_sim(&mut self, snap: &State) {
        let mut n = snap.clone();
        macro_rules! keep {
            ($($f:ident),* $(,)?) => { $( std::mem::swap(&mut n.$f, &mut self.$f); )* };
        }
        keep!(
            local_player, replay, replay_speed, replay_mult, replay_paused, replay_frames, selected,
            selection_emptied, key_alt, last_error, shown_error, center_view, center_view_end,
            alert_cooldown, alert_tick_counter, alert_pos, local_vision_mask, screen_tile, turn_out,
            sounds, minimap_pings
        );
        macro_rules! keep_outs {
            ($($f:ident),*) => { keep!($($f),*) };
        }
        outs!(keep_outs);
        *self = n;
    }

    /// Lengths of the output queues (see `outs!`).
    pub fn out_lens(&self) -> Vec<usize> {
        macro_rules! lens {
            ($($f:ident),*) => { vec![$(self.$f.len()),*] };
        }
        outs!(lens)
    }

    /// Cut the output queues back to `lens` (from `out_lens`).
    pub fn truncate_outs(&mut self, lens: &[usize]) {
        let mut it = lens.iter();
        macro_rules! cut {
            ($($f:ident),*) => { $( self.$f.truncate(*it.next().unwrap()); )* };
        }
        outs!(cut);
    }
}
