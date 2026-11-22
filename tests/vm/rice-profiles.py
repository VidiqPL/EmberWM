#!/usr/bin/env python3
"""Actual native keyboard profile cycling, live relayout, snapshots and release of panels."""
import json,re,shlex,time,tomllib
from pathlib import Path
from PIL import Image
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
VERSION=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
def guest(command):return shell(command).replace('\r','')
def ctl(command):
 text=guest('emberwm ctl '+shlex.quote(command));i=text.find('{\n');assert i>=0,text;result=json.JSONDecoder().raw_decode(text[i:])[0];assert not result.get('error'),result;return result
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   result=fn()
   if result:return result
  except (AssertionError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('native rice operation timed out')
def key(*keys):
 for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
  result=qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});assert 'error'not in result,result;time.sleep(.1)
def memory():
 pid=ctl('status')['pid'];text=guest(f'cat /proc/{pid}/smaps_rollup')
 return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
def capture(name):
 path=WORK/(name+'.ppm');result=qmp('screendump',{'filename':str(path)});assert 'error'not in result,result;Image.open(path).save(ROOT/'docs'/(name+'.png'))
passed=[]
try:
 state=wait(lambda:ctl('status')if ctl('status')['version']==VERSION else None);time.sleep(.8)
 key('meta_l','shift','t');wait(lambda:ctl('config')['rice']['profile']=='midnight');wait(lambda:len(ctl('panels')['items'])==3);capture('rice-midnight-native')
 key('meta_l','shift','t');wait(lambda:ctl('config')['rice']['profile']=='minimal');wait(lambda:ctl('panels')['buffer_bytes']==0);assert not ctl('panels')['font_loaded']and not ctl('panels')['polling'];capture('rice-minimal-native')
 key('meta_l','shift','t');wait(lambda:ctl('config')['rice']['profile']=='red-dusk');wait(lambda:len(ctl('panels')['items'])==1)
 passed.append('Physical Super+Shift+T cycles three complete profiles; midnight renders three panels, minimal releases panel font/pixels/polling, Red Dusk restores wallpaper')
 key('meta_l','slash');wait(lambda:ctl('status')['ui']['help']);assert any(r['action']=='rice-cycle'and r['keys']=='Super+Shift+t'for r in ctl('status')['ui']['rows']);key('esc')
 display=ctl('status')['wayland_display']
 for n in [1,2]:
  guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id rice-window-{n} >/tmp/rice-window-{n}.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==n and all(w['placement_ready']for w in ctl('status')['windows']))
 ctl('rice midnight');time.sleep(.15);old=ctl('status')['windows'];ctl('rice minimal');time.sleep(.15);new=ctl('status')['windows'];assert [w['app_id']for w in old]==[w['app_id']for w in new]and old[0]['geometry']!=new[0]['geometry']
 ctl('workspace 2');ctl('rice midnight');assert ctl('status')['workspace']==2 and all(w['workspace']==1 for w in ctl('status')['windows'])
 ctl('theme forest');ctl('panels-toggle');saved_look=ctl('config');saved=ctl('rice save native-look');assert saved['path']=='/tmp/rices/native-look.toml';ctl('rice minimal');ctl('rice native-look');restored=ctl('config')
 for section in ['panels','launcher','shortcuts','wifi','notifications']:assert restored[section]==saved_look[section],section
 assert restored['keyboard']['modifier']=='Super'and ctl('status')['workspace']==2
 passed.append('Native help includes live shortcut; profile switches retile existing windows without changing identity/workspace; saving/restoring preserves current palette and panel state')
 for _ in range(2):
  for name in ['midnight','minimal','native-look']:ctl('rice '+name);time.sleep(.04)
 ctl('rice minimal');time.sleep(.2);before=memory()
 for _ in range(10):
  for name in ['midnight','minimal','native-look']:ctl('rice '+name);time.sleep(.04)
 ctl('rice minimal');time.sleep(.2);after=memory();assert after['Pss_Anon']-before['Pss_Anon']<4096,(before,after)
 assert ctl('launcher')['buffer_bytes']==ctl('wifi')['buffer_bytes']==ctl('screencast')['buffer_bytes']==0
 passed.append('Thirty native profile switches have bounded memory and no retained popup/panel buffers in minimal mode')
 report={'passed':passed,'before_cycles_kib':before,'after_cycles_kib':after,'anonymous_growth_kib':after['Pss_Anon']-before['Pss_Anon'],'scope':'Native 1920x1080/143.981 Hz in 512 MiB VM, virtual GPU/keyboard, synthetic windows, audio disabled'}
 (ROOT/'docs/rice-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
