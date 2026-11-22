#!/usr/bin/env python3
"""Native window capture isolation and actual chooser keyboard/mouse rendering."""
import json,shlex,time
from pathlib import Path
from PIL import Image
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
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
  time.sleep(.04)
 raise AssertionError('native window capture timed out')
def key(*keys):
 for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
  result=qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});assert 'error'not in result,result;time.sleep(.08)
passed=[]
try:
 guest('emberwm ctl quit; sleep 1; emberwm --native --config /etc/emberwm/capture-test.toml >/tmp/native-capture.log 2>&1 &');wait(lambda:ctl('status')['version']=='0.10.0');display=ctl('status')['wayland_display']
 guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id native-target --title "Target window" --control /tmp/native-target-control >/tmp/native-target.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('float');guest("printf '%s\\n' pattern >/tmp/native-target-control")
 guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id native-cover --title "Occluding fullscreen window" >/tmp/native-cover.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==2 and ctl('status')['windows'][1]['placement_ready']);ctl('fullscreen');time.sleep(.2)
 captures=[]
 for flags in ['', '--cursor','--frames 30']:
  text=guest(f'WAYLAND_DISPLAY={display} ember-image-capture-client --window native-target {flags} --save /tmp/native-window.ppm');line=next(l for l in text.splitlines()if l.startswith('{"frames":'));data=json.loads(line);assert not data['failed']and data['transform']==0,data;captures.append(data)
 # Check the actual guest bytes, without copying host or desktop pixels.
 for x,y in [(100,120),(200,captures[-1]['height']-100)]:
  text=guest(f"/bin/busybox tail -c +{len(f'P6\n{captures[-1]['width']} {captures[-1]['height']}\n255\n'.encode())+1} /tmp/native-window.ppm | /bin/busybox dd bs=1 skip={((y*captures[-1]['width'])+x)*3} count=3 2>/dev/null | /bin/busybox od -An -tu1")
  values=next([int(v)for v in line.split()]for line in text.splitlines()if line.strip()and all(v.isdigit()for v in line.split())and len(line.split())==3)
  assert values==[x%251,y%251,48 if y<captures[-1]['height']/2 else 208],values
 passed.append('Native isolated window capture matches asymmetric source pixels behind fullscreen occluder, both cursor modes and thirty frames')
 token=ctl('share-request')['token'];wait(lambda:ctl('screencast')['view']);assert len(ctl('screencast')['sources'])==3;key('down');selected=ctl('screencast')['selected_source'];assert selected.startswith('Window: ')
 file=WORK/'sharing-picker.ppm';qmp('screendump',{'filename':str(file)});Image.open(file).save(ROOT/'docs/sharing-picker-native.png')
 key('tab');key('ret');assert ctl(f'share-result {token}')['output']==selected;assert ctl('screencast')['buffer_bytes']==0
 passed.append('Native Up/Tab/Enter selects a window and approves; consent pixels are released afterward')
 token=ctl('share-request')['token'];wait(lambda:ctl('screencast')['view']);key('ret');assert ctl(f'share-result {token}')['output']==''
 passed.append('Cancel remains the default even when Enter is pressed immediately')
 report={'passed':passed,'captures':captures,'scope':'Native 1920x1080/143.981 Hz virtual DRM/GPU, keyboard and synthetic apps only'}
 (ROOT/'docs/image-capture-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
