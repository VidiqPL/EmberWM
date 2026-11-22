#!/usr/bin/env python3
"""Independent ext capture clients: source isolation, bad buffers, cursor modes, off-workspace windows."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];WORK=ROOT/'artifacts/native-vm';ART=ROOT/'artifacts/image-capture';ART.mkdir(parents=True,exist_ok=True)
def wait(fn,timeout=10):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  value=fn()
  if value:return value
  time.sleep(.025)
 raise AssertionError('capture operation timed out')
with tempfile.TemporaryDirectory(prefix='ember-image-test-')as folder:
 tmp=Path(folder);cfg=tmp/'config.toml';cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')).replace('[screencast]\nenabled = true','[screencast]\nenabled = true\nallow_direct_capture = true'))
 runtime=Path(os.environ['XDG_RUNTIME_DIR']);known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ART/'server.log').open('w');wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],stdout=log,stderr=log)
 peer=runtime/f'ember-image-test-{os.getpid()}.sock';sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(peer));sock.settimeout(4);clients=[]
 try:
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server))
  def ctl(cmd):sock.send(cmd.encode());result=json.loads(sock.recv(262144));assert not result.get('error'),result;return result
  env=os.environ.copy();env['WAYLAND_DISPLAY']=ctl('status')['wayland_display'];fifo=tmp/'target-control'
  clients.append(subprocess.Popen([str(WORK/'wm-client'),'--app-id','capture-target','--control',str(fifo)],env=env,stdout=log,stderr=log))
  wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('float')
  with fifo.open('w')as f:f.write('pattern\n')
  clients.append(subprocess.Popen([str(WORK/'wm-client'),'--app-id','capture-cover'],env=env,stdout=log,stderr=log));wait(lambda:len(ctl('status')['windows'])==2 and ctl('status')['windows'][1]['placement_ready']);ctl('fullscreen');time.sleep(.15)
  def capture(args):
   result=subprocess.run([str(WORK/'image-capture-client'),*args],env=env,capture_output=True,text=True,timeout=15)
   return result,json.loads(result.stdout)if result.stdout else None
  reports=[]
  for flags in [[],['--cursor'],['--frames','30']]:
   file=ART/f'window-{len(reports)}.ppm';p,data=capture(['--window','capture-target','--save',str(file),*flags]);assert p.returncode==0,(p.returncode,p.stdout,p.stderr)
   image=Image.open(file);assert image.size!=(1280,800);assert data['transform']==0
   for x,y in[(100,120),(200,image.height-100)]:assert image.getpixel((x,y))==(x%251,y%251,48 if y<image.height/2 else 208),(data,x,y,image.getpixel((x,y)))
   reports.append(data)
  p,data=capture(['--window','capture-target','--bad-buffer']);assert p.returncode==5 and data['reason']==1,(p.returncode,p.stdout,p.stderr)
  ctl('workspace 2');p,data=capture(['--window','capture-target','--save',str(ART/'off-workspace.ppm')]);assert p.returncode==0,(p.stdout,p.stderr)
  image=Image.open(ART/'off-workspace.ppm');assert image.getpixel((100,120))==(100,120,48)
  assert not ctl('status')['image_capture']['pending']
  stop_log=(ART/'stop.log').open('w')
  consumer=subprocess.Popen([str(WORK/'image-capture-client'),'--window','capture-target','--frames','100000'],env=env,stdout=stop_log,stderr=stop_log)
  time.sleep(.2)
  with fifo.open('w')as f:f.write('quit\n')
  consumer.wait(timeout=10);stop_log.close();stopped=json.loads((ART/'stop.log').read_text());assert stopped['stopped'] and stopped['frames']>0,stopped

  report={'passed':['ext foreign-toplevel/source protocols discover the selected app','Isolated window pixels exclude fullscreen occluder, panels and background','Window cursor modes and thirty repeated frames succeed','Invalid buffer fails with buffer_constraints without crashing','Off-workspace window stays capturable with correct orientation','Closing shared window stops existing capture session','Capture buffers and pending frames are released'],'captures':reports,'scope':'Nested isolated Ember, synthetic Wayland apps only; no host pixels captured'}
  (ROOT/'docs/image-capture-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  for p in clients:
   if p.poll()is None:p.terminate();p.wait(timeout=4)
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();peer.unlink(missing_ok=True);log.close()
