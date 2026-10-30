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
| Draw | Draw timer (game time): reset by any unit/building death or any minerals mined (never gas: user, 2026-10-28). Warnings to both sides each minute from 5 to 9 min idle; draw at 10 min (14280 frames at Fastest). Draw -> fresh match on the same border at once. |
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
- Mining: 1.16.1's score counters (minerals gathered per player; not gas) feed the draw timer
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
`State.campaign: bool` from `GameSetup`. Each frame: if any death / minerals-gathered counter
changed, reset `idle_frames`; at 5..9 min push a notice to all players; at 10 min set every player's result to draw (5) and end the game. Checks:
replays and the suite unaffected (flag off); a headless AI-vs-AI run that idles hits it.
Done: `State.campaign / idle_frames / activity`, `State::campaign_tick` after each trigger pass
(view.rs `Scene::step`); activity = men + buildings lost + minerals gathered, every player
(not gas: 2026-10-28 an AI-only run had two matches past frame 3,000,000, minerals mined out
but depleted geysers still giving gas, so the timer never ran; the user's rule is "no minerals
mined"). A minute = 60000 / 42 = 1428 frames (Fastest). Warnings `NOTICE_DRAW` (0x1002, our
text "No kills and no minerals mined: the game is a draw in N minutes.") at 5..9 idle minutes; at 10,
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
Browser join: verified in F4 (https needed for remote players, see there).

### 7. In-game pieces (.exe) (DONE 2026-10-28, chooser in the terminal)
Done: `scbw campaign join <web map url> <name> [faction]` (now the glue's Join Game screen,
F2). Faction chat: web lines go to the faction's humans in each of its
matches (Storm messages from the host seat "Faction"); in-game lines starting with `!` go to
the faction's chat on the map and its other matches.
The terminal prompt was replaced by the Join Game screen (F2).

### 8. World generator — `scbw campaign gen <areas> <factions> <maps>... [--seed N]` (DONE 2026-10-28)
Done: `World::generate`: areas on a jittered grid, borders = Gabriel graph (planar,
connected), names from a list, factions Z / T / P in turn (3 factions named after races),
colours 0.., owners dealt (`deal`), `eliminated pick`. 30 areas: ~52 borders, 14 live,
owners 11 / 10 / 9. Check: unit test `generated_worlds` (4 sizes: counts, connected, every
faction owns land, live borders exist, write / parse round trip).

## Follow-ups (user, 2026-10-28)

