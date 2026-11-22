#!/usr/bin/env python3
"""Actual scratchpad bindings, virtual mouse stacking and hidden-window keyboard isolation."""
import json, shlex, time, tomllib
from pathlib import Path
from PIL import Image
from control import shell, qmp, WORK

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']

def guest(command): return shell(command).replace('\r', '')
def ctl(command):
    text = guest('emberwm ctl ' + shlex.quote(command)); start = text.find('{\n')
    assert start >= 0, text
    result = json.JSONDecoder().raw_decode(text[start:])[0]
    assert not result.get('error'), result
    return result
def state(): return ctl('status')
def win(app): return next((w for w in state()['windows'] if w['app_id'] == app), None)
def wait(fn, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = fn()
            if value: return value
        except (OSError, AssertionError, KeyError): pass
        time.sleep(.05)
    raise AssertionError('native scratchpad operation timed out')
def key(*keys):
    for down, sequence in [(True, keys), (False, tuple(reversed(keys)))]:
        events = [{'type': 'key', 'data': {'down': down, 'key': {'type': 'qcode', 'data': k}}} for k in sequence]
        assert 'error' not in qmp('input-send-event', {'events': events})
        time.sleep(.1)
def button(down):
    assert 'error' not in qmp('input-send-event', {'events': [{'type':'btn', 'data':{'down':down, 'button':'left'}}]})
    time.sleep(.08)
def move(x, y):
    for _ in range(40):
        p = state()['pointer']; dx, dy = x-p['x'], y-p['y']
        if abs(dx)<2 and abs(dy)<2: return
        events = [{'type':'rel', 'data':{'axis':axis, 'value':int(v/2) or (1 if v>0 else -1)}} for axis,v in [('x',dx),('y',dy)] if abs(v)>=1]
        assert 'error' not in qmp('input-send-event', {'events':events})
        time.sleep(.035)
    raise AssertionError('virtual mouse did not converge')
def command(app, text): guest('printf \'%s\n\' ' + shlex.quote(text) + ' >/tmp/' + app + '.fifo')
def events(app):
    return [json.loads(line) for line in guest('cat /tmp/' + app + '.log').splitlines() if line.startswith('{"event":')]
def spawn(app):
    display = state()['wayland_display']
    guest(f'WAYLAND_DISPLAY={display} ember-pointer-client --app-id {app} --log-keys --control /tmp/{app}.fifo >/tmp/{app}.log 2>&1 &')
    return wait(lambda: w if (w := win(app)) and w['placement_ready'] else None)
def capture(name):
    path = WORK / (name + '.ppm')
    assert 'error' not in qmp('screendump', {'filename':str(path)})
    image = Image.open(path).convert('RGB'); image.save(ROOT/'docs'/(name+'.png'))
    return image

passed = []
try:
    wait(lambda: state() if state()['version'] == VERSION else None)
    spawn('scratch-base'); spawn('scratch-overlay')
    key('meta_l', 'shift', 'g')
    w = wait(lambda: win('scratch-overlay') if win('scratch-overlay')['scratchpad'] else None)
    assert w['visible'] and w['focused'] and w['floating']
    command('scratch-overlay', 'pattern'); time.sleep(.2)
    rect = w['geometry']; point = (rect['x']+50, rect['y']+60)
    move(70,100); button(True); button(False)
    assert win('scratch-base')['focused']
    assert capture('scratchpad-native').getpixel(point) == (50,60,48)
    passed.append('Real Super+Shift+G marks the focused window; clicking the normal app focuses it while scratchpad pixels remain above it')
    key('meta_l','g')
    assert not win('scratch-overlay')['visible'] and win('scratch-base')['focused']
    assert capture('scratchpad-hidden-native').getpixel(point) == (39,24,36)
    key('a')
    wait(lambda: any(e.get('event')=='key' and e.get('code')==30 and e.get('state')==1 for e in events('scratch-base')))
    assert not any(e.get('event')=='key' and e.get('code')==30 for e in events('scratch-overlay'))
    key('meta_l','g'); assert win('scratch-overlay')['visible'] and win('scratch-overlay')['focused']
    assert win('scratch-overlay')['geometry'] == rect
    passed.append('Real Super+G removes the overlay pixels and keyboard focus; normal typing reaches only the normal app, and the next press restores the existing overlay')
    move(rect['x']+80, rect['y']+100); button(True); button(False)
    command('scratch-overlay','lock'); wait(lambda: state()['pointer_locked'])
    key('meta_l','g'); assert not win('scratch-overlay')['visible'] and not state()['pointer_locked']
    wait(lambda: any(e.get('event')=='unlocked' for e in events('scratch-overlay')))
    key('meta_l','g'); command('scratch-overlay','unlock')
    for request in ['move','resize']:
        shown = win('scratch-overlay')['geometry']
        move(shown['x']+80,shown['y']+100); button(True)
        command('scratch-overlay',request); time.sleep(.15)
        key('meta_l','g'); move(80,110); button(False)
        assert not win('scratch-overlay')['visible'], (request,state())
        assert win('scratch-overlay')['geometry']==shown, (request,shown,win('scratch-overlay'))
        key('meta_l','g'); assert win('scratch-overlay')['focused']
    passed.append('Hiding releases game pointer locks and cancels real xdg move/resize grabs; moving the mouse cannot remap or resize a hidden member')
    key('meta_l','3'); assert state()['workspace']==3 and win('scratch-overlay')['visible']
    key('meta_l','g'); assert not win('scratch-overlay')['visible']
    key('meta_l','2'); key('meta_l','g'); assert win('scratch-overlay')['focused']
    key('meta_l','shift','g')
    assert not win('scratch-overlay')['scratchpad'] and not win('scratch-overlay')['floating']
    assert win('scratch-overlay')['workspace']==2 and win('scratch-overlay')['visible']
    assert state()['scratchpad']=={'visible':False,'windows':0}
    key('meta_l','slash'); wait(lambda: state()['ui']['rows'])
    assert any(r['keys']=='Super+Shift+g' and r['action']=='scratchpad-mark' for r in state()['ui']['rows'])
    assert any(r['keys']=='Super+g' and r['action']=='scratchpad-toggle' for r in state()['ui']['rows'])
    key('esc')
    passed.append('Scratchpad follows real workspace bindings, summons on another workspace, unmarks into that normal layout and appears in the generated help')
    report = {'passed':passed, 'scope':'Isolated native 1920x1080/143.981 Hz VM, virtual keyboard/mouse and synthetic Wayland clients. No host input, app or clipboard changed.'}
    (ROOT/'docs/scratchpad-native-results.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
finally:
    try: ctl('quit')
    except (OSError, AssertionError): pass
