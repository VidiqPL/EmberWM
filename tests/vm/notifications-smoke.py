#!/usr/bin/env python3
"""Notifications and shortcut UI through native KMS, libinput and a private guest bus."""
import json,re,time,shlex
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
passed=[]
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
    text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text
    value=json.JSONDecoder().raw_decode(text[start:])[0];assert not value.get('error'),value;return value
def state():return ctl('status')
def wait(fn,timeout=10):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (AssertionError,KeyError):pass
        time.sleep(.1)
    raise AssertionError('native UI test timed out')
def key(*keys):
    result=qmp('send-key',{'keys':[{'type':'qcode','data':k} for k in keys],'hold-time':100});assert 'error' not in result,result;time.sleep(.15)
def move_to(x,y):
    for _ in range(30):
        p=state()['pointer'];dx=x-p['x'];dy=y-p['y']
        if abs(dx)<2 and abs(dy)<2:return
        events=[]
        for axis,d in [('x',dx),('y',dy)]:
            if abs(d)>=1:events.append({'type':'rel','data':{'axis':axis,'value':int(d/2) or (1 if d>0 else -1)}})
        result=qmp('input-send-event',{'events':events});assert 'error' not in result,result;time.sleep(.05)
    raise AssertionError(('pointer did not converge',state()['pointer'],x,y))
def click():
    for down in [True,False]:
        result=qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});assert 'error' not in result;time.sleep(.1)
def memory():
    text=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
    return {k:int(v) for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
try:
    guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/config.toml; emberwm --native --config /tmp/config.toml >/tmp/ember-ui.log 2>&1 &')
    wait(lambda:state()['notifications']['server_running'])
    caps=guest('gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.GetCapabilities')
    assert "'body'" in caps and "'actions'" in caps,caps
    passed.append('native server owns the private guest bus and answers standard D-Bus clients')
    ctl('terminal');wait(lambda:len(state()['windows'])==1)
    key('meta_l','slash');s=wait(lambda:(s if s['ui']['shortcuts_open'] and s['ui']['help'] else None) if (s:=state()) else None)
    assert any(r['keys']=='Super+/' for r in s['ui']['rows'])
    first=s['ui']['rasterizations'];time.sleep(1.2);assert state()['ui']['rasterizations']==first
    passed.append('physical Super+/ opens the generated sheet without continuously rasterizing it')
    guest('''sed -i 's/key = "Return"/key = "t"/' /tmp/config.toml''');ctl('reload')
    wait(lambda:any(r['keys']=='Super+t' and r['action']=='terminal' for r in state()['ui']['rows']))
    key('esc');wait(lambda:not state()['ui']['shortcuts_open'])
    passed.append('open sheet refreshes from reloaded bindings; native Escape closes it')
    text=guest('''gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.Notify -- 'Native tests' 0 '' 'Notification action' 'Click Copy below' "['copy', 'Copy']" '{}' 0''')
    assert 'uint32' in text,text
    s=wait(lambda:(s if s['ui']['cards'] else None) if (s:=state()) else None);button=s['ui']['cards'][0]['buttons'][0]
    move_to(button['x']+button['width']/2,button['y']+button['height']/2);click()
    wait(lambda:not state()['notifications']['items'])
    passed.append('native mouse clicks a real D-Bus notification action and closes its card')
    ctl('notify EmberWM | Native notifications and live shortcut reference')
    key('meta_l','slash');wait(lambda:state()['ui']['help'])
    ppm=WORK/'notifications-shortcuts.ppm';assert 'error' not in qmp('screendump',{'filename':str(ppm)})
    from PIL import Image
    Image.open(ppm).save(ROOT/'docs/notifications-shortcuts-native.png')
    key('esc');ctl('notifications-clear');key('meta_l','q');wait(lambda:not state()['windows'])
    before=memory()
    for _ in range(30):ctl('shortcuts-toggle');ctl('shortcuts-toggle')
    time.sleep(.3);after=memory();assert after['Pss_Anon']-before['Pss_Anon']<10*1024,(before,after)
    assert state()['ui']['buffer_bytes']==0 and not state()['ui']['font_loaded']
    passed.append('native open/close cycles release overlay resources and stay within memory-growth budget')
    report={'passed':passed,'memory_before_cycles_kib':before,'memory_after_cycles_kib':after,'scope':'Isolated native VM with private guest D-Bus, virtual input/GPU, audio disabled. Host desktop remains running.'}
    (ROOT/'docs/notifications-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
    try:ctl('quit')
    except Exception:pass
