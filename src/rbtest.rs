//! The rollback oracle (agent/areas/net.md): a replay played twice, straight and with every
//! player but the first seeing the others' commands `k` frames late. Missing commands are
//! predicted as none; when they arrive, the game goes back to the state before their frame
//! and plays forward again. Both games must end in the same state.

use crate::{bw, replay, view};
use web_time::Instant;

/// `a` is ready for the replay (`view::prepare_replay`). Returns the result line: MATCH or
/// DIFF, with the time of the straight game, the snapshots and the re-simulated frames.
pub fn run(a: &view::Assets, r: &replay::Replay, k: u32, end: u32) -> std::io::Result<String> {
    let end = end.min(r.header.frames);
    // The first human / computer slot, as ReplayPlay::new's follow.
    let local = r.header.slots.iter().position(|s| s.kind == 1 || s.kind == 2).unwrap_or(0) as u8;
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); end as usize];
    for (i, c) in r.commands.iter().enumerate() {
        if let Some(v) = at.get_mut(c.frame as usize) {
            v.push(i);
        }
    }
    // Frame `g`'s commands as known at frame `now`: the local player's at once, the
    // others' from `g + k` on.
    let run = |scene: &mut view::Scene, g: u32, now: u32| {
        for &i in &at[g as usize] {
            let c = &r.commands[i];
            if c.owner == local || g + k <= now {
                scene.replay_command(c.owner, &c.data);
            }
        }
        scene.step();
    };
    let mut sa = view::Scene::from_replay(a, r)?;
    // The same start: only a process's first game draws the music variant (0x4ee2d0).
    let mut sb = view::Scene::from_replay(a, r)?;
    sb.st = sa.st.clone();
    let t0 = Instant::now();
    for f in 0..end {
        run(&mut sa, f, u32::MAX);
    }
    let straight_ms = t0.elapsed().as_millis();
    // snaps[f % (k + 1)]: the state before frame f's commands.
    let mut snaps: Vec<Option<bw::State>> = vec![None; k as usize + 1];
    let (mut rollbacks, mut resim, mut snap_us, mut resim_us, mut snaps_taken) = (0u32, 0u32, 0u128, 0u128, 0u32);
    let t0 = Instant::now();
    for now in 0..end + k {
        // The others' commands of frame now - k arrive; any = a misprediction.
        if let Some(g) = now.checked_sub(k).filter(|&g| g < end) {
            if k > 0 && at[g as usize].iter().any(|&i| r.commands[i].owner != local) {
                let t = Instant::now();
                sb.st = snaps[(g % (k + 1)) as usize].clone().expect("snapshot");
                rollbacks += 1;
                for h in g..now.min(end) {
                    snaps[(h % (k + 1)) as usize] = Some(sb.st.clone());
                    snaps_taken += 1;
                    run(&mut sb, h, now);
                    resim += 1;
                }
                resim_us += t.elapsed().as_micros();
            }
        }
        if now < end {
            let t = Instant::now();
            snaps[(now % (k + 1)) as usize] = Some(sb.st.clone());
            snap_us += t.elapsed().as_micros();
            snaps_taken += 1;
            run(&mut sb, now, now);
        }
    }
    let ms = t0.elapsed().as_millis();
    let stats = format!(
        "f{end} delay {k}: straight {straight_ms} ms ({:.3} ms/frame), rollback game {ms} ms: {rollbacks} rollbacks, {resim} frames again ({:.3} ms per rollback), snapshot {:.3} ms ({snaps_taken} taken)",
        straight_ms as f64 / end.max(1) as f64,
        resim_us as f64 / 1000.0 / rollbacks.max(1) as f64,
        snap_us as f64 / 1000.0 / end.max(1) as f64,
    );
    let hashes = format!("hash {:016x} / {:016x}", sa.st.state_hash(), sb.st.state_hash());
    let (da, db) = (format!("{:#?}", sa.st), format!("{:#?}", sb.st));
    Ok(match da.lines().zip(db.lines()).position(|(x, y)| x != y) {
        None if da.len() == db.len() => format!("MATCH {stats}, {hashes}"),
        p => {
            let p = p.unwrap_or(0);
            #[cfg(not(target_arch = "wasm32"))]
            if let Ok(o) = std::env::var("SCBW_RBTEST_DUMP") {
                std::fs::write(format!("{o}.a"), &da)?;
                std::fs::write(format!("{o}.b"), &db)?;
            }
            format!("DIFF {stats} at dump line {p}\n  straight {:?}\n  rollback {:?}\n  {hashes}", da.lines().nth(p), db.lines().nth(p))
        }
    })
}
