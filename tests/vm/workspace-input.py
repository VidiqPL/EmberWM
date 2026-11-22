#!/usr/bin/env python3
"""Hidden X11 games must not intercept input on other workspaces."""
import json
import shlex
import sys
import time
from control import shell, qmp

baseline = '--baseline' in sys.argv

def guest(command):
    return shell(command).replace('\r', '')

def ctl(command):
    text = guest('emberwm ctl ' + shlex.quote(command))
    start = text.find('{\n')
    assert start >= 0, text
    result = json.JSONDecoder().raw_decode(text[start:])[0]
    assert not result.get('error'), result
    return result

def wait(predicate):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (AssertionError, KeyError):
            pass
        time.sleep(.05)
    raise AssertionError('workspace input test timed out')

def events(path):
    return [json.loads(line) for line in guest('cat ' + path).splitlines() if line.startswith('{')]

def clicks(path):
    return sum(e.get('button') == 1 or (e.get('event') == 'pointer-button' and e.get('state') == 1) for e in events(path))

def click():
    for down in (True, False):
        qmp('input-send-event', {'events': [{'type': 'btn', 'data': {'down': down, 'button': 'left'}}]})
        time.sleep(.1)

def move(x, y):
    for _ in range(45):
        p = ctl('status')['pointer']
        dx, dy = x-p['x'], y-p['y']
        if abs(dx) < 2 and abs(dy) < 2:
            return
        qmp('input-send-event', {'events': [
            {'type': 'rel', 'data': {'axis': axis, 'value': int(v/2) or (1 if v > 0 else -1)}}
            for axis, v in [('x', dx), ('y', dy)] if abs(v) > 1]})
        time.sleep(.04)
    raise AssertionError('pointer positioning failed')

try:
    binary = '/usr/bin/emberwm-stacking-baseline' if baseline else '/usr/bin/emberwm'
    guest('emberwm ctl quit; sleep 1; ' + binary + ' --native --config /tmp/config.toml >/tmp/workspace-server.log 2>&1 &')
    status = wait(lambda: ctl('status'))
    x = status['xwayland']
    env = 'DISPLAY=' + shlex.quote(x['display']) + ' XAUTHORITY=' + shlex.quote(x['auth'])
    guest(env + ' ember-x11-window active >/tmp/workspace-parent.log 2>&1 &')
    parent = wait(lambda: next((w for w in ctl('status')['windows'] if w['app_id'] == 'EmberX11Test' and w['focused']), None))
    move(180, 180)
    ctl('workspace 3')
    guest(env + ' ember-x11-window game >/tmp/workspace-game.log 2>&1 & echo $! >/tmp/workspace-game.pid')
    wait(lambda: any(w['app_id'] == 'CrashFixture' and w['focused'] for w in ctl('status')['windows']))
    ctl('workspace 1')
    time.sleep(.3)
    hit = next(json.loads(l) for l in guest(env + ' ember-x11-window hit-test ' + str(parent['x11_id'])).splitlines() if l.startswith('{'))
    before = clicks('/tmp/workspace-parent.log')
    click()
    delivered = clicks('/tmp/workspace-parent.log') - before
    assert hit['hit'] == (not baseline), hit
    assert delivered == (0 if baseline else 1), delivered
    assert any(w['app_id'] == 'CrashFixture' for w in ctl('status')['windows'])
    if not baseline:
        for workspace, target, log in [(3, 'CrashFixture', '/tmp/workspace-game.log'), (1, 'EmberX11Test', '/tmp/workspace-parent.log')] * 3:
            ctl('workspace ' + str(workspace))
            wait(lambda: ctl('status')['keyboard_focus_app'] == target)
            before = clicks(log)
            click()
            assert clicks(log) == before + 1, (workspace, target)
    # Check native pointer delivery after a stationary workspace switch too.
    guest('kill $(cat /tmp/workspace-game.pid)')
    wait(lambda: all(w['app_id'] != 'CrashFixture' for w in ctl('status')['windows']))
    ctl('workspace 2')
    guest('WAYLAND_DISPLAY=' + shlex.quote(status['wayland_display']) + ' ember-pointer-client --app-id native-browser >/tmp/workspace-native.log 2>&1 &')
    wait(lambda: any(w['app_id'] == 'native-browser' and w['focused'] and w['placement_ready'] for w in ctl('status')['windows']))
    ctl('workspace 1')
    move(200, 200)
    ctl('workspace 2')
    time.sleep(.2)
    before = clicks('/tmp/workspace-parent.log')
    click()
    native = clicks('/tmp/workspace-native.log')
    leaked = clicks('/tmp/workspace-parent.log') - before
    assert native == 1, (native, leaked)
    assert leaked == 0, (native, leaked)
    print(json.dumps({'baseline': baseline, 'visible_x11_hit': hit['hit'], 'visible_x11_clicks': delivered,
                      'native_clicks_after_stationary_switch': native, 'hidden_x11_clicks': leaked}, indent=2))
finally:
    try:
        ctl('quit')
    except Exception:
        pass
