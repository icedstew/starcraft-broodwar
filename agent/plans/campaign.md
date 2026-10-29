# Campaign mode: plan

Draft 2026-10-28 from a Q&A with the user. Our own feature on top of enhanced games
(agent/plans/enhanced.md: late join, shared armies, AI takeover, fast drops). Not 1.16.1
behavior: module docs say it is ours, no fake `0x...` addresses; cite the 1.16.1 functions
we reuse.

## What the user wants

A Risk-style world. Areas owned by factions; each border between two areas is a StarCraft
map. Every border between two different factions has a live match. Matches without humans
are played by the AI. Humans drop into any match of their faction at any time, several
humans share one side, leave whenever (AI takes over). A web map shows all areas with a
square on each live border: stats of that match and a Join button. Last faction standing
wins.

## Decisions (user, 2026-10-28)

| Topic | Decision |
|---|---|
| Match result | Winner (either side) takes the loser's area of that border. |
| Lost area | Every other live match on that area's borders ends (cancelled); new borders get new matches. |
| Match start | Every border between two factions is always live. A new one starts at once. |
| Time | Continuous real time; every match at normal speed (Fastest), AI-only ones too. |
| Hosting | One headless host process runs every match and is the star hub for all. No servers of ours. |
| Factions | Default: 3 factions = the 3 races. Supported: 2..8 factions, race fixed per faction at campaign creation. |
| Side size | One army per side (1v1 start on any 2+ player map), any number of humans share it. Later: more. |
| Carryover | None: fresh melee every match. Leave room for area bonuses (start extras) later; do not build it now. |
| Draw | Draw timer (game time): reset by any unit/building death or any mineral/gas mined. Warnings to both sides each minute from 5 to 9 min idle; draw at 10 min (14400 frames at Fastest). Draw -> fresh match on the same border at once. |
| Identity | Name only, unique per campaign. First join with a new name picks a faction; the host keeps name -> faction; that name joins only that faction's side. Reset each campaign. |
| World | Hand-written world file (areas, positions, borders, map per border, optional start owners) and a generator that writes one. No start owners -> generator-style contiguous blocks. Default world ~30 areas (~40 borders, ~20 live matches). |
| Map UI | Web page served by the host. Join from it in the browser (wasm build) or with the .exe via a launch code. |
| Border square | Players per side, army strength (supply / workers / army value), economy and time (mined, game time, draw timer), live minimap. A viewer sees only their own faction's side and vision; the enemy side is hidden. |
| After a match | Back to the web map; the .exe also gets an in-game list of the faction's live matches. |
| Eliminated faction | Host setting: pick a surviving faction / out (spectate later). |
| Spectating | Later. |
| Chat | Faction chat on the web map, also shown in-game in every match of that faction. |
| Host controls | Pause/resume campaign, kick (drop, may rejoin) and ban (drop, name refused for the rest of the campaign), rename, force-end a border (winner or draw). |
| Endpoint | One QUIC port for every match; the match id rides in the transport hello. |
| Map pool | Configurable: the world file / generator takes a list of maps or directories. |
| Colours | Fixed per faction in every match (not dealt). |

## Facts from the code that shape this

- Per-process state in net / glue / view is `thread_local!` (snet `NET`, view `NET_DRIVER`,
  `SAVE_EXPANSION`, live `REPORT_NEXT`): one match per thread in one process is the natural
  unit. Check the `static`s in bw/requirements.rs, bw/creep.rs, game/flingy.rs, mpq.rs are
  read-only tables before relying on it.
- Match end: 1.16.1's per-player result 0x58d700 (2 defeated, 3 victorious; triggers 0x650974
  has 5 = draw). Melee victory already ends an enhanced game; the runner just reads it.
- Mining: 1.16.1's score counters (minerals / gas gathered per player) feed the draw timer
  and the stats; deaths: the kill / death score counters. No new sim hooks if the counters
  are enough (compare per frame, deterministic).
- Today's enhanced host is a playing peer with a window or a replay player. A campaign match
  needs a host that controls no army (a "seat" with a human id, no army, empty turns).
- No HTTP server, no serde in Cargo.toml. Web page: a small std `TcpListener` HTTP server
  (static page + JSON polled every ~2 s) is enough; world file: a line format we parse
  ourselves, no new crates.
- Joining a match is the existing late join (save of the join frame, C_LATE), ~180 KB, ~2.5 s.

## Tasks, in order

Rules as in enhanced.md: both targets build with no warnings, regression set before each
commit, one commit per task, this file updated in the same commit.