### F1. Faction chat must not reach the enemy (DONE 2026-10-28)
Done: the host (`live.rs route_chat`) sends a campaign match's `!` line (`bw::enhanced::
FACTION_CHAT`) only to the links whose human is on the sender's army, and to itself (which
passes it to the web chat and the faction's other matches).
Found: a late joiner never saw any chat line: its `Chat.accept` stayed 0 because 0x4ee180
(`Chat::game_start`) is skipped for a game from a save; `late_game` now calls it (all
enhanced games).
Check: `CHAT=1 agent/tools/campaign_match.sh`: joiners on both sides; the enemy (NETREP_DEBUG
prints received lines) gets "hello all" (1) and not "!secret plan" (0); the host reports the
faction line; all three hashes equal.

### F2. In-game next-match list (DONE 2026-10-28)
Done: `scbw campaign join <web map> <name> [faction] [shot]` registers, then the glue opens
straight on the real Join Game screen (gluJoin.bin, `Glue::campaign_join_screen`) whose list
is the faction's live matches (`snet::set_campaign`: `enum_games` reads the host's
`/matches` once a second and makes melee descriptions of them); the description panel's
subtype line shows "Match: m:ss  supply u/m"; Create Game is disabled; Ok joins (the usual
lobby -> late start); after the match the glue comes back to this screen; Cancel quits. The
terminal prompt is gone.
Check: `CHOOSER=1 agent/tools/campaign_web.sh` (SCBW_GLUE_INPUT: list shot, Ok, the match
headless to frame 500 with a faction line said, the list again, Cancel = exit 0; the line
reached the map's chat). Looked at list.png / after.png: both matches listed, the stat line,
Create Game greyed, the list back after the match with the time moved on.

### F3. A refused joiner gets the reason (DONE 2026-10-28)
Done: before the Start the hub answers a lobby message type 3 + the reason ("the match is
full", "the match ended": no lobby for that game id) and closes; after it the match's host
sends control C_REFUSED (9) + the reason ("<name> is banned from this campaign", "<name>
plays for another faction", "the name <name> is in use in a match", "no army to join") and
closes the link 2 s later (`NetGame::refuse`, `take_refused`); a kick / ban / rename of a
player in the game sends it too and ends that player's game with it. The joiner keeps it
(`snet::refused`): the .exe prints it (`SCBW_JOIN` / `campaign join <code>` exit with it),
the glue's Join Game screen and lobby show it in a popup and go back to the list, the
browser's `net_join` fails with it (the page's status line). Test knob SCBW_CAMPAIGN_SLOTS.
Check: `agent/tools/campaign_refuse.sh`: each of banned / another faction / in use / full /
ended prints its reason; the glue popup (refused.png, back.png looked at).

### F4. Browser join (DONE 2026-10-28)
Done: nothing broke. `agent/tools/campaign_browser.sh`: `web/build.sh`, one campaign match
(`SCBW_NET_PACE_US=70000`: the runner now takes the same pacing knob as headless peers, so a
headless browser at ~18 fps keeps up), headless chromium on the web build with
`?join=<code>&frames=1500&name=web` (game files from web/auto/): it took army 0 from the AI
(the hub routed `#0`, 160 KB start), and its `frame 1500 hash` equals the host's. A second
page with a match id nobody hosts shows "refused: the match ended" in its status line.

**Remote players need https.** A browser only runs WebTransport from a secure context: the
campaign host's own `/play/` works for http://localhost, not for http://<lan or internet
address>. For anyone else, host the browser build (web/ after `web/build.sh`) on an https
origin (e.g. GitHub Pages) and start the host with SCBW_CAMPAIGN_WEB=<that https URL of
index.html>; the map's "Join in browser" then opens it with `?join=<code>&name=`. The
connection itself needs no certificate setup (the code pins the host's self-signed cert,
`serverCertificateHashes`); the host's UDP port must be reachable (port forward / UPnP /
LAN). The players' browsers still load the game files their own way (web/index.html).

