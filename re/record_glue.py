#!/usr/bin/env python3
"""record_glue.py record <dir> <game dir> | replay <dir>

record (run by re/native_record.sh, DISPLAY = the game's): starts 1.16.1 through
re/bwmem (which reads its in-game flag 0x6d11ec, frame 0x57f23c and seed 0x51ca10) and,
from the main menu on, polls the pointer, buttons and keys every few ms. Writes
<dir>/events.txt ("ms ingame frame dt kind args": mv x y, md/mu/rd/ru x y, kd/ku keysym,
game seed / end) and grabs the screen every ~250 ms to <dir>/frames/NNNNN.png
(frames.txt: "n ms ingame frame dt x y"). dt = ms since the frame began.

replay: turns the recording into SCBW_GLUE_INPUT steps (glue steps in ms, game steps at
frames: src/script.rs), runs `scbw glue` with them and diffs each native grab used as a
shot against ours: <dir>/ours/, <dir>/diff/NNNNN.png (native | ours | red where
different), <dir>/report.txt ("n where differing-pixels% what").
"""
import bisect, ctypes, os, queue, re, shutil, subprocess, sys, threading, time

EVERY = 0.25  # s between screen grabs
POLL = 0.004  # s between input polls
LEAD = 60     # ms: a glue shot is the last grab at least this long before the click / key

RE = os.path.dirname(os.path.abspath(__file__))

# X keysym -> Windows virtual key (glue steps).
VK = {'Return': 0x0d, 'KP_Enter': 0x0d, 'Escape': 0x1b, 'BackSpace': 8, 'Tab': 9, 'space': 0x20,
      'Prior': 0x21, 'Next': 0x22, 'End': 0x23, 'Home': 0x24, 'Left': 0x25, 'Up': 0x26,
      'Right': 0x27, 'Down': 0x28, 'Delete': 0x2e, 'Insert': 0x2d}
VK.update({f'F{i}': 0x6f + i for i in range(1, 13)})
# X keysym -> minifb key name (game steps), besides letters, digits and F keys.
MINIFB = {'Return': 'Enter', 'KP_Enter': 'NumPadEnter', 'Escape': 'Escape', 'BackSpace': 'Backspace',
          'Tab': 'Tab', 'space': 'Space', 'Prior': 'PageUp', 'Next': 'PageDown', 'End': 'End',
          'Home': 'Home', 'Left': 'Left', 'Up': 'Up', 'Right': 'Right', 'Down': 'Down',
          'Delete': 'Delete', 'Insert': 'Insert', 'Shift_L': 'LeftShift', 'Shift_R': 'RightShift',
          'Control_L': 'LeftCtrl', 'Control_R': 'RightCtrl', 'Alt_L': 'LeftAlt', 'Alt_R': 'RightAlt',
          'minus': 'Minus', 'equal': 'Equal', 'comma': 'Comma', 'period': 'Period', 'slash': 'Slash',
          'semicolon': 'Semicolon', 'apostrophe': 'Apostrophe', 'bracketleft': 'LeftBracket',
          'bracketright': 'RightBracket', 'backslash': 'Backslash', 'grave': 'Backquote', 'Pause': 'Pause'}
NAMED = {'space': ' ', 'minus': '-', 'equal': '=', 'period': '.', 'comma': ',', 'slash': '/',
         'apostrophe': "'", 'semicolon': ';', 'bracketleft': '[', 'bracketright': ']',
         'backslash': '\\', 'grave': '`'}
SHIFTED = dict(zip('`1234567890-=[]\\;\',./', '~!@#$%^&*()_+{}|:"<>?'))
# WM_CHAR of the control keys.
CTRL_CHARS = {'Return': 13, 'KP_Enter': 13, 'Escape': 27, 'BackSpace': 8, 'Tab': 9}


def char_of(sym, shift):
    ch = NAMED.get(sym, sym if len(sym) == 1 else None)
    if ch is None:
        return None
    return SHIFTED.get(ch, ch.upper()) if shift else ch


