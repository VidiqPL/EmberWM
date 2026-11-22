#!/usr/bin/env python3
"""Native virtual input test for the shared menu; never changes host Wi-Fi or power."""
import json,re,shlex,time,tomllib
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
def guest(c):return shell(c).replace('\r','')
def ctl(c):
 t=guest('emberwm ctl '+shlex.quote(c));i=t.find('{\n');assert i>=0,t
 r=json.JSONDecoder().raw_decode(t[i:])[0];assert not r.get('error'),r;return r
def wait(f):
 end=time.monotonic()+15
 while time.monotonic()<end:
  try:
   r=f()
   if r:return r
  except (AssertionError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('control-center operation timed out')
def key(*keys):
 assert 'error'not in qmp('send-key',{'keys':[{'type':'qcode','data':k}for k in keys],'hold-time':60});time.sleep(.13)
def move(x,y):
 for _ in range(45):
  p=ctl('status')['pointer'];dx=x-p['x'];dy=y-p['y']
  if abs(dx)<2 and abs(dy)<2:return
  qmp('input-send-event',{'events':[{'type':'rel','data':{'axis':a,'value':int(v/2)or(1 if v>0 else -1)}}for a,v in [('x',dx),('y',dy)]if abs(v)>1]});time.sleep(.03)
 raise AssertionError('pointer positioning failed')
def click(r,right=False):
 move(r['x']+r['width']/2,r['y']+r['height']/2)
 for down in [True,False]:qmp('input-send-event',{'events':[{'type':'btn','data':{'button':'right'if right else'left','down':down}}]});time.sleep(.08)
def logged_keys():return re.findall(r'"event":"key","code":(\d+)',guest('cat /tmp/menu-client.log'))
def ready(tab):
 m=ctl('menu');s=ctl('status');child=s[tab]
 return m if m['open']and m['tab']==tab and not child['busy']and child['buffer_bytes'] else None
def memory():
 t=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
 return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon):\s+(\d+)',t,re.M)}
def closed():
 s=ctl('status');return not s['menu']['open']and all(not s[n]['busy']and s[n]['buffer_bytes']==0 for n in ['wifi','power'])and not s['tray']['open']
