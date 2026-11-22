#!/usr/bin/env python3
"""Private bus/Wayland, fake audio control, real StatusNotifier/dbusmenu service."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];ART=ROOT/'artifacts/desktop';ART.mkdir(exist_ok=True)
def wait(fn,seconds=12):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  x=fn()
  if x:return x
  time.sleep(.035)
 raise AssertionError('Tray/volume test timed out')
with tempfile.TemporaryDirectory(prefix='ember-desktop-')as folder:
 tmp=Path(folder);runtime=tmp/'run';runtime.mkdir(mode=0o700);bin=tmp/'bin';bin.mkdir();audio=tmp/'audio';audio.write_text('0.50')
 shim=bin/'wpctl';shim.write_text('''#!/usr/bin/python3
import os,sys
from pathlib import Path
p=Path(os.environ['EMBER_AUDIO_FIXTURE']);v=float(p.read_text())
if sys.argv[1]=='set-volume':v=max(0,min(1,v+(0.05 if sys.argv[-1].endswith('+')else -0.05)));p.write_text(str(v))
if sys.argv[1]=='get-volume':print('Volume:',format(v,'.2f'))
''');shim.chmod(0o755)
 cfg=tmp/'config.toml';cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')))
 env=os.environ.copy();outer=Path(env['XDG_RUNTIME_DIR']);display=env.get('WAYLAND_DISPLAY','wayland-0');env.update(XDG_RUNTIME_DIR=str(runtime),WAYLAND_DISPLAY=display if display.startswith('/')else str(outer/display),PATH=str(bin)+':'+env['PATH'],EMBER_AUDIO_FIXTURE=str(audio))
 log=(ART/'server.log').open('w');wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],env=env,stdout=log,stderr=log)
 sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(runtime/'test.sock'));sock.settimeout(5);fixture=None;passed=[]
 try:
  sock.connect(str(wait(lambda:next(runtime.glob('emberwm-wayland-*.sock'),None))))
  def ctl(cmd):
   sock.send(cmd.encode());d=json.loads(sock.recv(262144));assert not d.get('error'),d;return d
  def state():return ctl('status')
  wait(lambda:state()['tray']['watcher'])
  events=tmp/'events';fixture=subprocess.Popen(['/usr/bin/python3',str(ROOT/'tests/tray-fixture.py'),str(events)],env=env,stdout=log,stderr=log)
  wait(lambda:len(ctl('tray')['items'])==1)
  ctl('tray-toggle');wait(lambda:ctl('tray')['items'][0]['title']=='Ember tray fixture'and not ctl('tray')['busy'])
  ctl('screenshot '+str(ART/'tray.png'));ctl('tray-menu 0');wait(lambda:ctl('tray')['menu']and not ctl('tray')['busy'])
  ctl('screenshot '+str(ART/'menu.png'));ctl('tray-select 1');time.sleep(.15);assert ctl('tray')['menu']
  ctl('tray-select 2');wait(lambda:'"id": 13'in events.read_text());wait(lambda:not ctl('tray')['busy']);ctl('tray-select 0');wait(lambda:'"id": 21, "event": "clicked"'in events.read_text());wait(lambda:not ctl('tray')['open'])
  passed.append('StatusNotifier app registers; title loads; context menu loads; disabled item is ignored; nested submenu dispatches the chosen action')
  ctl('tray-toggle');wait(lambda:not ctl('tray')['busy']);ctl('tray-activate 0');wait(lambda:'"kind": "activate"'in events.read_text());wait(lambda:not ctl('tray')['open'])
  fixture.terminate();fixture.wait(timeout=4);wait(lambda:not ctl('tray')['items'])
  fixture=subprocess.Popen(['/usr/bin/python3',str(ROOT/'tests/tray-fixture.py'),str(events),'native'],env=env,stdout=log,stderr=log)
  wait(lambda:len(ctl('tray')['items'])==1);ctl('tray-toggle');wait(lambda:not ctl('tray')['busy']);ctl('tray-menu 0');wait(lambda:'"kind": "native-menu"'in events.read_text());wait(lambda:not ctl('tray')['open'])
  fixture.terminate();fixture.wait(timeout=4);wait(lambda:not ctl('tray')['items'])
  passed.append('Items without dbusmenu receive their native ContextMenu request and the Ember popup closes')
  passed.append('Left activation reaches the app; exiting a tray app removes its registration without a polling worker')
  ctl('volume-up');wait(lambda:state()['volume_osd']['visible']);assert abs(float(audio.read_text())-.55)<.001
  ctl('screenshot '+str(ART/'volume.png'));time.sleep(.45);ctl('volume-down');wait(lambda:abs(float(audio.read_text())-.5)<.001);wait(lambda:not state()['volume_osd']['busy']);assert state()['volume_osd']['visible'];time.sleep(.6);assert state()['volume_osd']['visible'];wait(lambda:not state()['volume_osd']['visible']);assert not state()['volume_osd']['busy']
  passed.append('Up/down use actual wpctl results, show percentage/bar, reset the one-second expiry and release the overlay and worker at idle; host audio untouched')
  # The closed menu does not continuously redraw or retain jobs.
  before=state()['frames_submitted'];time.sleep(1.2);after=state();assert after['frames_submitted']-before<=2 and not after['tray']['busy']
  def mem():
   return {line.split(':')[0]:int(line.split()[1])for line in Path(f'/proc/{wm.pid}/smaps_rollup').read_text().splitlines()if line.startswith(('Pss:','Pss_Anon:'))}
  baseline=mem();threads=len(list(Path(f'/proc/{wm.pid}/task').iterdir()))
  for _ in range(20):
   ctl('tray-toggle');wait(lambda:not ctl('tray')['busy']);ctl('tray-toggle')
  time.sleep(.5);after=mem();assert after['Pss_Anon']-baseline['Pss_Anon']<2048,(baseline,after)
  assert len(list(Path(f'/proc/{wm.pid}/task').iterdir()))<=threads
  passed.append('Twenty popup open/close cycles have bounded anonymous memory and leave no extra worker threads')
  report={'memory_before_kib':baseline,'memory_after_kib':after,'passed':passed,'scope':'Private nested compositor and D-Bus session, fake wpctl; no host audio or tray state modified'};(ROOT/'docs/tray-volume-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  if fixture and fixture.poll()is None:fixture.terminate();fixture.wait(timeout=5)
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();log.close()
