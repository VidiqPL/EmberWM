#!/usr/bin/env python3
"""Scratchpad layout, stacking pixels, focus, fullscreen and lifetime on private Wayland."""
import json, os, re, socket, subprocess, tempfile, time
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ART = ROOT / 'artifacts/scratchpad'
ART.mkdir(parents=True, exist_ok=True)
BIN = ROOT / 'target/release/emberwm'
HELPER = ROOT / 'artifacts/native-vm/wm-client'

def wait(fn, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = fn()
        if result:
            return result
        time.sleep(.04)
    raise AssertionError('scratchpad operation timed out')

with tempfile.TemporaryDirectory(prefix='ember-scratchpad-') as folder:
    tmp = Path(folder)
    cfg = tmp / 'config.toml'
    source = (ROOT / 'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg', str(ROOT / 'assets/abbey-oakwood.jpg'))
    source += '\n[[window_rules]]\napp_id="float-app"\nfloating=true\nx=100\ny=120\nwidth=420\nheight=260\n'
    cfg.write_text(source)
    env = os.environ.copy()
    outer = Path(env['XDG_RUNTIME_DIR'])
    display = env.get('WAYLAND_DISPLAY', 'wayland-0')
    runtime = tmp / 'run'
    runtime.mkdir(mode=0o700)
    env['XDG_RUNTIME_DIR'] = str(runtime)
    env['WAYLAND_DISPLAY'] = display if display.startswith('/') else str(outer / display)
    log = (ART / 'server.log').open('w')
    proc = subprocess.Popen([str(BIN), '--nested', '--config', str(cfg)], env=env, stdout=log, stderr=log)
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    peer = runtime / 'test.sock'
    sock.bind(str(peer))
    sock.settimeout(5)
    clients = []
    passed = []
    try:
        server = wait(lambda: next(runtime.glob('emberwm-wayland-*.sock'), None))
        sock.connect(str(server))
        def ctl(command):
            sock.send(command.encode())
            result = json.loads(sock.recv(262144))
            assert not result.get('error'), (command, result)
            return result
        def state(): return ctl('status')
        def win(app): return next((w for w in state()['windows'] if w['app_id'] == app), None)
        def focus(app):
            for _ in range(len(state()['windows']) + 1):
                if win(app)['focused']: return
                ctl('focus-next')
            raise AssertionError(('could not focus', app, state()))
        def spawn(app):
            fifo = tmp / (app + '.fifo')
            out = (ART / (app + '.log')).open('w')
            client_env = env.copy()
            client_env['WAYLAND_DISPLAY'] = state()['wayland_display']
            child = subprocess.Popen([str(HELPER), '--app-id', app, '--control', str(fifo)], env=client_env, stdout=out, stderr=out)
            clients.append((child, out))
            wait(lambda: (w if w['placement_ready'] else None) if (w := win(app)) else None)
            return fifo
        def command(fifo, text):
            with fifo.open('w') as f: f.write(text + '\n')
        def capture(name):
            path = ART / (name + '.png')
            path.unlink(missing_ok=True)
            ctl('screenshot ' + str(path))
            def ready():
                try:
                    image = Image.open(path).convert('RGB'); image.load(); return image
                except (FileNotFoundError, OSError): return None
            return wait(ready)
        def memory():
            text = Path(f"/proc/{state()['pid']}/smaps_rollup").read_text()
            return {k: int(v) for k, v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)', text, re.M)}

        ctl('scratchpad-toggle'); ctl('scratchpad-mark')
        assert state()['scratchpad'] == {'visible': False, 'windows': 0}
        b = spawn('base')
        a = spawn('overlay')
        original_geometry = win('overlay')['geometry'].copy()
        ctl('scratchpad-mark')
        w = wait(lambda: win('overlay') if win('overlay')['scratchpad'] else None)
        assert w['visible'] and w['focused'] and w['floating']
        assert w['geometry'] == original_geometry, (w['geometry'], original_geometry)
        assert win('base')['geometry']['width'] > w['geometry']['width']
        command(a, 'pattern'); time.sleep(.15)
        rect = w['geometry']; point = (rect['x'] + 50, rect['y'] + 60)
        assert capture('shown').getpixel(point) == (50, 60, 48)
        focus('base')
        assert capture('above-focused-base').getpixel(point) == (50, 60, 48)
        ctl('scratchpad-toggle')
        assert not win('overlay')['visible'] and win('base')['focused']
        assert capture('hidden').getpixel(point) == (39, 24, 36)
        ctl('scratchpad-toggle')
        assert win('overlay')['focused'] and win('overlay')['geometry'] == rect
        passed.append('Marking floats above focused normal windows; hiding removes pixels and focus, showing restores the same window/geometry')

        ctl('scratchpad-toggle'); focus('base'); ctl('send 3'); ctl('workspace 2')
        workspace_labels = [item['text'] for panel in ctl('panels')['items'] for item in panel['widgets'] if item['kind'] == 'workspaces']
        assert '1' not in workspace_labels and not win('overlay')['visible']
        ctl('scratchpad-toggle'); assert win('overlay')['visible'] and win('overlay')['focused']
        c = spawn('fullscreen-base'); command(c, 'fullscreen')
        wait(lambda: win('fullscreen-base')['fullscreen'])
        assert win('overlay')['visible'] and win('fullscreen-base')['visible']
        assert capture('above-fullscreen').getpixel(point) == (50, 60, 48)
        ctl('scratchpad-toggle'); assert win('fullscreen-base')['focused']
        command(a, 'fullscreen'); wait(lambda: win('overlay')['fullscreen'])
        assert not win('overlay')['visible'] and win('fullscreen-base')['fullscreen']
        ctl('workspace 4'); assert ctl('panels')['items']
        ctl('scratchpad-toggle'); wait(lambda: not ctl('panels')['items'])
        assert win('overlay')['focused']
        command(a, 'unfullscreen'); wait(lambda: not win('overlay')['fullscreen'])
        assert win('overlay')['geometry'] == rect
        passed.append('Scratchpad summons across workspaces without phantom occupied workspaces; normal fullscreen stays intact and hidden fullscreen cannot hide panels or steal focus')

        ctl('scratchpad-mark')
        assert not win('overlay')['scratchpad'] and not win('overlay')['floating']
        assert win('overlay')['workspace'] == 4 and win('overlay')['visible']
        assert state()['scratchpad'] == {'visible': False, 'windows': 0}
        d = spawn('float-app'); saved = win('float-app')['geometry']
        ctl('scratchpad-mark'); assert win('float-app')['geometry'] == saved
        ctl('rules-apply'); ctl('scratchpad-mark')
        assert win('float-app')['floating'] and win('float-app')['geometry'] == saved
        ctl('scratchpad-mark'); focus('overlay'); ctl('scratchpad-mark')
        assert state()['scratchpad'] == {'visible': True, 'windows': 2}
        ctl('scratchpad-toggle'); assert not win('overlay')['visible'] and not win('float-app')['visible']
        ctl('scratchpad-toggle'); assert win('overlay')['visible'] and win('float-app')['visible']
        ctl('send 5')
        assert not win('overlay')['scratchpad'] and win('overlay')['workspace'] == 5
        assert win('float-app')['scratchpad'] and win('float-app')['focused']
        passed.append('Repeated mark restores tiling or exact saved floating geometry; multiple members hide/show together; sending a member returns it to a normal workspace')

        ctl('shortcuts-toggle')
        rows = wait(lambda: state()['ui']['rows'])
        assert any(row['keys'] == 'Alt+g' and row['action'] == 'scratchpad-toggle' for row in rows)
        assert any(row['keys'] == 'Alt+Shift+g' and row['action'] == 'scratchpad-mark' for row in rows)
        ctl('shortcuts-toggle')
        cfg.write_text(source); ctl('reload')
        assert win('float-app')['scratchpad'] and win('float-app')['focused']
        ctl('scratchpad-toggle'); time.sleep(2.2); before = memory()
        for _ in range(50): ctl('scratchpad-toggle'); ctl('scratchpad-toggle')
        time.sleep(.3); after = memory()
        assert after['Pss_Anon'] - before['Pss_Anon'] < 4096, (before, after)
        assert not state()['ui']['buffer_bytes'] and not ctl('launcher')['buffer_bytes']
        command(d, 'quit'); wait(lambda: win('float-app') is None)
        assert state()['scratchpad'] == {'visible': False, 'windows': 0}
        spawn('new-base'); e = spawn('closing-overlay'); ctl('scratchpad-mark')
        command(e, 'quit'); wait(lambda: win('closing-overlay') is None)
        assert win('new-base')['focused'] and state()['scratchpad'] == {'visible': False, 'windows': 0}
        f = spawn('unmark-over-fullscreen'); ctl('scratchpad-mark')
        focus('new-base'); ctl('fullscreen'); focus('unmark-over-fullscreen'); ctl('scratchpad-mark')
        assert win('new-base')['fullscreen'] and win('new-base')['focused']
        assert not win('unmark-over-fullscreen')['scratchpad'] and not win('unmark-over-fullscreen')['visible']
        passed.append('Unmarking above a fullscreen app preserves that app and returns keyboard focus to a visible window')
        passed.append('Generated shortcuts and reload retain scratchpad state; 100 toggles stay bounded; closing hidden/visible members clears references and restores visible focus')
        fullscreen_geometry = win('new-base')['geometry'].copy()
        ctl('scratchpad-mark')
        assert win('new-base')['geometry'] == fullscreen_geometry and not win('new-base')['fullscreen']
        ctl('scratchpad-mark')
        assert not win('new-base')['scratchpad'] and not win('new-base')['floating']
        passed.append('Marking preserves exact tiled, floating and fullscreen display geometry; unmarking still restores the prior layout mode')
        report = {'passed': passed, 'memory_before_kib': before, 'memory_after_kib': after,
                  'scope': 'Isolated nested Wayland session, private runtime and synthetic clients. No host input, apps, clipboard or workspace modified.'}
        (ROOT / 'docs/scratchpad-results.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    finally:
        for child, out in clients:
            if child.poll() is None: child.terminate(); child.wait(timeout=5)
            out.close()
        if proc.poll() is None:
            try: ctl('quit'); proc.wait(timeout=5)
            except (OSError, AssertionError, subprocess.TimeoutExpired): proc.terminate(); proc.wait(timeout=5)
        sock.close(); log.close()
