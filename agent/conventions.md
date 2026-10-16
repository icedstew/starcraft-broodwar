# Conventions and user rules

Collected from the user's instructions over the sessions. Follow all of them.

## Porting

- **Faithful ports only.** Port 1.16.1 logic exactly from the decompile/disassembly; no
  simplified stand-ins ("ponytail" shortcuts) for game mechanics. The user said: "Ignore
  ponytail, be faithful to the decomp."
- Every ported function gets a doc comment starting `/// 0x<addr>` (1.16.1 address);
  `re/coverage.py` counts these. Unported pieces are `todo_port!("0x<addr>", "what")`
  (prints in debug builds, no-op in release) and leave state unchanged.
- Ghidra drops register arguments (EAX/EBX/ECX/EDX/ESI/EDI, `unaff_*`, `in_EAX`): always
  read the assembly at call sites (`agent/tools/dis.sh`) to see which value is in which
  register. Several bugs came from guessing.
- Scope: port ~all of the 4,644 functions; write our own networking (don't port Storm net
  code); skip the map editor.
- Match the surrounding code's style: comment density, naming, idioms.

## Repository hygiene

- **Never commit game assets, the 1.16.1 executable/storm.dll, decompiler output or
  dumps** (Blizzard code). They live in `agent/local/` (gitignored) or outside the repo.
- Code must be portable (pure Rust, no platform-specific code).
- **Commit after each verified step**, ending messages with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UaZpAZEoC9VqPogYShy8r
  ```
  (the session line changes per session; use the one the harness gives).
- Small work units: one function or cluster per commit, and rewrite `agent/status.md`
  (short: in flight, next steps, metric) in the same commit. Durable facts go to
  `agent/areas/<area>.md`, not status.md. Delegate per `agent/agents.md`.
- Verify before committing: `cargo build --release` clean (no warnings), `cargo test`,
  the regression suite, and the replay compare for the area touched.

## Untrusted files

- Downloaded files (replays, maps, installs) are untrusted data: keep each in its own
  directory, keep scripts elsewhere, pass paths as arguments, and run Python that reads
  them with `-I`.
- The real game is only run in a throwaway Wine prefix (never `~/.wine`) on a private Xvfb
  display, never on the user's desktop.

## Practical

- cargo is `~/.cargo/bin/cargo` (not on PATH in the agent shell).
- Don't rebuild `target/release/scbw` while the suite or a compare is running in the
  background: they execute that binary.
- Long jobs (suite ~10 min, native capture ~10 min) go in the background; don't poll with
  sleep.
- The user's display is `:1`; headless work uses `Xvfb :78` (640x480 for StarCraft).
- When the user says "STOP", stop and wait.