### 1. Campaign model (pure, no net) — `src/campaign/world.rs` (DONE 2026-10-28)
World file parse/write; areas (id, name, x, y, owner), borders (a, b, map path), factions
(name, race, colour), map pool (files / directories). Functions: `live_borders()`, `apply(result)` (capture -> list of
borders to cancel and to start), elimination, campaign winner. Unit tests on a small world
(capture, cancel siblings, elimination, win).
Done: `World::parse / write / deal / live_borders / apply / winner`, 4 tests, `scbw campaign
check <world>` (prints factions and live borders). A 6x5 grid deals 10/10/10 with 13 live
borders; small odd graphs can deal unevenly (3x3: 4/3/2).
All campaign work goes on branch `campaign` (user, 2026-10-28).

### 2. Draw timer in the sim — `src/bw/enhanced.rs` (DONE 2026-10-28)
`State.campaign: bool` from `GameSetup`. Each frame: if any death / gather counter changed,
reset `idle_frames`; at 5..9 min push a notice to all players ("No fighting or mining: draw
in N minutes"); at 10 min set every player's result to draw (5) and end the game. Checks:
replays and the suite unaffected (flag off); a headless AI-vs-AI run that idles hits it.
Done: `State.campaign / idle_frames / activity`, `State::campaign_tick` after each trigger pass
(view.rs `Scene::step`); activity = men + buildings lost + minerals + gas gathered, every
player. A minute = 60000 / 42 = 1428 frames (Fastest). Warnings `NOTICE_DRAW` (0x1002, our
text "No fighting or mining: the game is a draw in N minutes.") at 5..9 idle minutes; at 10,
every in-game human / computer gets `player_left` 5 and the end-screen timer 0x2d (as a
trigger draw 0x4c5070 settled by 0x48a200). Carried to joiners in `Extras` (campaign,
idle_frames). Checked: unit test `bw::enhanced::tests::draw_timer` (warnings 5..1, reset by
mining and by a death, draw at 10 min, nothing when off); full regression set passed (flag
off: replays / normal games unchanged).

### 3. Headless match runner — `src/campaign/match.rs`
One thread per match: load map, enhanced + campaign setup, two armies both AI from frame 0,
host seat with no army, Fastest pacing, accepts late joins on its endpoint. Every ~1 s sends
`MatchStats` to the campaign thread (per side: humans, supply, workers, army value, mined,
game time, draw timer; minimap bitmap per side from that side's vision). Ends with
`Result {winner side | draw | cancelled}`. Check: two runners side by side in one process,
hashes match a joiner's.

### 4. Campaign host — `scbw campaign host <world> [--resume <dir>]`
Owns the world; starts/kills runners per `live_borders`, applies results, restarts draws.
Persistence: every ~2 min write campaign file (ownership, names -> faction, settings) + each
live match's 1.16.1 save (the late-join save path); `--resume` loads them. Pause/resume:
pause every match (1.16.1 pause command from the host seat).

### 5. Join routing and identity
Launch code = host address + cert hash + match id + name. One QUIC endpoint for all
matches: the match id in the hello routes the link to that match's thread. Name registry:
unique names; first join picks faction; `enhanced_pick` restricted to the faction's army.
Kick (drop the link; may rejoin), ban (drop + name refused until the campaign ends; ban
list saved with the campaign), rename. Faction colour forced into the match setup (not
dealt).

### 6. Web map — host serves `campaign.html` + `/state.json?name=...`
Areas and borders drawn in SVG, squares on live borders, panel with the match stats filtered
to the viewer's faction (filtered on the host, not in the page). Join: browser -> the wasm
build with `?join=<code>`; .exe -> copy code (`scbw join <code>`); `scbw://` handler later.
Admin view behind a host token: pause, kick/rename, force-end.

### 7. In-game pieces (.exe)
After a match: list of the faction's live matches (our dialog), pick -> join. Faction chat:
web chat relayed by the host into every match of that faction as chat lines, and in-game
chat to "faction" sent back out.

### 8. World generator — `scbw campaign gen <areas> <factions> <map dir> > world.txt`
Planar graph (points + Delaunay-ish neighbours, or a grid with jitter), maps from a pool
(e.g. mappacks/iccup 2-player-capable), contiguous start blocks per faction.

### Later (not now)
Area bonuses (start extras per owned areas), spectating with faction vision, several armies
per side, `scbw://` URL handler.

## Open questions

- Stats bandwidth: minimap per side per match every ~2 s to every web viewer; fine at 20
  matches, measure.
- Name-only identity lets anyone claim an enemy name and see their faction's stats; OK for
  friends, kick / ban / rename cover abuse.
- Map pool contents: maps must load in 1.16.1 (910map SC:R-era maps may not); check each
  pool map loads when the campaign starts and skip the rest.