### F5. HTTPS web map (DONE 2026-10-28)
Done: the host serves the same handlers over plain HTTP (web port, default 8780) and HTTPS
(`scbw campaign host <world> [dir] [port] [web port] [https port]`, default web port + 1):
rustls through wtransport's re-export (`wtransport::tls::rustls`, ring provider; no new
crate), a self-signed certificate from wtransport's builder (400 days; localhost, 127.0.0.1,
SCBW_NET_BIND, SCBW_CAMPAIGN_NAMES) saved as `dir/https.der` + `https.key` and reused on
resume. Both URLs are printed. The page's Join default stays its own relative
`/play/index.html`, so it is https when the page came over https (SCBW_CAMPAIGN_WEB still
overrides). A hub bound to 0.0.0.0 puts 0.0.0.0 in its codes: the page swaps in the address
it was loaded from, and the .exe's list the web map's address.
WebTransport after an accepted certificate warning: **works** in both browsers tried, the
page is a secure context (`isSecureContext` true) once the warning is passed; the game's
QUIC link does not depend on that certificate (it pins the hub's own by hash).
Check: `agent/tools/campaign_https.sh` (host on 0.0.0.0, page https://192.168.4.20:<port>/
play/index.html?join=..., host paced 70 ms a frame): equal frame 1500 hashes with
- Chromium `--ignore-certificate-errors`;
- Chromium without it, the warning page passed by typing "thisisunsafe" (CLICK=1); without
  that the page stays on the warning (`isSecureContext` false);
- Firefox 154 headless over WebDriver BiDi with `acceptInsecureCerts` (FIREFOX=1;
  agent/tools/bidi_console.mjs).
The restarted host serves the same certificate (SHA-256 fingerprint equal).
Not verified: a person clicking through in a windowed browser; another machine on the LAN
(all runs were on this machine's LAN address); Safari.

### F6. Gas does not count for the draw timer; whole-campaign check (DONE 2026-10-28)
An AI-only run had two matches past frame 3,000,000 (Protoss 77 supply v Zerg 8, minerals
mined out, gas still coming from depleted geysers): `activity_count` is now deaths + minerals
gathered only (user: gas never counts); the draw_timer test feeds gas through the 10 minutes
and still gets the draw; the notice reads "No kills and no minerals mined: ...".
Check: `agent/tools/campaign_e2e.sh` (4-area ring, 4 maps, 3 factions, AI only, unpaced):
PASS in 75 s: captures, Terran then Zerg eliminated, three draws on West-North restarted,
"Protoss wins the campaign".

### F7. Fixes from a live 30-area campaign (DONE 2026-10-28)
User's run (`campaign gen 30 3 <BroodWar maps> --seed 7`, normal pace):
- **Resume spun**: after `save` / `quit` / restart every match stayed at its saved frame,
  ~1160% CPU, joins unanswered. The resumed host's session (`Session::new_at(.., start)`)
  knew only frames below `start`; its own turns for start..start+delay never came, so it
  waited on (and dropped) itself in a loop. The host now joins its own session as a joiner
  does (`add_player(0, 0, host, start + delay)`).
- **Kept across a restart** (`dir/host.txt`): the game port, the admin token, the next match
  id; each match's id is saved with it (`border-N.txt` `game G`), so join links and the admin
  URL survive; the QUIC certificate (`quic.der` / `quic.key`) is reused while younger than 13
  days (browsers pin only certificates valid <= 14 days; after that it is made again and the
  codes change).
- `/play`: the browser build is looked for in SCBW_CAMPAIGN_PLAY, `web` in the working
  directory, beside the executable and in its repo (`<repo>/target/release/scbw`); none:
  an error line at start naming SCBW_CAMPAIGN_PLAY. `/favicon.ico` answers 204.
- Web map: area labels go below, above, right or left, wherever no border square or area
  covers them; the faction list shows the race only when the name is not the race.
- A headless-browser joiner on a normal-pace host is dropped after ~24 s: a headless
  chromium runs ~18 fps and cannot keep up with 24 (expected; the checks pace the host at 70 ms).
  A real windowed browser keeping up is not confirmed yet.
Check: `agent/tools/campaign_host.sh` "resume checks": frames advance after the restart
(650 -> 794 in ~6 s), same join code, same admin token, a joiner with the old code gets in;
host started from /tmp serves /play (200) and /favicon.ico (204); seed-7 map screenshot looked
at (Umoja, Korhal, Braxis clear).

### F8. Headless and silent (DONE 2026-10-29)
User saw scbw windows flash on their desktop: campaign_refuse.sh's joiners ran `scbw campaign
join <code>` (the glue with a window) on the user's display. Now: `view::forbid_windows()`
(set by `campaign host` / `campaign match`) makes every window open fail and Audio::open
return None; SCBW_MUTE=1 opens no sound device anywhere (game unchanged); every
agent/tools script exports SCBW_MUTE=1, the campaign scripts unset DISPLAY / WAYLAND_DISPLAY
and the refused joiners use the glue's screenshot mode.

### F9. Browser campaign play (DONE 2026-10-29)
User reports from a real browser join; one page now:
- **Late joiner's view** (native and browser): the save's local view was the host seat's:
  `local_vision_mask` (0x57f0b0, not in a save) stayed State::new's 1 = player 0's vision
  (the enemy's, for army 1) and the circle rows 0x581d6a made own units red. `late_game`
  sets the mask to 1 << the army as a new game does and runs 0x49b870 (nothing selected yet).
- **Keys as a file name**: in the browser the glue's keydown listener kept filling its buffer
  during the game; after it the score screen's save-replay box got them. The glue clears
  it when a game returns.
- **Leave**: in a campaign match the end-mission dialog's quit button is "Leave" (text
  "LLeave": first character = hotkey) and leaves at once (`menu::Action::Leave`: the loop
  ends, the net driver's drop announces the leave, the army goes to the AI); no surrender
  question, no score screen, no replay. Other games keep 1.16.1's.
- **One page**: the campaign map page loads the player's own files once (install folder, or
  their Google Drive folder with their API key + folder id, or `#drive=key/folder`; kept in
  IndexedDB, web/files.js; key and id never kept or logged), loads the browser build
  (web/stage.js, then pkg/scbw.js: `campaign_load`), then shows the map. Join hides the map
  and plays in the page's stage (`campaign_play`: link, late start, the game; no glue); when
  the game ends (Leave, won, lost, the match over) the stage is emptied and the map is back,
  same name and faction; join again without a reload. index.html shares web/stage.js.
- **No game files from the host**: /play/auto/ answers 404 unless SCBW_CAMPAIGN_AUTO=1
  (tests); players load their own install.
- Repeat joins: a game's parked window, taken again by the next game, took no keys (F10 dead
  in the second game); `campaign_play` now drops it at the end and the page removes its
  canvas, so every game makes a fresh one. The host's "name in use" check read stats up to
  1 s old, refusing a quick rejoin after Leave: the runner now reports at once when a side's
  humans change.
