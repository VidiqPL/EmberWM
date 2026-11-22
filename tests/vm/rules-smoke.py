#!/usr/bin/env python3
"""Native physical input for F11 and all ten workspaces, plus client fullscreen requests."""
import json,shlex,time
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2];GUEST=WORK/'root';passed=[]
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
    text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text
    d=json.JSONDecoder().raw_decode(text[start:])[0];assert not d.get('error'),d;return d
def state():return ctl('status')
def wait(fn,timeout=10):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (AssertionError,IndexError,KeyError):pass
        time.sleep(.06)
    raise AssertionError('native rule/input condition timed out')
def key(*keys):
    for down,sequence in [(True,keys),(False,tuple(reversed(keys)))]:
        events=[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in sequence]
        result=qmp('input-send-event',{'events':events});assert 'error' not in result,result
        time.sleep(.15)
def win(app):return next((w for w in state()['windows']if w['app_id']==app),None)
def command(fifo,cmd):guest('printf %s\\n '+shlex.quote(cmd)+' > '+shlex.quote(fifo))
def client_events(log):
    text=guest('cat '+shlex.quote(log));events=[]
    for line in text.splitlines():
        if line.startswith('{"event"'):events.append(json.loads(line))
    return events
def spawn(app,fifo,log):
    display=state()['wayland_display'];guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id {shlex.quote(app)} --control {shlex.quote(fifo)} >{shlex.quote(log)} 2>&1 &')
    wait(lambda:(w if w['placement_ready']else None)if(w:=win(app))else None)
base=(GUEST/'etc/emberwm/config.toml').read_text()
extra='''
[[window_rules]]
app_id = "org.vinegarhq.Sober"
width = 560
height = 340
center = true

[[window_rules]]
app_id = "rule-native"
workspace = 10
width = 500
height = 300
x = 110
y = 110
'''
config=base+extra
(GUEST/'assets/rules-session.toml').write_text(config)
try:
    guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /assets/rules-session.toml /tmp/config.toml; emberwm --native --config /tmp/config.toml >/tmp/ember-rules.log 2>&1 &')
    wait(lambda:state()['session_active']);assert state()['workspace_count']==10
    time.sleep(1)  # Allow libinput's initial udev device enumeration to finish.
    for n in range(1,11):
        key('meta_l',str(n%10));s=state();assert s['workspace']==n,(n,s['workspace'],s['session_active'])
    key('meta_l','1');ctl('terminal');wait(lambda:len(state()['windows'])==1)
    passed.append('real Super+1 through Super+9 and Super+0 select all ten workspaces')
    spawn('org.vinegarhq.Sober','/tmp/sober-test.fifo','/tmp/sober-test.log')
    original=win('org.vinegarhq.Sober')['geometry'];assert original['width']==560 and original['height']==340
    key('f11');wait(lambda:win('org.vinegarhq.Sober')['fullscreen'] and client_events('/tmp/sober-test.log')[-1]['fullscreen'])
    assert win('org.vinegarhq.Sober')['geometry']=={'x':0,'y':0,'width':1920,'height':1080}
    assert client_events('/tmp/sober-test.log')[-1]['f11_keys']==0 and not state()['panels']['items']
    key('f11');wait(lambda:not win('org.vinegarhq.Sober')['fullscreen'])
    assert win('org.vinegarhq.Sober')['geometry']==original
    passed.append('unmodified physical F11 toggles fullscreen and restores floating geometry, with a Sober app-ID protocol client')
    key('meta_l','shift','0');assert win('org.vinegarhq.Sober')['workspace']==10 and not win('org.vinegarhq.Sober')['visible']
    key('meta_l','0');assert state()['workspace']==10 and win('org.vinegarhq.Sober')['focused']
    passed.append('real Super+Shift+0 sends the focused app to workspace 10; Super+0 reveals and focuses it')
    no_binding=config.replace('[[bindings]]\nkey = "F11"\naction = "fullscreen"\nmodifier = false\n','')
    assert no_binding!=config
    (GUEST/'assets/rules-client-f11.toml').write_text(no_binding);guest('cp /assets/rules-client-f11.toml /tmp/config.toml');ctl('reload')
    key('f11');wait(lambda:win('org.vinegarhq.Sober')['fullscreen'] and client_events('/tmp/sober-test.log')[-1]['f11_keys']==1)
    key('f11');wait(lambda:not win('org.vinegarhq.Sober')['fullscreen'] and client_events('/tmp/sober-test.log')[-1]['f11_keys']==2)
    assert win('org.vinegarhq.Sober')['geometry']==original
    passed.append('removing the F11 binding forwards it to the app; app-originated fullscreen and unfullscreen requests work')
    key('meta_l','1');ctl('fullscreen');before=next(w for w in state()['windows']if w['focused'])
    spawn('rule-native','/tmp/rule-native.fifo','/tmp/rule-native.log')
    assert state()['workspace']==1 and next(w for w in state()['windows']if w['focused'])['app_id']==before['app_id']
    assert win('rule-native')['workspace']==10 and not win('rule-native')['visible']
    wait(lambda:client_events('/tmp/rule-native.log')[-1]['width']==500)
    passed.append('background native rules configure hidden windows while preserving the current fullscreen app')
    changed=no_binding.replace('app_id = "rule-native"\nworkspace = 10','app_id = "rule-native"\nworkspace = 9\nfollow = true\nfullscreen = true')
    (GUEST/'assets/rules-changed.toml').write_text(changed);guest('cp /assets/rules-changed.toml /tmp/config.toml');ctl('reload')
    wait(lambda:state()['workspace']==9 and win('rule-native')['fullscreen']);assert win('rule-native')['focused']
    ctl('shortcuts-toggle');wait(lambda:state()['ui']['help'])
    assert any(r['keys']=='Super+0' and r['action']=='workspace 10'for r in state()['ui']['rows'])
    ppm=WORK/'rules-workspaces.ppm';assert 'error' not in qmp('screendump',{'filename':str(ppm)})
    from PIL import Image
    Image.open(ppm).save(ROOT/'docs/rules-workspaces-native.png')
    passed.append('live rules change workspace/fullscreen/focus and the generated shortcut sheet includes workspace 10')
    report={'passed':passed,'scope':'Private native VM, physical virtual keyboard input and real Wayland protocol clients. org.vinegarhq.Sober matches the installed Flatpak ID; actual Roblox/Sober gameplay was not launched.'}
    (ROOT/'docs/rules-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
    try:ctl('quit')
    except Exception:pass
