#!/usr/bin/env python3
"""Native relative input/constraints, escape paths and live themes using virtual hardware only."""
import json,re,shlex,time
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
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
 p=ctl('status')['pointer'];move(round(x-p['x']),round(y-p['y']))
passed=[]
try:
 guest('emberwm ctl quit; sleep 1; emberwm --native --config /tmp/config.toml >/tmp/ember-mouse.log 2>&1 &')
 wait(lambda:ctl('status')['version']=='0.10.0');display=ctl('status')['wayland_display']
 guest(f'WAYLAND_DISPLAY={display} ember-pointer-client --app-id mouse-test --control /tmp/pointer-control >/tmp/pointer-client.log 2>&1 &')
 wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready'])
 # Disable acceleration on the virtual mouse only via large direct motion then use status coordinates.
 position(180,180);wait(lambda:any(e['event']=='pointer-enter'for e in events()))
 command('lock');wait(lambda:ctl('status')['pointer_locked']);before=ctl('status')['pointer'];count=len(events());move(150,90);after=ctl('status')['pointer'];assert before==after,(before,after)
 delta=[e for e in events()[count:]if e['event']=='relative'];assert delta and any(e['raw_dx']!=0 or e['raw_dy']!=0 for e in delta),delta
 assert not any(e['event']=='pointer-motion'for e in events()[count:]),events()[count:]
 passed.append('Native relative + unaccelerated motion reaches locked client; absolute position stays fixed with no absolute motion events')
 command('hint 110 130');time.sleep(.1);command('unlock');wait(lambda:not ctl('status')['pointer_locked']);time.sleep(.15)
 geo=ctl('status')['windows'][0]['geometry'];p=ctl('status')['pointer'];assert abs(p['x']-geo['x']-110)<1 and abs(p['y']-geo['y']-130)<1,(p,geo)
 passed.append('Committed surface-local cursor hint is applied on client unlock')
 command('lock');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','esc');assert not ctl('status')['pointer_locked'];move(8,8);assert not ctl('status')['pointer_locked']
 command('unlock');position(0,0);position(160,160);command('lock');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','spc');wait(lambda:ctl('launcher')['open']);assert not ctl('status')['pointer_locked'];key('esc');command('unlock')
 passed.append('Super+Escape releases persistent lock; launcher can open and releases capture')
 position(0,0);position(160,160);command('confine');wait(lambda:any(e['event']=='confined'for e in events()));move(900,900);p=ctl('status')['pointer'];assert geo['x']+50<=p['x']<geo['x']+250 and geo['y']+50<=p['y']<geo['y']+210,(p,geo)
 command('unconfine');move(40,40);assert ctl('status')['pointer']!=p
 passed.append('Confinement keeps virtual pointer inside client region and releases cleanly')
 position(0,0);position(160,160);command('lock-once');wait(lambda:ctl('status')['pointer_locked']);key('meta_l','2');assert not ctl('status')['pointer_locked'];key('meta_l','1');move(1,1);assert not ctl('status')['pointer_locked'];command('unlock')
 passed.append('Workspace switch releases capture; a one-shot lock never reactivates')
 themes=ctl('themes')['available'];assert themes[:5]==['red','midnight','forest','mono','light']
 for theme in themes[:5]:
  ctl('theme '+theme);c=ctl('config');assert c['theme']['preset']==theme;assert not c['theme']['wallpaper'];assert c['keyboard']['modifier']=='Super'
 key('meta_l','t');assert ctl('themes')['current']=='red'
 before=memory()
 for _ in range(10):
  for theme in themes[:5]:ctl('theme '+theme)
 time.sleep(.3);after=memory();assert after['Pss_Anon']-before['Pss_Anon']<4096,(before,after)
 ctl('theme default');assert ctl('config')['theme']['wallpaper']
 passed.append('All five presets apply live; Super+T cycles; fifty switches have bounded memory and default restores original wallpaper')
 report={'passed':passed,'before_theme_cycles_kib':before,'after_theme_cycles_kib':after,'anonymous_growth_kib':after['Pss_Anon']-before['Pss_Anon'],'scope':'Native DRM/KMS in 512 MiB VM with virtual relative mouse and keyboard; no host input or settings changed'}
 (ROOT/'docs/mouse-themes-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
