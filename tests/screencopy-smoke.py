#!/usr/bin/env python3
"""Real wlr-screencopy clients against a nested compositor; synthetic pixel pattern."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];WORK=ROOT/'artifacts/native-vm';ART=ROOT/'artifacts/screencopy';ART.mkdir(parents=True,exist_ok=True)
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  v=fn()
  if v:return v
  time.sleep(.03)
 raise AssertionError('capture test timed out')
with tempfile.TemporaryDirectory(prefix='ember-copy-test-')as folder:
 tmp=Path(folder);cfg=tmp/'config.toml';cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')).replace('[screencast]\nenabled = true','[screencast]\nenabled = true\nallow_direct_capture = true'))
 runtime=Path(os.environ['XDG_RUNTIME_DIR']);known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ART/'server.log').open('w');wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],stdout=log,stderr=log)
 path=runtime/f'ember-copy-test-{os.getpid()}.sock';sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(path));sock.settimeout(4);client=None
 try:
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server))
  def ctl(cmd):sock.send(cmd.encode());r=json.loads(sock.recv(262144));assert not r.get('error'),r;return r
  env=os.environ.copy();env['WAYLAND_DISPLAY']=ctl('status')['wayland_display'];fifo=tmp/'control.fifo';client=subprocess.Popen([str(WORK/'wm-client'),'--control',str(fifo)],env=env,stdout=log,stderr=log)
  wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('fullscreen');wait(lambda:ctl('status')['windows'][0]['fullscreen'])
  with fifo.open('w')as f:f.write('pattern\n')
  time.sleep(.2)
  results=[]
  for args in [[],['--cursor'],['--damage'],['--cursor','--damage']]:
   file=ART/('frame-'+str(len(results))+'.ppm');p=subprocess.run([str(WORK/'screencopy-client'),*args,'--save',str(file)],env=env,capture_output=True,text=True,timeout=15);assert p.returncode==0,(p.returncode,p.stdout,p.stderr)
   im=Image.open(file);expected=lambda x,y:(x%251,y%251,48 if y<im.height/2 else 208)
   for x,y in[(100,120),(200,600)]:assert im.getpixel((x,y))==expected(x,y),(args,(x,y),im.getpixel((x,y)),expected(x,y))
   results.append(json.loads(p.stdout))
  p=subprocess.run([str(WORK/'screencopy-client'),'--region','100','120','180','110','--save',str(ART/'region.ppm')],env=env,capture_output=True,text=True,timeout=15);assert p.returncode==0,(p.stdout,p.stderr);im=Image.open(ART/'region.ppm');assert im.size==(180,110);assert im.getpixel((0,0))==(100,120,48)
  p=subprocess.run([str(WORK/'screencopy-client'),'--bad-buffer'],env=env,capture_output=True,text=True,timeout=15);assert p.returncode==5,p
  p=subprocess.run([str(WORK/'screencopy-client'),'--cursor','--frames','30'],env=env,capture_output=True,text=True,timeout=20);assert p.returncode==0 and json.loads(p.stdout)['frames']==30,(p.stdout,p.stderr)
  assert ctl('screencast')['pending_frames']==0 and not ctl('screencast')['buffer_bytes']
  result={'passed':['SHM full-screen capture in both cursor modes','RGB channels and top/bottom orientation match synthetic pattern','copy_with_damage works','Region capture coordinates match exact pixels','Invalid buffer fails safely','Repeated streaming frames release pending resources'],'scope':'Real Wayland clients and nested Ember; synthetic fullscreen fixture; no host screenshot taken','captures':results}
  (ROOT/'docs/screencopy-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
 finally:
  if client and client.poll()is None:client.terminate();client.wait(timeout=5)
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();path.unlink(missing_ok=True);log.close()
