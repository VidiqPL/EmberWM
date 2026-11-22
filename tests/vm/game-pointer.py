#!/usr/bin/env python3
"""Native relative input/constraints, escape paths and live themes using virtual hardware only."""
import json,re,shlex,time,tomllib
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
VERSION=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
def guest(command):return shell(command).replace('\r','')
def ctl(command):
 text=guest('emberwm ctl '+shlex.quote(command));start=text.find('{\n');assert start>=0,text
 data=json.JSONDecoder().raw_decode(text[start:])[0];assert not data.get('error'),data;return data
def wait(fn,timeout=10):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   result=fn()
   if result:return result
  except (AssertionError,KeyError):pass
  time.sleep(.03)
 raise AssertionError('native mouse/theme operation timed out')
def key(*keys):
 for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
  result=qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});assert 'error'not in result,result;time.sleep(.07)
def move(x,y):
 result=qmp('input-send-event',{'events':[{'type':'rel','data':{'axis':'x','value':x}},{'type':'rel','data':{'axis':'y','value':y}}]});assert 'error'not in result,result;time.sleep(.08)
def command(value):guest("printf '%s\\n' "+shlex.quote(value)+" >/tmp/pointer-control")
def events():
 return [json.loads(line)for line in guest('cat /tmp/pointer-client.log').splitlines()if line.startswith('{')]
def memory():
 text=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
 return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
def position(x,y):
 for _ in range(40):
  p=ctl('status')['pointer'];dx=x-p['x'];dy=y-p['y']
  if abs(dx)<2 and abs(dy)<2:return
  move(int(dx/2)or(1 if dx>0 else -1)if abs(dx)>1 else 0,int(dy/2)or(1 if dy>0 else -1)if abs(dy)>1 else 0)
 raise AssertionError(('pointer did not converge',ctl('status')['pointer'],x,y))
passed=[]
try:
 guest('emberwm ctl quit; sleep 1; emberwm --native --config /tmp/config.toml >/tmp/ember-mouse.log 2>&1 &')
 wait(lambda:ctl('status')['version']==VERSION);display=ctl('status')['wayland_display']
 guest(f'WAYLAND_DISPLAY={display} ember-pointer-client --app-id mouse-test --control /tmp/pointer-control >/tmp/pointer-client.log 2>&1 &')
 wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready'])
 ctl('fullscreen')
 x=ctl('status')['xwayland'];xenv='DISPLAY='+shlex.quote(x['display'])+' XAUTHORITY='+shlex.quote(x['auth'])
 guest(xenv+' ember-x11-window overlay >/tmp/overlay-fixture.log 2>&1 & echo $! >/tmp/overlay-fixture.pid')
 wait(lambda:any(w['app_id']=='linusware_overlay'for w in ctl('status')['windows']))
 assert next(w for w in ctl('status')['windows']if w['app_id']=='mouse-test')['focused']
 time.sleep(.2)
 capture=ROOT/'artifacts/native-vm/overlay-above-fullscreen.ppm';qmp('screendump',{'filename':str(capture)})
 from PIL import Image
 assert Image.open(capture).convert('RGB').getpixel((60,70))==(0x34,0x56,0x78)
 passed.append('Click-through X11 drawing overlay maps above fullscreen without taking keyboard focus')
 # Disable acceleration on the virtual mouse only via large direct motion then use status coordinates.
 position(180,180);wait(lambda:any(e['event']=='pointer-enter'for e in events()))
 command('lock');wait(lambda:ctl('status')['pointer_locked']);before=ctl('status')['pointer'];count=len(events());move(150,90);after=ctl('status')['pointer'];assert before==after,(before,after)
 delta=[e for e in events()[count:]if e['event']=='relative'];assert delta and any(e['raw_dx']!=0 or e['raw_dy']!=0 for e in delta),delta
 assert not any(e['event']=='pointer-motion'for e in events()[count:]),events()[count:]
 passed.append('Native relative + unaccelerated motion reaches locked client; absolute position stays fixed with no absolute motion events')
 key('meta_l','2');assert not ctl('status')['pointer_locked'];key('meta_l','1');move(1,1)
 wait(lambda:ctl('status')['pointer_locked'])
 assert next(w for w in ctl('status')['windows']if w['app_id']=='mouse-test')['focused']
 passed.append('Persistent capture resumes after leaving and returning to the game workspace with an overlay; no Insert toggles required')
 guest(xenv+' ember-x11-window menu >/tmp/menu-fixture.log 2>&1 & echo $! >/tmp/menu-fixture.pid')
 menu=wait(lambda:next((w for w in ctl('status')['windows']if w['app_id']=='LinusWare'),None));assert menu['floating']and menu['geometry']['width']==420 and menu['geometry']['height']==260,menu
 assert next(w for w in ctl('status')['windows']if w['app_id']=='mouse-test')['fullscreen']
 assert menu['visible']and menu['focused']and not ctl('status')['pointer_locked']
 guest('kill $(cat /tmp/menu-fixture.pid)');wait(lambda:all(w['app_id']!='LinusWare'for w in ctl('status')['windows']));move(1,1);wait(lambda:ctl('status')['pointer_locked'])
 passed.append('Interactive menu keeps its requested size above the fullscreen game, suspends game capture, then restores game focus/capture when closed')
 command('hint 110 130');time.sleep(.1);command('unlock');wait(lambda:not ctl('status')['pointer_locked']);time.sleep(.15)
 geo=ctl('status')['windows'][0]['geometry'];p=ctl('status')['pointer'];assert abs(p['x']-geo['x']-110)<1 and abs(p['y']-geo['y']-130)<1,(p,geo)
 passed.append('Committed surface-local cursor hint is applied on client unlock')
 command('lock');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','esc');assert not ctl('status')['pointer_locked'];move(8,8);assert not ctl('status')['pointer_locked']
 command('unlock');key('meta_l','2');key('meta_l','1');position(160,160);command('lock');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','spc');wait(lambda:ctl('launcher')['open']);assert not ctl('status')['pointer_locked'];key('esc');command('unlock')
 passed.append('Super+Escape releases persistent lock; launcher can open and releases capture')
 position(0,0);position(160,160);command('confine');wait(lambda:any(e['event']=='confined'for e in events()));move(900,900);p=ctl('status')['pointer'];assert geo['x']+50<=p['x']<geo['x']+250 and geo['y']+50<=p['y']<geo['y']+210,(p,geo)
 command('unconfine');move(40,40);assert ctl('status')['pointer']!=p
 passed.append('Confinement keeps virtual pointer inside client region and releases cleanly')
 position(0,0);position(160,160);command('lock-once');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','2');assert not ctl('status')['pointer_locked'];key('meta_l','1');move(1,1);assert not ctl('status')['pointer_locked'];command('unlock')
 passed.append('Workspace switch releases capture; a one-shot lock never reactivates')
 command('lock');time.sleep(.2);key('meta_l','esc');command('unlock');command('lock');time.sleep(.2)
 assert not ctl('status')['pointer_locked'],'client recreated the lock after emergency release'
 passed.append('A replacement client lock cannot bypass emergency release on the same surface')
 report={'passed':passed,'scope':'Native DRM virtual keyboard/mouse fixture; no physical input or Payday gameplay'}
 (ROOT/'docs/game-pointer-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
