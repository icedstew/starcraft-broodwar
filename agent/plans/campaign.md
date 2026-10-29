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

### 3. Headless match runner — `src/campaign/runner.rs` (DONE 2026-10-28)
Done: `runner::run` (a thread per match): melee setup on the map's first two start slots
(both computers, faction name / race / colour, own force each, template +0xf = 0: 0x4c3ed0
would otherwise ally two computers and the match ended at frame 2 in a draw), the host plays
the first empty slot (named "Faction") and sends only join / pause commands; `ai_taken` holds
both armies so `enhanced_pick` gives joiners one. Paced at 42 ms a frame. Reports: code,
`Stats` every second (humans, supply, workers, minerals / gas, mined, frame, idle), end
(`Won(side)` / `Draw` / `Stopped` / `Failed`, from 0x58d700).
Found and fixed on the way:
- A multiplayer save refuses a game with no human slot (0x4d02d0's header text); an
  all-computer enhanced game lists its computers there instead (view.rs `save_file`), or no
  join could ever happen.
- A joiner's start state (~160 KB) took 13.7 s at real-time pace (one 500-byte control part
  a frame): `transport.rs flush` now sends up to `CTL_WINDOW` = 16 control datagrams per
  flush; 1.1 s now. Applies to every game (regression set passed).
Check: `agent/tools/campaign_match.sh` (two matches in one process, a joiner without maps
takes an army: host and joiner hash equal at frame 900).

### 4. Campaign host — `scbw campaign host <world> [dir] [port] [web port]` (DONE 2026-10-28)
Done: `src/campaign/host.rs`. Starts a runner per live border (map from the border or a
random pool map; pool directories are walked recursively; a map that fails goes on a bad
list, a border retries 3 times), applies results (`World::apply`: capture, cancel, start;
elimination and campaign win printed), restarts draws. `dir` (default `campaign/`):
`world.txt`, `players.txt`, `border-N.sav` + `.txt` every 2 min and at quit; a host started
on a dir that has `world.txt` resumes: saved matches load (`Scene::from_save_steps`,
`State::campaign_resume`: every army to the AI), the rest start fresh. Saving a match saves a
copy of its state (saving changes a game, 0x4e3410). Stdin commands: status, pause, resume,
kick, ban, unban, rename, end <border> <faction|draw>, save, quit. Pause = command 0x10 from
the host seat in every match (also new ones); the draw timer stops while paused.
Check: `agent/tools/campaign_host.sh` (`end 0 0` captures and restarts B-C as 0 v 2, a joiner
registers, ban kicks it, save, quit, restart resumes the same world and matches).
Measured: a generated 30-area world (14 live matches) keeps real time in one process, ~1 GB
RSS (~70 MB a match: each thread loads its own assets), ~50% of one core.

### 5. Join routing and identity (DONE 2026-10-28)
Done: one endpoint for every match (`transport.rs Hub`): code `addr#hash#game`, the hello
carries the game id (`hello_msg(me, Some(m))`, native and browser), each match a lobby
`Transport` of its own; a game id is new per match so a border's next match never meets old
links. Names (`host.rs Players`): unique (case-insensitive); a new name joining by code is
registered to the side whose faction has fewer players (the web map registers explicitly);
`NetGame.join_filter` gives a joiner only its faction's army or refuses it; kick = leave
(reason 6) + link closed; ban = kick + name refused, saved in `players.txt`; rename (the old
name is kicked from matches). Faction colours forced into the setup (`GameSetup.colors`).
Not done: a refused joiner gets no reason (its link closes; the glue join loop times out).
Check: covered by campaign_host.sh (joiner registered to faction 0, ban) and campaign_web.sh.

### 6. Web map — `src/campaign/web.rs`, `campaign.html` (DONE 2026-10-28)
Done: std `TcpListener` server (default port 8780): `/` the page, `/state?name=` JSON (stats
and join codes only for the viewer's faction's sides, filtered on the host), `/minimap`
(a PNG of a copy of the state seen with that side's vision mask), `/join` (register / re-pick
after elimination when `eliminated pick`), `/chat`, `/matches` (text, for the .exe),
`/admin?token=&cmd=` (the token is printed at start), `/play/...` serves `web/` (or
SCBW_CAMPAIGN_PLAY); Join in browser opens SCBW_CAMPAIGN_WEB (default `/play/index.html`)
with `?join=<code>&name=`. Page: SVG map, squares on live borders (yellow outline from 5
idle minutes), side panel (you / factions / match / faction chat / admin with `?token=`).
Check: `agent/tools/campaign_web.sh` (curl of every endpoint incl. hidden enemy stats,
refused second faction, PNG, 404 without token; SHOT=1 screenshot looked at: map, panel,
hidden enemy side, join buttons, minimap).
Not verified: the browser Join (the wasm build from `/play/`; a non-localhost http page
cannot use WebTransport, so SCBW_CAMPAIGN_WEB must be https for remote players).

### 7. In-game pieces (.exe) (DONE 2026-10-28, chooser in the terminal)
Done: `scbw campaign join <web map url> <name> [faction]` registers, lists the faction's live
matches, plays the one picked (a child process `scbw campaign join <code> <name>`) and lists
again after it ends. Faction chat: web lines go to the faction's humans in each of its
matches (Storm messages from the host seat "Faction"); in-game lines starting with `!` go to
the faction's chat on the map and its other matches.
Not done (decision): the match list is a terminal prompt, not an in-game glue dialog.
Check: campaign_web.sh with CHOOSER=1 (lists 2 matches, plays one headless to frame 700,
"!hi from the game" said at 400 shows in the faction chat, lists again).

### 8. World generator — `scbw campaign gen <areas> <factions> <maps>... [--seed N]` (DONE 2026-10-28)
Done: `World::generate`: areas on a jittered grid, borders = Gabriel graph (planar,
connected), names from a list, factions Z / T / P in turn (3 factions named after races),
colours 0.., owners dealt (`deal`), `eliminated pick`. 30 areas: ~52 borders, 14 live,
owners 11 / 10 / 9. Check: unit test `generated_worlds` (4 sizes: counts, connected, every
faction owns land, live borders exist, write / parse round trip).

### Later (not now)
Area bonuses (start extras per owned areas), spectating with faction vision, several armies
per side, `scbw://` URL handler.

## Open questions

- Memory: ~70 MB per match (assets loaded per thread); share them if worlds get bigger.
- The AI's play after a human leaves or on resume is not checked beyond its script starting.
- Name-only identity lets anyone claim an enemy name and see their faction's stats; OK for
  friends, kick / ban / rename cover abuse.
- Map pool contents: maps must load in 1.16.1 (910map SC:R-era maps may not); a map that
  fails to start goes on the host's bad list (not checked up front).
