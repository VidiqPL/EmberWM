#!/usr/bin/env python3
"""Native launcher key/mouse/usage and screencopy/consent tests on virtual hardware."""
import json,re,shlex,time
from pathlib import Path
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
def guest(command):return shell(command).replace('\r','')
def ctl(command):
 text=guest('emberwm ctl '+shlex.quote(command));start=text.find('{\n');assert start>=0,text
 data=json.JSONDecoder().raw_decode(text[start:])[0];assert not data.get('error'),data;return data
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except (AssertionError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('native launcher/capture timed out')
def key(*keys):
 for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
  assert 'error' not in qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});time.sleep(.1)
def launcher():return ctl('launcher')
def ready():
 s=launcher();return s if s['open']and not s['loading']and s['view']else None
def type_text(text):
 for c in text:key(c)
def memory():
 text=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
 return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
passed=[]
try:
 guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/launcher-config.toml; rm -f /tmp/launch-records; mkdir -p /tmp/empty')
 guest("sed -i '/^[[]screencast[]]/a allow_direct_capture = true' /tmp/launcher-config.toml")
 guest('XDG_DATA_HOME=/etc/launcher-apps XDG_DATA_DIRS=/tmp/empty XDG_STATE_HOME=/tmp/launcher-state emberwm --native --config /tmp/launcher-config.toml >/tmp/ember-launcher.log 2>&1 &')
 wait(lambda:ctl('status')['version']=='0.10.0');display=ctl('status')['wayland_display']
 guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id launcher-input-test --log-keys --control /tmp/launcher-client.fifo >/tmp/launcher-client.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready'])
 key('meta_l','spc');s=wait(ready);assert s['count']==2,s
 type_text('fire');assert launcher()['results'][0]['name']=='Firefox';key('ret');wait(lambda:not launcher()['open']and not launcher()['launching']);wait(lambda:'firefox|'in guest('cat /tmp/launch-records'))
 assert f'firefox|{display}|Ember'in guest('cat /tmp/launch-records')
 key('meta_l','spc');wait(ready);type_text('fir');key('ret');wait(lambda:not launcher()['launching']);time.sleep(.1)
 key('meta_l','spc');wait(ready);type_text('fi');s=launcher();assert s['results'][0]['name']=='Firefox'and s['results'][0]['uses']>=2,s
 key('down');assert launcher()['results'][launcher()['cursor']]['name']=='Files';key('up');assert launcher()['cursor']==0
 assert 'error' not in qmp('input-send-event',{'events':[{'type':'btn','data':{'down':True,'button':'wheel-down'}},{'type':'btn','data':{'down':False,'button':'wheel-down'}}]});time.sleep(.15);assert launcher()['cursor']==1
 ppm=WORK/'launcher.ppm';assert 'error' not in qmp('screendump',{'filename':str(ppm)})
 from PIL import Image
 Image.open(ppm).save(ROOT/'docs/launcher-native.png')
 key('ret');wait(lambda:not launcher()['open']and not launcher()['launching']);wait(lambda:'files|'in guest('cat /tmp/launch-records'))
 keys=[int(v)for v in re.findall(r'"event":"key","code":(\d+)',guest('cat /tmp/launcher-client.log'))];assert not set(keys)&{33,23,19,18},keys
 passed.append('Physical Super+Space opens launcher; real desktop entries launch with Ember display/session and no text input leaks')
 passed.append('Usage persists across openings; fi ranks Firefox first; arrows, mouse wheel and Enter select apps')
 key('meta_l','shift','spc');assert ctl('status')['windows'][0]['floating'];key('meta_l','shift','spc');assert not ctl('status')['windows'][0]['floating']
 passed.append('Floating toggle remains available on Super+Shift+Space')
 for _ in range(3):ctl('launcher-toggle');wait(ready);ctl('launcher-toggle')
 before=memory()
 for _ in range(30):ctl('launcher-toggle');wait(ready);ctl('launcher-toggle')
 time.sleep(.3);after=memory();s=launcher();assert s['buffer_bytes']==0 and not s['font_loaded']and s['count']==0 and not s['results']and not s['query']
 assert after['Pss_Anon']-before['Pss_Anon']<4096,(before,after)
 passed.append('Thirty native launcher open/close cycles free fonts, pixels, index and query with bounded memory growth')
 ctl('fullscreen');wait(lambda:ctl('status')['windows'][0]['fullscreen']);guest("printf '%s\\n' pattern >/tmp/launcher-client.fifo");time.sleep(.15)
 captures=[]
 for flags in ['', '--cursor', '--damage', '--cursor --damage']:
  text=guest(f'WAYLAND_DISPLAY={display} ember-screencopy-client {flags}');line=next(line for line in text.splitlines()if line.startswith('{"frames":'));result=json.loads(line);assert result['frames']==1 and not result['failed'],result
  assert result['samples']==[[100,120,48],[200,(result['height']-100)%251,208]],result;captures.append(result)
 text=guest(f'WAYLAND_DISPLAY={display} ember-screencopy-client --cursor --frames 20');assert '"frames":20'in text,text
 passed.append('Real native wlr-screencopy buffers match asymmetric pattern in both cursor modes, including damage and repeated frames')
 token=ctl('share-request')['token'];wait(lambda:ctl('screencast')['prompt_open']);key('esc');assert ctl(f'share-result {token}')['output']==''
 token=ctl('share-request')['token'];wait(lambda:ctl('screencast')['prompt_open']);key('tab');key('ret');assert ctl(f'share-result {token}')['output'];assert ctl('screencast')['buffer_bytes']==0
 passed.append('Native screen-sharing consent responds to Escape/Tab/Enter and drops its UI buffer afterward')
 report={'passed':passed,'before_cycles_kib':before,'after_cycles_kib':after,'anonymous_growth_kib':after['Pss_Anon']-before['Pss_Anon'],'captures':captures,'scope':'Native DRM/KMS at 1920x1080/143.981 Hz, 512 MiB QEMU with virtual keyboard/mouse and synthetic apps/pixels; no physical screen/input touched'}
 (ROOT/'docs/launcher-screencast-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
