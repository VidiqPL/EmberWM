#!/usr/bin/env python3
"""Real native presentation callbacks on an isolated virtual output."""
import json, re, shlex, time, tomllib
from pathlib import Path
from control import shell

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']

def guest(command):
    return shell(command).replace('\r', '')

def ctl(command):
    text = guest('emberwm ctl ' + shlex.quote(command))
    start = text.find('{\n')
    assert start >= 0, text
    data = json.JSONDecoder().raw_decode(text[start:])[0]
    assert not data.get('error'), data
    return data

def wait(fn, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except (AssertionError, OSError):
            pass
        time.sleep(.06)
    raise AssertionError('native presentation test timed out')

try:
    state = wait(lambda: ctl('status') if ctl('status')['version'] == VERSION else None)
    expected = round(1_000_000_000 / state['output']['refresh_hz'])
    text = guest(f"WAYLAND_DISPLAY={shlex.quote(state['wayland_display'])} ember-frame-client")
    frames = json.loads(next(line for line in text.splitlines() if line.startswith('{"frames":')))
    assert frames['presentation_available'] and frames['clock_id'] == 1, frames
    assert frames['presented'] > 300 and frames['invalid_feedback'] == 0, frames
    assert frames['discarded'] < 5 and 125 < frames['fps'] < 155, frames
    assert abs(frames['min_refresh_ns'] - expected) <= 2, (frames, expected)
    assert frames['min_refresh_ns'] == frames['max_refresh_ns'], frames
    # Colour cycles must preserve the image and native palette at every step.
    original = ctl('config')['theme']
    for name in ['midnight', 'forest', 'mono', 'light', 'red']:
        ctl('theme ' + name)
        actual = ctl('config')['theme']
        for field in ['wallpaper', 'wallpaper_mode', 'wallpaper_palette']:
            assert actual[field] == original[field], (name, field, actual)
    ctl('theme default')
    time.sleep(2.5)
    wait(lambda: not ctl('panels')['font_loaded'])
    state = ctl('status')
    assert not state['windows'] and not state['ui']['buffer_bytes']
    text = guest(f"cat /proc/{state['pid']}/smaps_rollup")
    memory = {k: int(v) for k, v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)', text, re.M)}
    frames_before = state['frames_submitted']
    time.sleep(.3)
    # An occasional clock minute update is allowed, continuous idle repaint is not.
    assert ctl('status')['frames_submitted'] - frames_before <= 1
    report = {
        'passed': ['Native wp_presentation reports CLOCK_MONOTONIC and real flip timestamps/sequences with no invalid feedback',
                   'Presentation refresh matches the configured 144 Hz output; frame callbacks sustain near 144 fps',
                   'Every colour preset and return to red preserves wallpaper, framing and native palette',
                   'Closing the client releases it; idle panel font expires without continuous repaint or popup buffers'],
        'client': frames, 'idle_pss_kib': memory,
        'scope': 'Native 1920x1080 / 143.981 Hz, 512 MiB isolated VM, synthetic Wayland client, audio disabled. Does not establish real Discord/browser scrolling performance.'}
    (ROOT / 'docs/presentation-results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
finally:
    try: ctl('quit')
    except (AssertionError, OSError): pass