passed=[]
helper=WORK/'root/usr/local/libexec/emberwm-power'
helper.parent.mkdir(parents=True,exist_ok=True)
assert not helper.exists(), 'fixture helper path already occupied'
helper.write_text('# Test-only presence marker; fake pkexec never executes this.\n')
try:
 fixture=(ROOT/'tests/wifi-fixture-nmcli.sh').read_text().replace('*:Ember',':Ember')
 guest('emberwm ctl quit; sleep 1; mkdir -p /tmp/menu-bin; printf %s '+shlex.quote(fixture)+' > /tmp/menu-bin/nmcli; chmod 755 /tmp/menu-bin/nmcli; cp /etc/emberwm/config.toml /tmp/menu-config.toml')
 guest("sed -i 's/key = \"F11\"/key = \"F12\"/; s/\\[panels\\]/[panels]/' /tmp/menu-config.toml")
 # Synthetic cpufreq is bind-mounted inside this disposable guest only.
 guest('mkdir -p /tmp/fake-cpufreq/policy0; printf %s fixture-epp > /tmp/fake-cpufreq/policy0/scaling_driver; printf %s powersave > /tmp/fake-cpufreq/policy0/scaling_governor; printf %s balance_performance > /tmp/fake-cpufreq/policy0/energy_performance_preference; printf %s '+shlex.quote('performance balance_performance power')+' > /tmp/fake-cpufreq/policy0/energy_performance_available_preferences; /bin/busybox mount --bind /tmp/fake-cpufreq /sys/devices/system/cpu/cpufreq')
 fake='#!/bin/sh\nprintf "%s\\n" "$*" > /tmp/power-auth-args\nsleep 2\necho "Authorization denied by test fixture" >&2\nexit 126\n'
 guest('printf %s '+shlex.quote(fake)+' > /tmp/menu-bin/pkexec; chmod 755 /tmp/menu-bin/pkexec')
 guest('PATH=/tmp/menu-bin:$PATH DBUS_SYSTEM_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" emberwm --native --config /tmp/menu-config.toml >/tmp/menu-server.log 2>&1 &')
 s=wait(lambda:ctl('status'));assert s['version']==tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
 guest('WAYLAND_DISPLAY='+shlex.quote(s['wayland_display'])+' ember-wm-client --app-id menu-input-test --log-keys >/tmp/menu-client.log 2>&1 &')
 wait(lambda:len(ctl('status')['windows'])==1)
 key('meta_l','dot');m=wait(lambda:ready('wifi'));assert len(m['tabs'])==3
 assert ctl('wifi')['view']==m['body']
 qmp('screendump',{'filename':str(WORK/'menu-wifi.ppm')})
 key('ret');wait(lambda:ctl('wifi')['selected']is not None)
 before=logged_keys()
 for k in ['a','b','c','f11']:key(k)
 assert ctl('wifi')['password_entered'];assert logged_keys()==before
 key('ctrl','tab');wait(lambda:ready('tray'));assert not ctl('wifi')['password_entered']and not ctl('wifi')['open']
 assert ctl('menu')['body']==m['body']
 passed.append('Super+. opens shared chrome; Wi-Fi fits its body; password keys do not reach the app and switching tabs clears the secret')
 qmp('screendump',{'filename':str(WORK/'menu-tray.ppm')})
 key('alt','3');wait(lambda:ready('power'))
 qmp('screendump',{'filename':str(WORK/'menu-power.ppm')})
 p=ctl('power');click(next(b for b in p['buttons']if b['index']==6))
 wait(lambda:ctl('power')['confirmation']=='power-off');assert not ctl('power')['busy']
 key('esc');assert ctl('power')['confirmation']is None and ctl('menu')['open']
 key('esc');wait(closed)
 before=logged_keys();key('w');wait(lambda:len(logged_keys())>len(before))
 passed.append('Ctrl+Tab and Alt+3 switch tabs; shutdown requires confirmation; Escape cancels safely and returns input to the app')
 ctl('power-toggle');wait(lambda:ready('power'));click(next(b for b in ctl('power')['buttons']if b['index']==1))
 wait(lambda:not ctl('menu')['open']);assert ctl('power')['busy']
 before=logged_keys();key('x');wait(lambda:len(logged_keys())>len(before))
 wait(lambda:not ctl('power')['busy'])
 wait(lambda:any('Authorization denied by test fixture' in n['body'] for n in ctl('notifications')['items']))
 assert 'balanced' in guest('cat /tmp/power-auth-args')
 assert 'balance_performance' in guest('cat /tmp/fake-cpufreq/policy0/energy_performance_preference')
 passed.append('Profile action releases modal input before authorization, calls the fixed helper, and reports denial without changing settings')

 # Warm caches before checking repeated open/close growth.
 for _ in range(3):ctl('power-toggle');wait(lambda:ready('power'));ctl('menu-toggle');wait(closed)
 baseline=memory()
 for _ in range(20):ctl('power-toggle');wait(lambda:ready('power'));ctl('menu-toggle');wait(closed)
 after=memory();assert after['Pss_Anon']-baseline['Pss_Anon']<2048,(baseline,after)
 s=ctl('status');assert s['menu']['buffer_bytes']==0
 passed.append('Twenty power-tab cycles release overlay buffers and leave no power worker or refresh timer while closed')
 # Removing a tab updates keyboard traversal and disabling the menu closes it.
 guest(r"sed -i 's/tabs = \[\"wifi\", \"tray\", \"power\"\]/tabs = [\"tray\", \"power\"]/' /tmp/menu-config.toml")
 ctl('reload');key('meta_l','dot');wait(lambda:ready('tray'));assert len(ctl('menu')['tabs'])==2
 key('ctrl','shift','tab');wait(lambda:ready('power'))
 guest("sed -i '/\\[menu\\]/,/\\[xwayland\\]/s/enabled = true/enabled = false/' /tmp/menu-config.toml")
 ctl('reload');wait(closed)
 passed.append('Live config removes/reorders tabs and disabling the control center closes all child UI')
 print(json.dumps({'passed':passed,'anonymous_growth_KiB':after['Pss_Anon']-baseline['Pss_Anon'],'scope':'Native DRM virtual seat, fake Wi-Fi, no session power action confirmed'},indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
 try:guest("/bin/busybox umount /sys/devices/system/cpu/cpufreq")
 except Exception:pass
 helper.unlink(missing_ok=True)