Check: `agent/tools/campaign_page.sh` (headless chromium, CDP steps): files loaded once, join
match 1, box select (own units green), F10 opens the game menu, End Mission shows Leave,
Leave -> the map, join match 2, F10 menu, Leave -> the map; screenshots looked at. Native late joiner on army 1
(scripted box select): own units green-circled, the minimap fogged but for its own base.

### F10. Handoff fixes (DONE 2026-10-29)
User report from a live game: after changing players buildings stopped, drones stopped, workers
"return minerals and just stay there".
- **The AI never took an army back** (campaign matches): a campaign army is a computer from the
  start, so its own slot is human index n (< 8) that never was a human and never leaves. After a
  joiner left, `leave_notice` found that slot as "another human" on the army and skipped
  `ai_take_over`: the army stayed type 2 with no AI (no new buildings, nothing re-tasked).
  `enhanced_join` now marks the slot gone when it takes an AI army (unless the joiner is that slot).
- `ai_take_over` also claims units made while a human had the army (0x463040 `ai_take_unit`,
  those with no AI state).
- Experiment: `scbw campaign handoff <map> [at] [more]` (SEED=<n> picks the races): one match four
  ways (untouched / save+load / +join / join, leave, rejoin); per 120 frames minerals mined, unfinished
  buildings, workers by order. After the fix the AI builds again after a leave (3 unfinished at +840).