def minifb_name(sym):
    if len(sym) == 1 and sym.isalpha():
        return sym.upper()
    if len(sym) == 1 and sym.isdigit():
        return f'Key{sym}'
    if re.fullmatch(r'F\d+', sym):
        return sym
    return MINIFB.get(sym)


class XImage(ctypes.Structure):
    _fields_ = [('width', ctypes.c_int), ('height', ctypes.c_int), ('xoffset', ctypes.c_int),
                ('format', ctypes.c_int), ('data', ctypes.c_void_p), ('byte_order', ctypes.c_int),
                ('bitmap_unit', ctypes.c_int), ('bitmap_bit_order', ctypes.c_int),
                ('bitmap_pad', ctypes.c_int), ('depth', ctypes.c_int),
                ('bytes_per_line', ctypes.c_int), ('bits_per_pixel', ctypes.c_int)]


class X:
    def __init__(self):
        self.x = x = ctypes.cdll.LoadLibrary('libX11.so.6')
        x.XOpenDisplay.restype = ctypes.c_void_p
        x.XDefaultRootWindow.restype = ctypes.c_ulong
        x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        x.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong] + [ctypes.c_void_p] * 7
        x.XQueryKeymap.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        x.XkbKeycodeToKeysym.restype = ctypes.c_ulong
        x.XkbKeycodeToKeysym.argtypes = [ctypes.c_void_p, ctypes.c_ubyte, ctypes.c_int, ctypes.c_int]
        x.XKeysymToString.restype = ctypes.c_char_p
        x.XKeysymToString.argtypes = [ctypes.c_ulong]
        x.XGetImage.restype = ctypes.POINTER(XImage)
        x.XGetImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int,
                                ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int]
        x.XDestroyImage.argtypes = [ctypes.POINTER(XImage)]
        self.d = x.XOpenDisplay(None)
        if not self.d:
            sys.exit('record_glue: no X display')
        self.root = x.XDefaultRootWindow(self.d)

    def pointer(self):
        r, c = ctypes.c_ulong(), ctypes.c_ulong()
        rx, ry, wx, wy, mask = (ctypes.c_int() for _ in range(5))
        self.x.XQueryPointer(self.d, self.root, *map(ctypes.byref, (r, c, rx, ry, wx, wy, mask)))
        return rx.value, ry.value, mask.value

    def screen(self):
        """The 640x480 screen as BGRX bytes (XGetImage, ZPixmap: under 1 ms, so a grab fits
        in one game frame, unlike `import`)."""
        p = self.x.XGetImage(self.d, self.root, 0, 0, 640, 480, 0xffffffff, 2)
        im = p.contents
        raw = ctypes.string_at(im.data, im.bytes_per_line * im.height), im.bytes_per_line
        self.x.XDestroyImage(p)
        return raw

    def keys(self):
        b = ctypes.create_string_buffer(32)
        self.x.XQueryKeymap(self.d, b)
        return {i * 8 + j for i, v in enumerate(b.raw) for j in range(8) if v >> j & 1}

    def keysym(self, kc):
        s = self.x.XKeysymToString(self.x.XkbKeycodeToKeysym(self.d, kc, 0, 0))
        return s.decode() if s else '?'


