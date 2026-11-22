#!/usr/bin/env python3
"""Native KMS/libinput popup test. Only virtual guest devices and fake Wi-Fi data."""
import json,re,shlex,time
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(command):
    text=guest('emberwm ctl '+shlex.quote(command));start=text.find('{\n');assert start>=0,text
    data=json.JSONDecoder().raw_decode(text[start:])[0];assert not data.get('error'),data;return data
def wait(fn,timeout=12):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            value=fn()
            if value:return value
        except (AssertionError,KeyError):pass
        time.sleep(.08)
    raise AssertionError('native Wi-Fi test timed out')
def wifi():return ctl('wifi')
def key(*keys):
    result=qmp('send-key',{'keys':[{'type':'qcode','data':k} for k in keys],'hold-time':60});assert 'error' not in result,result;time.sleep(.12)
def move_to(x,y):
    for _ in range(30):
        p=ctl('status')['pointer'];dx=x-p['x'];dy=y-p['y']
        if abs(dx)<2 and abs(dy)<2:return
        events=[{'type':'rel','data':{'axis':axis,'value':int(d/2) or (1 if d>0 else -1)}} for axis,d in [('x',dx),('y',dy)] if abs(d)>=1]
        assert 'error' not in qmp('input-send-event',{'events':events});time.sleep(.04)
    raise AssertionError('pointer did not converge')
def click(button):
    move_to(button['x']+button['width']/2,button['y']+button['height']/2)
    for down in (True,False):
        assert 'error' not in qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});time.sleep(.08)
def ready():
    s=wifi();return s if s['open'] and not s['busy'] and len(s['networks'])==2 and s['view'] else None
def memory():
    text=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
    return {k:int(v) for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
def keys_received():
    text=guest('cat /tmp/wifi-client.log');return [int(v) for v in re.findall(r'"event":"key","code":(\d+)',text)]
passed=[]
try:
    guest('emberwm ctl quit; sleep 1; mkdir -p /tmp/wifi-bin; cp /usr/bin/fixture-nmcli /tmp/wifi-bin/nmcli; chmod 755 /tmp/wifi-bin/nmcli; cp /etc/emberwm/config.toml /tmp/wifi-config.toml')
    # The default fixture's active AP becomes unconnected so we can type a password.
    guest(r"sed -i 's/\*:Ember/:Ember/' /tmp/wifi-bin/nmcli")
    # Let the client see F11 outside the modal, to check modal key suppression.
    guest("sed -i 's/key = \"F11\"/key = \"F12\"/' /tmp/wifi-config.toml")
    guest('PATH=/tmp/wifi-bin:$PATH DBUS_SYSTEM_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" emberwm --native --config /tmp/wifi-config.toml >/tmp/ember-wifi.log 2>&1 &')
    wait(lambda:ctl('status')['version']=='0.9.0')
    display=ctl('status')['wayland_display']
    guest(f'WAYLAND_DISPLAY={shlex.quote(display)} ember-wm-client --app-id wifi-input-test --log-keys >/tmp/wifi-client.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==1)
    key('meta_l','dot');s=wait(ready);assert s['networks'][1]['ssid']=='Cafe: guest'
    count=s['rasterizations'];time.sleep(.4);assert wifi()['rasterizations']==count
    passed.append('Real Super+. opens native popup; unchanged display is cached')
    key('ret');wait(lambda:wifi()['selected'] is not None)
    before=keys_received()
    for keys in [('shift','a'),('b',),('c',),('1',),('2',),('f11',)]:key(*keys)
    assert wifi()['password_entered'];assert keys_received()==before
    assert not ctl('status')['windows'][0]['fullscreen']
    key('ctrl','u');assert not wifi()['password_entered'];key('a');key('backspace');assert not wifi()['password_entered']
    passed.append('Password input is masked; Ctrl+U/backspace edit it; app receives no modal key presses/releases')
    for k in 'testpass':key(k)
    shot=ROOT/'docs/wifi-password-native.png'
    ppm=WORK/'wifi-password.ppm';assert 'error' not in qmp('screendump',{'filename':str(ppm)})
    from PIL import Image
    Image.open(ppm).save(shot)
    key('ret');wait(lambda:not wifi()['busy']);assert wifi()['failed']
    assert 'testpass' not in json.dumps(wifi())
    passed.append('Unavailable network service reports an error; password is cleared and stays out of status')
    key('esc');wait(lambda:wifi()['selected'] is None)
    s=wifi();button=next(b for b in s['buttons'] if b['action']=='select' and b['index']==1);click(button);wait(lambda:wifi()['selected'] and wifi()['selected']['ssid']=='Cafe: guest')
    click(next(b for b in wifi()['buttons'] if b['action']=='back'));wait(lambda:wifi()['selected'] is None)
    click(next(b for b in wifi()['buttons'] if b['action']=='close'));wait(lambda:not wifi()['open'])
    key('f11');wait(lambda:ctl('status')['windows'][0]['fullscreen'])
    passed.append('Native mouse selects networks/back/close; normal app keyboard input resumes afterward')
    for _ in range(3):key('meta_l','dot');wait(ready);key('meta_l','dot');wait(lambda:not wifi()['busy'])
    before_memory=memory()
    for _ in range(20):ctl('wifi-toggle');wait(ready);ctl('wifi-toggle');wait(lambda:not wifi()['busy'])
    after_memory=memory();s=wifi();assert not s['font_loaded'] and s['buffer_bytes']==0 and not s['networks'] and not s['password_entered']
    assert after_memory['Pss_Anon']-before_memory['Pss_Anon']<4096,(before_memory,after_memory)
    passed.append('20 native open/close cycles release UI resources and remain within 4 MiB anonymous growth')
    result={'passed':passed,'before_cycles_kib':before_memory,'after_cycles_kib':after_memory,'anonymous_growth_kib':after_memory['Pss_Anon']-before_memory['Pss_Anon'],'scope':'Isolated 512 MiB QEMU guest, native DRM/KMS and virtual libinput; fake Wi-Fi scan data; no host display/input/network changes'}
    (ROOT/'docs/wifi-native-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
finally:
    try:ctl('quit')
    except Exception:pass
