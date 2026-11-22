#!/usr/bin/env python3
"""Native appearance/selection reload checks and idle RAM, on the virtual seat only."""
import json, re, shlex, statistics, time, tomllib
from pathlib import Path
from PIL import Image
from control import shell, qmp, WORK

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
passed = []

def guest(command):
    return shell(command).replace('\r', '')

def rpc(command):
    text = guest('emberwm ctl ' + shlex.quote(command))
    start = text.find('{\n')
    assert start >= 0, text
    return json.JSONDecoder().raw_decode(text[start:])[0]

def ctl(command):
    result = rpc(command)
    assert not result.get('error'), result
    return result

def wait(fn, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (AssertionError, KeyError):
            pass
        time.sleep(.06)
    raise AssertionError('native appearance test timed out')

def start(binary='/usr/bin/emberwm', version=VERSION, config='/etc/emberwm/config.toml'):
    try:
        ctl('quit')
        time.sleep(.3)
    except AssertionError:
        pass
    guest(f'cp {config} /tmp/video-rice.toml; {binary} --native --config /tmp/video-rice.toml >/tmp/video-rice.log 2>&1 &')
    return wait(lambda: ctl('status') if ctl('status')['version'] == version else None)

def capture(name):
    path = WORK / (name + '.ppm')
    assert 'error' not in qmp('screendump', {'filename': str(path)})
    image = Image.open(path).convert('RGB')
    image.save(ROOT / 'docs' / (name + '.png'))
    return image

def key(*keys):
    for down, seq in [(True, keys), (False, tuple(reversed(keys)))]:
        events = [{'type': 'key', 'data': {'down': down, 'key': {'type': 'qcode', 'data': k}}} for k in seq]
        assert 'error' not in qmp('input-send-event', {'events': events})
        time.sleep(.1)

def button(down):
    assert 'error' not in qmp('input-send-event', {'events': [{'type': 'btn', 'data': {'down': down, 'button': 'left'}}]})
    time.sleep(.1)

def move(x, y):
    for _ in range(40):
        point = ctl('status')['pointer']
        dx, dy = x - point['x'], y - point['y']
        if abs(dx) < 2 and abs(dy) < 2:
            return
        events = [{'type': 'rel', 'data': {'axis': axis, 'value': int(v / 2) or (1 if v > 0 else -1)}} for axis, v in [('x', dx), ('y', dy)] if abs(v) >= 1]
        assert 'error' not in qmp('input-send-event', {'events': events})
        time.sleep(.04)
    raise AssertionError('virtual pointer did not converge')

def measure(binary='/usr/bin/emberwm', version=VERSION, config='/etc/emberwm/config.toml', profile=None):
    state = start(binary, version, config)
    if profile:
        ctl('rice ' + profile)
    time.sleep(3.2)
    wait(lambda: not ctl('panels')['font_loaded'])
    samples = []
    for _ in range(5):
        text = guest(f"cat /proc/{state['pid']}/smaps_rollup; cat /proc/meminfo")
        values = {k: int(v) for k, v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|MemTotal|MemAvailable):\s+(\d+)', text, re.M)}
        values['guest_used_available'] = values['MemTotal'] - values['MemAvailable']
        samples.append(values)
        time.sleep(.1)
    state = ctl('status')
    assert not state['windows'] and state['audio']['state'] == 'disabled'
    assert not state['launcher']['buffer_bytes'] and not state['wifi']['buffer_bytes'] and not state['ui']['buffer_bytes']
    result = {k: int(statistics.median(s[k] for s in samples)) for k in samples[0]}
    result.update(panel_bytes=state['panels']['buffer_bytes'], panel_font_loaded=state['panels']['font_loaded'], refresh_hz=state['output']['refresh_hz'])
    return result

try:
    start()
    panels = ctl('panels')
    assert panels['items'][0]['x'] == 12 and panels['items'][0]['y'] == 8
    assert panels['items'][0]['height'] == 21 and not panels['polling']
    original = capture('rice-abbey-native')
    guest("sed -i 's/wallpaper_palette = .*/wallpaper_palette = []/' /tmp/video-rice.toml")
    frames = ctl('status')['frames_submitted']
    ctl('reload'); wait(lambda: ctl('status')['frames_submitted'] > frames); time.sleep(.1)
    uncoloured = capture('rice-abbey-original-native')
    assert sum(abs(a-b) for a,b in zip(original.getpixel((1600,300)), uncoloured.getpixel((1600,300)))) > 40
    generation = ctl('status')['theme_generation']
    guest("sed -i 's/wallpaper_palette = .*/wallpaper_palette = [\"invalid\"]/' /tmp/video-rice.toml")
    assert rpc('reload').get('error') and ctl('status')['theme_generation'] == generation
    guest('cp /etc/emberwm/config.toml /tmp/video-rice.toml'); frames = ctl('status')['frames_submitted']
    ctl('reload'); wait(lambda: ctl('status')['frames_submitted'] > frames); time.sleep(.1)
    assert capture('rice-abbey-native').getpixel((1600,300)) == original.getpixel((1600,300))
    passed.append('Native wallpaper palette reload changes pixels without changing the source; empty palette restores original; malformed palette rejects atomically')
    move(250, 170); key('meta_l', 'd'); button(True); move(620,430)
    selection = capture('rice-selection-native')
    rgb = tuple(int(ctl('config')['theme']['selection'][i:i+2], 16) for i in (1,3,5))
    base = original.getpixel((400,300)); actual = selection.getpixel((400,300))
    expected = tuple(round(a*.6 + b*.4) for a,b in zip(base,rgb))
    assert all(abs(a-b) <= 2 for a,b in zip(actual,expected)), (base,actual,expected)
    key('esc'); button(False)
    assert not ctl('status')['draw_armed'] and not ctl('status')['windows']
    passed.append('Real Super+D and mouse drag render the configured translucent selection; Escape cancels without spawning')
    ctl('notify Recording started|Native compact notification')
    card = wait(lambda: ctl('status')['ui']['cards'])[0]
    assert card['y'] == 12 and card['width'] == 360 and card['height'] == 55, card
    capture('rice-notification-native')
    move(card['x'] + card['width'] - 17, card['y'] + 12); button(True); button(False)
    wait(lambda: not ctl('status')['ui']['cards'])
    passed.append('Compact two-row notification is 360x55 at top-right margin; virtual mouse close dismisses it')
    measurements = {}
    baseline = ROOT / 'artifacts/native-vm/root/usr/bin/ember-before-video-rice'
    if baseline.exists():
        measurements['before_0.11.1'] = measure('/usr/bin/ember-before-video-rice','0.11.1','/etc/emberwm/before-video-rice.toml')
        capture('rice-before-native')
        measurements['new_same_old_config'] = measure(config='/etc/emberwm/before-video-rice.toml')
    measurements['new_abbey_default'] = measure()
    measurements['new_minimal'] = measure(profile='minimal')
    passed.append('Settled native idle uses no popup buffers or retained panel font; optional minimal profile releases the entire panel')
    report = {'passed': passed, 'measurements_kib': measurements, 'scope': 'Native 1920x1080 / 143.981 Hz in a 512 MiB VM. PSS is compositor-only; guest_used_available is whole guest. No windows; audio disabled; physical Gentoo, audio and portal helper costs are not established.'}
    (ROOT / 'docs/video-rice-results.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
finally:
    try: ctl('quit')
    except AssertionError: pass