- Not reproduced: workers parked at the mineral line after returning cargo (mining carries on through
  every handoff, income within a few percent); one drone (the AI's builder) stops at the join, and is
  not re-tasked by the AI after a take-over (open).
- Campaign page: the game canvas scales (`fitStage` after the stage is shown; Fullscreen button);
  `/drive` serves `SCBW_CAMPAIGN_DRIVE` (`<key>/<folder>`, env only) and the page loads from it.

### F11. AI <-> human switching (DONE 2026-10-29)
`scbw campaign switch <map> [cycles]` (SEED=<n>; SWITCH_AT=<frame> first join, default 3000;
SWITCH_DEBUG=1 prints scripts, towns, buildings, requests): army 0 goes AI -> human (1200 frames)
-> AI (1800) ... through the live commands (join with its save and load, the human's select / stop
/ right-click, leave 0x57); `BUG:` lines per failed check. Fixed in `bw/enhanced.rs`:
- **Retaken AI dead**: `ai_release` set flag 4 on every script-pool entry of the army, free ones
  too; 0x45aef0 keeps a reused entry's `flags & 7`, so the next take-over's melee script was
  killed on its first run. Release now unlinks the army's scripts and clears their flags.
- **AI still ran the human's army**: units kept their AI (towns, worker / guard posts): stopped
  workers went back to mining, units sent away were attack-moved back. Release now takes every
  unit out of the AI (0x4a1e50, as for a change of owner).
- **Retaken AI never trained**: idle units kept the human's idle order, and only 0x9c serves the
  AI's requests. Take-over gives idle units units.dat's computer idle order (as 0x49fa40 does for
  a computer's new unit), and starts the player's AI data fresh (0x447680 / 0x437a70 for it).
- **Workers built for the human**: a worker the AI had sent to place a building (orders 0x19 /
  0x1a / 0x1e / 0x1f, current or queued) is stopped at the release, as the Stop command does.
Sweep (4 maps x 9 seeds x 3 cycles): clean apart from one run (below).
- The save and load at a join changes `state_hash`: native load behaviour, not a loss. 0x4e2e20
  refreshes the overlap status 0x40000 (0x42d9a0) of every unit it puts back in the finder, and
  0x493f70 / 0x475420 give a pylon with AI state order 0x9c. Every peer loads, so no desync.
  SWITCH_DEBUG lists the unit fields that differ other than 0x40000.
- **Retaken AI lost its expansions** (Bottleneck SEED=8: stalled with no powered gateway spot at
  the start town): the release frees the towns and the melee script's start_town takes everything
  into one town at the start location. Take-over now runs the melee script's first step at once,
  starts the expansion script (`State::ai_expand_script`, ours: the last `expand` target, kept in
  `Extras`) over each other town hall's resource area, moves the units within 0x200 of that hall
  into its town, re-picks a town's main building when it moved, and claims unfinished units into
  the nearest town (start_town takes finished ones only; an expansion hall still warping in at a
  take-over never got AI before). Idle orders are fixed for every unit, not only unclaimed ones.
- **Retaken AI never progressed** (long runs: one town, the main mined out, stuck at 32 minerals):
  each take-over restarted the melee script from its start, and its timed waits never let it reach
  `expand` (f13825 untouched) between handoffs. The release now parks the army's script threads
  (`State::ai_parked`, ours, in `Extras`: position, flags, area, and each town's place, resource
  area, worker limit and build list `town_units`); the take-over remakes the towns (the one nearest
  the start first: its start_town takes the finished units; the others take the units within 0x200)
  and resumes the threads. PlayerAi stays (only its requests start over). The start town goes first
  in the town list: equal-priority requests are pushed in town order, and the release had stopped
  the worker placing the Forge, so an expansion's cannon request (needs the Forge) blocked it.
- A dying unit (order 0) is never claimed (its AI cleanup has run; a town kept a dead worker and
  panicked in the merge 0x434330); a unit a building trains joins when it finishes (0x4a2830), but
  a unit still hatching from an egg is claimed. Stopped workers (PlayerGuard 3) get the computer's
  idle order.
- Tests: `agent/tools/campaign_switch.sh` (agent/testing.md 4c); `extras_round_trip` unit test.
  2026-10-29: 478 / 480 runs clean (6 maps x 16 seeds x 5 modes); the two left bank 1500-1900
  minerals in busy mode. `enhanced.sh` with QUIT_AT: host and late joiner hashes equal.

### F12. Web joins and shared armies (2026-10-29)
`agent/tools/web_army.sh` (agent/testing.md 4c): browsers joining a running match, sharing an army
with a native player, leaving, and taking it back from the AI. Fixed:
- An enhanced host dropped a slow but live player: the 3 s drop ran from the start of a stall
  run, so a browser a little behind the host's pace (short stalls back to back) was dropped. Now a
  player is dropped when its own turns made no progress for 3 s (`NetGame::progress`).
- A campaign host printed `frame K hash` for the state it had run to (past K with a slow peer);
  for K a multiple of 8 it now prints confirmed frame K's (`NetGame::confirmed_hash`).
- live.rs logs go through `elog` (the browser console shows them; eprintln is lost in wasm).
- Test hooks: `?quit=` / `?script=` for a running game joined from the page; `selw` selects the
  local army's worker (it used the link id as a human id).
Fixed after (same day): the browser build runs in a Web Worker, so a background tab keeps playing
(see agent/testing.md 4c, `ONE_BROWSER=1 web_army.sh`).

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