class Mem:
    """re/bwmem: starts the game and answers "<in game> <frame> <seed>" per request."""

    def __init__(self, game_dir):
        exe = os.path.join(RE, 'bwmem', 'bwmem.exe.so')
        self.p = subprocess.Popen(['wine', exe, 'C:\\StarCraft\\StarCraft.exe'], cwd=game_dir,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.lock = threading.Lock()

    def peek(self, addr):
        with self.lock:
            self.p.stdin.write(f'r {addr}\n')
            self.p.stdin.flush()
            return self.p.stdout.readline().strip()

    def read(self):
        with self.lock:
            self.p.stdin.write('\n')
            self.p.stdin.flush()
            return tuple(int(v) for v in self.p.stdout.readline().split()) or (-1, 0, 0, 0, 0)


def main_menu_shown(png):
    # The main menu's "Single Player" (green text on dark: wine's plain blue-green desktop
    # passes the colour test alone).
    r = subprocess.run(['magick', png, '-crop', '140x16+148+112', '-format', '%[fx:mean.g-mean.r] %[fx:standard_deviation]', 'info:'],
                       capture_output=True, text=True)
    try:
        green, contrast = map(float, r.stdout.split())
        return green > 0.06 and contrast > 0.08
    except ValueError:
        return False


def record(d, game_dir):
    os.makedirs(f'{d}/frames', exist_ok=True)
    x, mem = X(), Mem(game_dir)
    print('Skip the intro (Esc); recording starts at the main menu.', flush=True)
    while not (subprocess.run(['import', '-window', 'root', f'{d}/menu.png']).returncode == 0
               and main_menu_shown(f'{d}/menu.png')):
        if mem.read()[0] < 0:
            return
        time.sleep(1)
    os.remove(f'{d}/menu.png')
    print(f'Recording to {d}. Close the game to stop.', flush=True)
    t0 = time.monotonic()
    ms = lambda: int((time.monotonic() - t0) * 1000)
    state = {'frame': 0, 'frame_ms': 0, 'ingame': 0}
    stop = threading.Event()

    def sample():
        ingame, frame, seed, sx, sy = (mem.read() + (0, 0, 0, 0))[:5]
        t = ms()
        if frame != state['frame']:
            state['frame'], state['frame_ms'] = frame, t
        return t, ingame, frame, t - state['frame_ms'], seed, sx, sy

    pngs = queue.Queue()

    def save():
        from PIL import Image
        while (item := pngs.get()) is not None:
            path, (raw, stride) = item
            Image.frombuffer('RGB', (640, 480), raw, 'raw', 'BGRX', stride, 1).save(path, compress_level=1)

    saver = threading.Thread(target=save)
    saver.start()

    def grab():
        n = 0
        with open(f'{d}/frames.txt', 'w') as f:
            while not stop.is_set():
                t, ingame, frame, dt, _, sx, sy = sample()
                px, py, _ = x2.pointer()
                img = x2.screen()
                # In a game, a grab is kept only if no frame passed while it was taken.
                if not ingame or mem.read()[1] == frame:
                    pngs.put((f'{d}/frames/{n:05d}.png', img))
                    f.write(f'{n} {t} {ingame} {frame} {dt} {px} {py} {sx} {sy}\n')
                    f.flush()
                    n += 1
                time.sleep(max(0, EVERY - (ms() - t) / 1000))

    # The mouse wheel: a press and release at once, which polling misses, so from xinput's
    # raw events (buttons 4 up, 5 down).
    wheel = queue.Queue()

    def wheel_events():
        p = subprocess.Popen(['stdbuf', '-oL', 'xinput', 'test-xi2', '--root'], stdout=subprocess.PIPE, text=True)
        press = False
        for line in p.stdout:
            if line.startswith('EVENT'):
                press = '(RawButtonPress)' in line
            elif press and (m := re.match(r'\s+detail: ([45])$', line)):
                wheel.put('down' if m[1] == '5' else 'up')
                press = False
            if stop.is_set():
                p.kill()
                break

    threading.Thread(target=wheel_events, daemon=True).start()
    x2 = X()  # Xlib displays are not shared across threads
    threading.Thread(target=grab, daemon=True).start()
    with open(f'{d}/events.txt', 'w') as f:
        last_ptr, last_keys, ingame = None, set(), 0
        try:
            while True:
                t, now_in, frame, dt, seed, _, _ = sample()
                if now_in < 0:
                    break
                stamp = f'{t} {now_in} {frame} {dt}'
                if now_in != ingame:
                    # The game's speed and scroll speeds (0x6cdfd4 / d8 / dc) as it starts.
                    opts = ' '.join(mem.peek(a) for a in ('6cdfd4', '6cdfd8', '6cdfdc'))
                    f.write(f'{stamp} game {seed} {opts}\n' if now_in else f'{stamp} end\n')
                    ingame = now_in
                px, py, mask = x.pointer()
                while not wheel.empty():
                    f.write(f'{stamp} wh {wheel.get()} {px} {py}\n')
                if last_ptr is None or (px, py) != last_ptr[:2]:
                    f.write(f'{stamp} mv {px} {py}\n')
                for bit, (dn, up) in ((1 << 8, ('md', 'mu')), (1 << 10, ('rd', 'ru'))):
                    was = last_ptr is not None and last_ptr[2] & bit
                    if bool(mask & bit) != bool(was):
                        f.write(f'{stamp} {dn if mask & bit else up} {px} {py}\n')
                last_ptr = (px, py, mask)
                keys = x.keys()
                # Within one poll: modifiers go down first and up last (Shift+a typed fast).
                mod = lambda kc: x.keysym(kc).split('_')[0] in ('Shift', 'Control', 'Alt')
                for kc in sorted(keys - last_keys, key=lambda kc: not mod(kc)):
                    f.write(f'{stamp} kd {x.keysym(kc)}\n')
                for kc in sorted(last_keys - keys, key=mod):
                    f.write(f'{stamp} ku {x.keysym(kc)}\n')
                last_keys = keys
                f.flush()
                time.sleep(POLL)
        except KeyboardInterrupt:
            pass
    stop.set()
    time.sleep(EVERY)
    pngs.put(None)
    saver.join()


def replay(d):
    events = [l.split() for l in open(f'{d}/events.txt')]
    frames = [tuple(map(int, l.split())) for l in open(f'{d}/frames.txt')]
    # The game's screen position per grab (recordings since bwmem reports it): ours is
    # moved there before each game shot, and a difference is reported.
    cam = {fr[0]: fr[7:9] for fr in frames if len(fr) >= 9}
    frames = [fr[:7] for fr in frames]
    # Ours starts at the main menu: the recording from its first grab showing it (older
    # recordings started early, at wine's desktop).
    first = next((fr for fr in frames if not fr[2] and main_menu_shown(f'{d}/frames/{fr[0]:05d}.png')), None)
    if first and first[1] > 0:
        t0 = first[1]
        events = [[str(int(e[0]) - t0)] + e[1:] for e in events if int(e[0]) >= t0]
        frames = [(n, ms - t0, *fr) for n, ms, *fr in frames if ms >= t0]
    for sub in ('ours', 'diff'):
        shutil.rmtree(f'{d}/{sub}', ignore_errors=True)
        os.makedirs(f'{d}/{sub}')
    steps, shots = [], []
    now = 0          # ms of the glue time the steps reached
    used = -1        # the last grab used
    shift = False
    pending_down = None  # a left press in the glue: its step waits for the release
    seeds = [int(e[5]) for e in events if e[4] == 'game']
    # Frame 0 lasts from the game's start (the recorder's dt counts from the last frame
    # change, which for frame 0 is before the game).
    start = 0
    for e in events:
        if e[4] == 'game':
            start = int(e[0])
        elif e[2] == '0' and e[1] == '1':
            e[3] = str(int(e[0]) - start)
    starts = [int(e[0]) for e in events if e[4] == 'game']
    frames = [(n, ms, ing, fr, ms - max([s for s in starts if s <= ms], default=0) if ing and fr == 0 else dt, x, y)
              for n, ms, ing, fr, dt, x, y in frames]

    def wait(t):
        nonlocal now
        if t > now:
            steps.append(f'w {t - now}')
        now = max(now, t)

    def glue_shot(t, what):
        nonlocal used
        f = [fr for fr in frames if not fr[2] and used < fr[0] and fr[1] <= t - LEAD]
        if not f:
            return
        n, ms, _, _, _, x, y = f[-1]
        used = n
        wait(ms)
        steps.extend([f'p {x} {y}', f's {d}/ours/{n:05d}.png'])
        shots.append((n, 'glue', what))

    # Game steps: (frame, dt, order, lines), sorted when the game ends. 1.16.1 takes input
    # that came during frame N with the step to N + 1, unless the game is paused (the
    # frame lasts longer than PAUSED ms): then at N.
    PAUSED = 200
    game = []
    # (when the frame began, frame) for every in-game stamp.
    stamps = sorted([(int(e[0]) - int(e[3]), int(e[2])) for e in events if e[1] == '1'] + [(fr[1] - fr[4], fr[3]) for fr in frames if fr[2]])
    stamp_ms = [m for m, _ in stamps]

    def input_gate(t, frame, dt):
        i = bisect.bisect_right(stamp_ms, t - dt)
        end = next((m for m, f in stamps[i:] if f != frame), None)
        if end is not None and end - (t - dt) <= PAUSED:
            return frame + 1, 0
        return frame, dt

    def game_add(frame, dt, *lines):
        game.append((frame, dt, len(game), lines))

    def game_flush():
        for frame, dt, _, lines in sorted(game):
            for l in lines:
                steps.extend([f'@ {frame} {dt}', l])
        game.clear()

    def game_shots(upto_ms):
        nonlocal used
        for n, ms, ingame, frame, dt, x, y in frames:
            if used < n and ingame and ms <= upto_ms:
                used = n
                lines = [f'cam {cam[n][0]} {cam[n][1]} {n}'] if n in cam else []
                game_add(frame, dt, *lines, f's {d}/ours/{n:05d}.png')
                shots.append((n, f'game {frame}', ''))

    if seeds:
        steps.append(f'seed {seeds.pop(0)}')
    for e in events:
        t, ingame, frame, dt, kind, args = int(e[0]), int(e[1]), e[2], e[3], e[4], e[5:]
        if kind == 'game':
            # Ours starts with the recorded game's speed and scroll speeds.
            if len(args) >= 4:
                game_add(0, -1, f'set {args[1]} {args[2]} {args[3]}')
            continue
        if kind == 'end':
            game_shots(t)
            game_flush()
            now = t
            if seeds:
                steps.append(f'seed {seeds.pop(0)}')
            continue
        if ingame:
            game_shots(t)
            gf, gdt = input_gate(t, int(frame), int(dt))
            if kind in ('kd', 'ku'):
                sym = args[0]
                if sym.startswith('Shift'):
                    shift = kind == 'kd'
                name = minifb_name(sym)
                if name:
                    game_add(gf, gdt, f'{kind} {name}')
                ch = CTRL_CHARS.get(sym) or (ord(c) if (c := char_of(sym, shift)) else None)
                if kind == 'kd' and ch:
                    game_add(gf, gdt, f'ch {ch}')
            elif kind == 'mv':
                game_add(gf, gdt, f'mv {args[0]} {args[1]}')
            elif kind == 'wh':
                pass  # the game has no wheel input
            else:
                game_add(gf, gdt, f'mv {args[0]} {args[1]}', kind)
            continue
        # The glue: clicks (press and release at one point) or drags, keys and text.
        if kind == 'md':
            glue_shot(t, f'click {args[0]} {args[1]}')
            wait(t)
            pending_down = (int(args[0]), int(args[1]))
        elif kind == 'mu' and pending_down:
            (x0, y0), (x1, y1) = pending_down, (int(args[0]), int(args[1]))
            pending_down = None
            if abs(x1 - x0) <= 2 and abs(y1 - y0) <= 2:
                steps.append(f'c {x0} {y0}')
            else:
                steps.extend([f'd {x0} {y0}', f'w {t - now}', f'u {x1} {y1}'])
                now = t
        elif kind == 'wh':
            wait(t)
            steps.extend([f'p {args[1]} {args[2]}', f'wh {int(args[0] == "down")}'])
        elif kind in ('kd', 'ku'):
            sym = args[0]
            if sym.startswith('Shift'):
                shift = kind == 'kd'
                continue
            if kind == 'ku':
                continue
            ch = char_of(sym, shift)
            vk = VK.get(sym) or (ord(ch.upper()) if ch and ch.isalnum() else None)
            if vk is None and ch is None:
                continue
            glue_shot(t, f'key {sym}')
            wait(t)
            if vk:
                steps.append(f'k {vk:x}')
            if ch and vk not in (8, 9, 0x0d, 0x1b):
                steps.append(f't {ch}')
    if frames:
        game_shots(frames[-1][1])
        game_flush()
        glue_shot(frames[-1][1] + LEAD, 'end')
    open(f'{d}/script.txt', 'w').write('\n'.join(steps) + '\n')

    # Ours starts from the install's characters (the recording's game dir was a fresh copy).
    install = os.environ.get('SCBW_INSTALL', os.path.expanduser('~/Downloads/StarCraft-BroodWar_1.16.1'))
    chars = f'{d}/scbw/characters'
    shutil.rmtree(f'{d}/scbw', ignore_errors=True)
    if os.path.isdir(f'{install}/characters'):
        shutil.copytree(f'{install}/characters', chars)
    else:
        os.makedirs(chars)
    exe = os.environ.get('SCBW', os.path.join(RE, '../target/release/scbw'))
    env = {**os.environ, 'SCBW_GLUE_SCRIPT': f'{d}/script.txt', 'SCBW_CHARACTERS': chars}
    run = subprocess.run([exe, 'glue', f'{d}/ours/last.png'], env=env, stderr=subprocess.PIPE, text=True)
    sys.stderr.write(''.join(l + '\n' for l in run.stderr.splitlines() if not l.startswith('script: cam ')))
    run.check_returncode()
    # "script: cam <shot> <ours x y> <native x y>": ours was elsewhere before the shot.
    cam_off = {int(v[2]): f' cam ({v[3]},{v[4]}) native ({v[5]},{v[6]})' for v in (l.split() for l in run.stderr.splitlines() if l.startswith('script: cam '))}
    red = f'{d}/diff/red.png'
    with open(f'{d}/report.txt', 'w') as rep:
        for n, where, what in shots:
            nat, ours = f'{d}/frames/{n:05d}.png', f'{d}/ours/{n:05d}.png'
            if not os.path.exists(ours):
                rep.write(f'{n:05d} {where} (not reached) {what}\n')
                continue
            r = subprocess.run(['magick', 'compare', '-metric', 'AE', nat, ours, red], capture_output=True, text=True)
            ae = float(r.stderr.split()[0]) if r.stderr else 0
            subprocess.run(['magick', nat, ours, red, '+append', f'{d}/diff/{n:05d}.png'])
            rep.write(f'{n:05d} {where} {100 * ae / (640 * 480):.2f}% {what}{cam_off.get(n, "")}\n')
    if os.path.exists(red):
        os.remove(red)
    commands(d, exe)
    print(open(f'{d}/report.txt').read(), end='')


def commands(d, exe):
    """The game's commands (its LastReplay.rep, kept by native_record.sh) against ours
    (`scbw glue` writes <dir>/scbw/maps/replays/LastReplay.rep): the first that differs is
    where our interface did something else with the same input. agent/tools/repcmp.sh then
    checks the simulation on the game's own replay."""
    native, ours = f'{d}/replays/LastReplay.rep', f'{d}/scbw/maps/replays/LastReplay.rep'
    if not (os.path.exists(native) and os.path.exists(ours)):
        return
    def cmds(rep):
        out = f'{rep}.txt'
        subprocess.run([exe, 'repdump', rep, out], check=True, capture_output=True)
        return [l.split()[1:] for l in open(out) if l.startswith('cmd ')]
    a, b = cmds(native), cmds(ours)
    i = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
    with open(f'{d}/report.txt', 'a') as rep:
        if i == len(a) == len(b):
            rep.write(f'commands: all {i} match\n')
        else:
            rep.write(f'commands: {i} of {len(a)} match; then native {a[i] if i < len(a) else "-"} ours {b[i] if i < len(b) else "-"}\n')


if __name__ == '__main__':
    if sys.argv[1] == 'record':
        record(os.path.abspath(sys.argv[2]), sys.argv[3])
    else:
        replay(os.path.abspath(sys.argv[2]))
