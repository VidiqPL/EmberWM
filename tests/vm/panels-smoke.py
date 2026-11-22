#!/usr/bin/env python3
"""Native panel behavior and matched cold-process RAM comparison in the private VM."""
import json
import re
import time
from pathlib import Path
from control import shell, qmp, WORK

ROOT = Path(__file__).resolve().parents[2]
passed = []
def guest(command): return shell(command).replace('\r', '')
def ctl(command):
    result = guest('emberwm ctl ' + command)
    start = result.find('{\n')
    assert start >= 0, result
    value = json.JSONDecoder().raw_decode(result[start:])[0]
    assert not value.get('error'), value
    return value
def status(): return ctl('status')
def memory():
    text = guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup; cat /proc/meminfo')
    values = {k: int(v) for k, v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|MemTotal|MemAvailable):\s+(\d+)', text, re.M)}
    values['guest_used_kib'] = values['MemTotal'] - values['MemAvailable']
    return values
def wait(fn, timeout=8):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            value = fn()
            if value: return value
        except AssertionError:
            pass
        time.sleep(.1)
    raise AssertionError('native panel operation timed out')
def start(enabled):
    guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/config.toml')
    if not enabled:
        guest(r"sed -i '/^\[panels\]/,/^\[\[panels.items\]\]/s/^enabled = true/enabled = false/' /tmp/config.toml")
    guest('emberwm --native --config /tmp/config.toml >/tmp/ember-panel.log 2>&1 &')
    wait(lambda: status()['session_active'])
    time.sleep(1)
def key(*keys):
    result = qmp('send-key', {'keys': [{'type': 'qcode', 'data': key} for key in keys], 'hold-time': 100})
    assert 'error' not in result, result
    time.sleep(.2)

try:
    start(False); off = memory(); p = ctl('panels')
    assert not p['font_loaded'] and p['buffer_bytes'] == 0 and not p['polling'], p
    start(True); on = memory(); p = ctl('panels')
    assert p['font_loaded'] and len(p['items']) == 1 and not p['polling'] and not p['volume_query_running'], p
    assert abs(status()['output']['refresh_hz'] - 144) < .1
    passed.append('matched cold native sessions compare panel-disabled and workspace/clock panel memory')
    before = status()['frames_submitted']; rasters = p['rasterizations']; time.sleep(3)
    frames = status()['frames_submitted'] - before
    assert frames <= 1 and ctl('panels')['rasterizations'] - rasters <= 1
    passed.append('minute clock causes at most one native redraw in three seconds; no polling/helper')
    key('meta_l', 'b')
    wait(lambda: not ctl('panels')['font_loaded'])
    assert ctl('panels')['buffer_bytes'] == 0
    key('meta_l', 'b'); wait(lambda: ctl('panels')['font_loaded'])
    passed.append('Super+B through native libinput toggles and releases panel resources')
    ctl('terminal'); wait(lambda: len(status()['windows']) == 1)
    assert status()['windows'][0]['geometry']['y'] == 44
    ctl('fullscreen'); wait(lambda: not ctl('panels')['items'])
    assert status()['windows'][0]['geometry']['y'] == 0
    ctl('fullscreen'); wait(lambda: ctl('panels')['items'])
    ctl('panels-toggle'); wait(lambda: not ctl('panels')['items'])
    assert status()['windows'][0]['geometry']['y'] == 14
    ctl('panels-toggle'); wait(lambda: ctl('panels')['items'])
    passed.append('native tiling/fullscreen/toggle correctly reserve and restore the output area')
    key('meta_l', 'q'); wait(lambda: not status()['windows'])
    guest('''cat >> /tmp/config.toml <<'PANELS'
[[panels.items]]
name = "extra"
anchor = "bottom-right"
modules = ["text:EMBER", "date", "memory", "cpu"]
height = 26
font_size = 12.0
PANELS''')
    ctl('reload'); wait(lambda: len(ctl('panels')['items']) == 2)
    assert ctl('panels')['polling']
    guest("sed -i 's|clock_format = \"%H:%M\"|clock_format = \"%H:%M:%S\"|' /tmp/config.toml")
    ctl('reload'); time.sleep(.2)
    first = ctl('panels')['items'][0]['widgets'][-1]['text']; time.sleep(1.5)
    second = ctl('panels')['items'][0]['widgets'][-1]['text']; assert first != second
    passed.append('adding a second native panel and seconds-clock reload update scanout content')
    guest('''sed -i 's/background = "#211317"/background = "#16432b"/; s/opacity = 0.94/opacity = 1.0/' /tmp/config.toml''')
    ctl('reload'); time.sleep(.3)
    ppm = WORK / 'panels-native.ppm'
    assert 'error' not in qmp('screendump', {'filename': str(ppm)})
    from PIL import Image
    image = Image.open(ppm).convert('RGB')
    assert image.getpixel((9, 9)) == (22, 67, 43), image.getpixel((9, 9))
    image.save(ROOT / 'docs/panels-native-preview.png')
    passed.append('native panel colour/opacity hot reload passes a scanout pixel check')
    report = {'passed': passed, 'panels_off_cold_kib': off, 'default_panel_on_cold_kib': on,
              'panel_process_pss_delta_kib': on['Pss'] - off['Pss'], 'idle_frames_in_three_seconds': frames,
              'panel_process_anonymous_delta_kib': on['Pss_Anon'] - off['Pss_Anon'],
              'comparison_note': 'Anonymous-memory delta isolates most font/runtime cost. Total PSS and whole-guest deltas include variable code/page residency and kernel/cache state, so they are not a precise panel overhead estimate.',
              'default_panel_buffer_bytes': p['buffer_bytes'],
              'scope': 'Isolated 512 MiB QEMU VM, 1920x1080 at 143.981 Hz, software GLES; minimal userspace, audio disabled. Cold compositor processes in the same boot; not physical Gentoo system RAM.'}
    (ROOT / 'docs/panels-native-results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
finally:
    try: ctl('quit')
    except Exception: pass
