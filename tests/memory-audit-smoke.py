#!/usr/bin/env python3
"""Panel font eviction preserves pixels; exact nested region clipboard captures preserve orientation."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from PIL import Image,ImageChops
ROOT=Path(__file__).resolve().parents[1];ART=ROOT/'artifacts/memory-audit';ART.mkdir(parents=True,exist_ok=True)
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  value=fn()
  if value:return value
  time.sleep(.025)
 raise AssertionError('memory-audit test timed out')
with tempfile.TemporaryDirectory(prefix='ember-memory-audit-')as folder:
 tmp=Path(folder);cfg=tmp/'config.toml';cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')).replace('"workspaces", "clock"','"workspaces", "text:UNCHANGED"'))
 test_env=os.environ.copy();host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0');runtime=tmp/'run';runtime.mkdir(mode=0o700);test_env['XDG_RUNTIME_DIR']=str(runtime);test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display);known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ART/'server.log').open('w');wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],env=test_env,stdout=log,stderr=log)
 peer=runtime/f'ember-memory-audit-{os.getpid()}.sock';sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(peer));sock.settimeout(4);client=None
 try:
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server))
  def ctl(cmd):sock.send(cmd.encode());value=json.loads(sock.recv(262144));assert not value.get('error'),value;return value
  def screenshot(name):
   path=ART/name;path.unlink(missing_ok=True);ctl('screenshot '+str(path))
   def ready():
    try:
     image=Image.open(path);image.load();return image.copy()
    except (FileNotFoundError,OSError):return None
   return wait(ready)
  first=ctl('panels');assert first['font_loaded'];before=screenshot('before-eviction.png');count=ctl('panels')['rasterizations']
  wait(lambda:not ctl('panels')['font_loaded']);assert ctl('panels')['items']and ctl('panels')['rasterizations']==count
  frames=ctl('status')['frames_submitted'];time.sleep(.2);assert ctl('status')['frames_submitted']==frames
  after=screenshot('after-eviction.png');assert ImageChops.difference(before.convert('RGB'),after.convert('RGB')).getbbox()is None
  for _ in range(20):ctl('status')
  assert not ctl('panels')['font_loaded']and ctl('panels')['rasterizations']==count
  ctl('workspace 2');wait(lambda:ctl('panels')['rasterizations']>count);assert ctl('panels')['font_loaded']
  ctl('shortcuts-toggle');wait(lambda:ctl('status')['ui']['font_loaded']);wait(lambda:not ctl('panels')['font_loaded']);assert ctl('status')['ui']['font_loaded'];ctl('shortcuts-toggle');wait(lambda:not ctl('status')['ui']['font_loaded'])
  env=test_env.copy();env['WAYLAND_DISPLAY']=ctl('status')['wayland_display'];fifo=tmp/'control'
  client=subprocess.Popen([str(ROOT/'artifacts/native-vm/wm-client'),'--control',str(fifo)],env=env,stdout=log,stderr=log)
  wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('fullscreen');wait(lambda:ctl('panels')['buffer_bytes']==0);assert not ctl('panels')['font_loaded']
  with fifo.open('w')as f:f.write('pattern\n')
  time.sleep(.1);size=ctl('status')['output'];results=[]
  for i,(x,y,w,h)in enumerate([(100,120,320,120),(200,600,320,120),(0,0,size['width'],size['height'])]):
   ctl(f'screenshot-region {x} {y} {w} {h}');time.sleep(.1);path=tmp/f'paste-{i}.png'
   with fifo.open('w')as f:f.write('clipboard '+str(path)+'\n')
   def image_ready():
    try:
     image=Image.open(path);image.load();return image.convert('RGB')
    except (FileNotFoundError,OSError):return None
   im=wait(image_ready);assert im.size==(w,h)
   for xx,yy in[(0,0),(w//2,h//2),(w-1,h-1)]:assert im.getpixel((xx,yy))==((x+xx)%251,(y+yy)%251,48 if y+yy<size['height']/2 else 208),(i,xx,yy,im.getpixel((xx,yy)))
   status=ctl('status')['clipboard'];assert status['allocated_bytes']==status['bytes'];results.append({'region':[x,y,w,h],'clipboard_bytes':status['bytes']})
  report={'passed':['After two idle seconds, panel font is released with identical rendered pixels and no idle redraw','Unchanged panel bitmaps never reload font; changing workspace reloads and redraws','Open help keeps its own shared font; closed help releases it','Fullscreen immediately drops hidden panel font/buffers','Nested top/bottom/full-output PNG clipboard captures preserve exact orientation and channels','Clipboard allocation is trimmed to encoded PNG length'],'captures':results,'scope':'Isolated nested compositor and synthetic pattern client; host desktop/clipboard untouched'}
  (ROOT/'docs/memory-audit-functional.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  if client and client.poll()is None:client.terminate();client.wait(timeout=5)
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();peer.unlink(missing_ok=True);log.close()
