#!/usr/bin/env python3
"""Test native panels in a real nested Wayland session, without host input injection."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
ART = ROOT / 'artifacts'
BIN = ROOT / 'target/release/emberwm'

def wait(fn, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError('timed out waiting for panels')

def memory(pid):
    return {k: int(v.split()[0]) for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
            if ':' in line for k, v in [line.split(':', 1)] if k in ['Rss', 'Pss', 'Pss_Anon', 'Pss_File', 'Private_Dirty']}

ART.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='ember-panels-') as tmp:
    config = Path(tmp) / 'config.toml'
    original = (ROOT / 'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png', str(ROOT / 'assets/red-dusk.png'))
    config.write_text(original)
    test_env=os.environ.copy()
    host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0')
    runtime=Path(tmp)/'run';runtime.mkdir(mode=0o700)
    test_env['XDG_RUNTIME_DIR']=str(runtime)
    test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display)
    before = set(runtime.glob('emberwm-wayland-*.sock'))
    log = (ART / 'panels.log').open('w')
    proc = subprocess.Popen([str(BIN), '--nested', '--config', str(config)], env=test_env, stdout=log, stderr=log)
    client_path = runtime / f'ember-panels-test-{os.getpid()}.sock'
    client = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    client.bind(str(client_path)); client.settimeout(4)
    passed = []
    try:
        server = wait(lambda: next(iter(set(runtime.glob('emberwm-wayland-*.sock')) - before), None))
        client.connect(str(server))
        def ctl(cmd, error=False):
            client.send(cmd.encode()); value = json.loads(client.recv(262144))
            assert bool(value.get('error')) == error, (cmd, value)
            return value
        def state(): return ctl('status')
        def panels(): return ctl('panels')
        def reload(text):
            config.write_text(text); ctl('reload')
            time.sleep(.12)
        p = wait(lambda: (p if len(p['items']) == 1 else None) if (p := panels()) else None)
        assert p['font_loaded'] and not p['polling'] and not p['volume_query_running'], p
        assert [w['kind'] for w in p['items'][0]['widgets']] == ['workspaces', 'clock'], p
        passed.append('default workspace/clock panel uses no polling or helper process')
        ctl('terminal')
        s = wait(lambda: (s if len(s['windows']) == 1 else None) if (s := state()) else None)
        top = s['windows'][0]['geometry']['y']
        expected_top = ctl('config')['panels']['height'] + ctl('config')['panels']['margin'] + ctl('config')['layout']['gap'] + ctl('config')['theme']['border_width']
        assert top == expected_top, s
        ctl('fullscreen'); wait(lambda: not panels()['items'])
        assert next(w for w in state()['windows'] if w['focused'])['geometry']['y'] == 0
        ctl('fullscreen'); wait(lambda: panels()['items'])
        passed.append('tiling reserves panel space; fullscreen hides panels and uses the full output')
        for text in [original.replace('enabled = true\nfont = "builtin"', 'enabled = false\nfont = "builtin"'),
                     original[:original.index('[[panels.items]]')] + 'items = []\n']:
            reload(text)
            p = panels()
            assert not p['items'] and not p['font_loaded'] and p['buffer_bytes'] == 0 and not p['polling'] and not p['volume_query_running'], p
            assert state()['windows'][0]['geometry']['y'] == ctl('config')['layout']['gap'] + ctl('config')['theme']['border_width']
        passed.append('global disable and an empty panel list release fonts, buffers, timers and reserved space')
        reload(original)
        ctl('panels-toggle'); wait(lambda: not panels()['font_loaded'])
        ctl('panels-toggle'); wait(lambda: panels()['font_loaded'])
        passed.append('panel toggle action works without editing configuration')
        generation = state()['theme_generation']
        for invalid in [original.replace('font = "builtin"', 'font = "/missing/font.ttf"'),
                        original.replace('"workspaces", "clock"', '"not-a-module"')]:
            config.write_text(invalid); ctl('reload', error=True)
            assert state()['theme_generation'] == generation and panels()['items']
        passed.append('invalid fonts and unknown modules reject reload without losing the working UI')
        font = Path(tmp) / 'custom.ttf'
        font.write_bytes((ROOT / 'assets/fonts/EmberPanelMono.ttf').read_bytes())
        reload(original.replace('font = "builtin"', 'font = "custom.ttf"'))
        assert panels()['font_loaded'] and not panels()['error']
        passed.append('custom font paths resolve relative to the config and render successfully')
        multiple = original.replace('name = "system"\nenabled = false', 'name = "system"\nenabled = true') + '''
[[panels.items]]
name = "bottom"
anchor = "bottom-center"
modules = ["text:EMBER", "title", "layout", "date"]
font_size = 13.0
height = 28
background = "#301a23"
on_click = "terminal"
'''
        reload(multiple)
        p = wait(lambda: (p if len(p['items']) == 3 else None) if (p := panels()) else None)
        assert p['polling'] and all(i['widgets'] for i in p['items']) and not p['error'], p
        bottom = p['items'][2]
        assert bottom['y'] > 600 and bottom['on_click'] == 'terminal'
        assert [w['kind'] for w in bottom['widgets']] == ['text:EMBER', 'title', 'layout', 'date']
        shot = ROOT / 'docs/panels-preview.png'; shot.unlink(missing_ok=True)
        ctl(f'screenshot {shot}'); wait(lambda: shot.exists() and shot.stat().st_size > 100)
        passed.append('three independently styled panels render custom text, title, date and system modules')
        static = original.replace('"workspaces", "clock"', '"workspaces", "text:EMBER"')
        reload(static)
        ctl('workspace 9'); time.sleep(.2)
        first = panels()['rasterizations']; frames = state()['frames_submitted']
        time.sleep(2.2)
        assert panels()['rasterizations'] == first
        assert state()['frames_submitted'] - frames <= 2  # terminal cursor / host exposure
        assert not panels()['font_loaded'] and panels()['items']
        passed.append('unchanged panel content reuses buffers and releases its font after idle')
        seconds = original.replace('clock_format = "%H:%M"', 'clock_format = "%H:%M:%S"')
        reload(seconds)
        def clock(): return panels()['items'][0]['widgets'][-1]['text']
        a = clock(); time.sleep(1.5); b = clock()
        assert a != b, (a, b)
        passed.append('optional seconds clock updates live')
        reload(original); baseline = memory(proc.pid)
        for _ in range(30): ctl('reload')
        time.sleep(.2); after = memory(proc.pid)
        assert after['Pss_Anon'] - baseline['Pss_Anon'] < 10 * 1024, (baseline, after)
        passed.append('30 panel reloads stay within the 10 MiB anonymous-memory growth limit')
        report = {'passed': passed, 'memory_before_reloads_kib': baseline, 'memory_after_reloads_kib': after,
                  'scope': 'Real nested Wayland compositor with real terminal; host Plasma remains running.'}
        (ROOT / 'docs/panels-results.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    finally:
        if proc.poll() is None:
            try: ctl('quit'); proc.wait(timeout=5)
            except Exception: proc.terminate(); proc.wait(timeout=5)
        client.close(); client_path.unlink(missing_ok=True); log.close()
