#!/usr/bin/env python3
"""Private Xwayland plus the actual X11 arboard backend, never the host clipboard."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];ART=ROOT/'artifacts/xwayland';ART.mkdir(exist_ok=True)
PROBE=ROOT/'artifacts/x11-probe-target/debug/ember-x11-clipboard-probe';BIN=ROOT/'target/release/emberwm'
def wait(fn,seconds=15):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  result=fn()
  if result:return result
  time.sleep(.05)
 raise AssertionError('Xwayland operation timed out')
with tempfile.TemporaryDirectory(prefix='ember-x11-test-')as folder:
 tmp=Path(folder);runtime=tmp/'run';runtime.mkdir(mode=0o700);cfg=tmp/'config.toml'
 cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')))
 env=os.environ.copy();outer=Path(env['XDG_RUNTIME_DIR']);display=env.get('WAYLAND_DISPLAY','wayland-0')
 env['RUST_LOG']='emberwm=info';env['XDG_RUNTIME_DIR']=str(runtime);env['WAYLAND_DISPLAY']=display if display.startswith('/')else str(outer/display)
 log=(ART/'server.log').open('w');wm=subprocess.Popen([str(BIN),'--nested','--config',str(cfg)],env=env,stdout=log,stderr=log)
 sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(runtime/'test.sock'));sock.settimeout(6);clients=[];passed=[]
 try:
  server=wait(lambda:next(runtime.glob('emberwm-wayland-*.sock'),None));sock.connect(str(server))
  def ctl(command):
   sock.send(command.encode());d=json.loads(sock.recv(262144));assert not d.get('error'),d;return d
  def state():return ctl('status')
  initial=wait(lambda:state()['xwayland']if state()['xwayland']['pid']else None)
  assert initial['display']and initial['auth']and not initial['error'],initial
  before_exe=Path(f"/proc/{initial['pid']}/exe").resolve();assert 'xwayland-helper'in str(before_exe),before_exe
  assert not initial['ready'],initial
  def memory(pid):
   return {line.split(':')[0]:int(line.split()[1])for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()if line.startswith(('Pss:','Pss_Anon:','Rss:'))}
  starter_memory=memory(initial['pid'])
  assert Path(initial['auth']).stat().st_mode & 0o777 == 0o600
  xenv=env.copy();xenv['DISPLAY']=initial['display'];xenv['XAUTHORITY']=initial['auth']
  xenv['WAYLAND_DISPLAY']=state()['wayland_display']
  def probe(*args):
   r=subprocess.run([str(PROBE),*map(str,args)],env=xenv,capture_output=True,text=True,timeout=15)
   assert r.returncode==0,(args,r.returncode,r.stdout,r.stderr,(ART/'server.log').read_text()[-3000:]);return r.stdout
  def capture(name):
   path=ART/name;path.unlink(missing_ok=True);ctl('screenshot '+str(path))
   def ready():
    try:im=Image.open(path);im.load();return im.convert('RGBA')
    except (FileNotFoundError,OSError):return None
   return wait(ready)
  original=capture('before.png');ctl('screenshot-region 100 100 80 60');time.sleep(.15)
  raw=tmp/'pasted.rgba';result=json.loads(probe('image',raw));assert (result['width'],result['height'])==(80,60),result
  assert raw.read_bytes()==original.crop((100,100,180,160)).tobytes()
  ready=wait(lambda:state()['xwayland']if state()['xwayland']['ready']else None)
  assert Path(f"/proc/{ready['pid']}/exe").resolve()==Path('/usr/bin/Xwayland').resolve()
  invalid=xenv.copy();invalid['XAUTHORITY']=str(tmp/'no-authority')
  denied=subprocess.run([str(PROBE),'text',str(tmp/'unauthorized.txt')],env=invalid,capture_output=True,text=True,timeout=6)
  assert denied.returncode!=0,'X11 unexpectedly accepted a client without session authorization'
  passed.append('Private Xauthority is mode 0600 and unauthenticated X11 connections are rejected')
  passed.append('X server stays unstarted until the first X11 clipboard connection; arboard pastes the exact native screenshot pixels')
  client=subprocess.Popen([str(ROOT/'artifacts/x11-window')],env=xenv,stdout=log,stderr=log);clients.append(client)
  window=wait(lambda:next((w for w in state()['windows']if w['x11']and w['placement_ready']),None))
  assert window['visible']and window['focused'],window
  image=capture('x11-window.png');g=window['geometry'];assert image.getpixel((g['x']+g['width']//2,g['y']+g['height']//2))[:3]==(0x34,0x56,0x78)
  ctl('float');ctl('fullscreen');assert next(w for w in state()['windows']if w['x11'])['fullscreen']
  ctl('fullscreen');ctl('scratchpad-mark');assert next(w for w in state()['windows']if w['x11'])['scratchpad']
  ctl('scratchpad-toggle');assert not next(w for w in state()['windows']if w['x11'])['visible']
  ctl('scratchpad-toggle');ctl('scratchpad-mark');ctl('close');client.wait(timeout=5)
  wait(lambda:not state()['windows'])
  passed.append('Real X11 window maps, focuses, floats, fullscreens, uses the scratchpad, hides/shows and closes without crashing native window management')
  fifo=tmp/'native.fifo';native=subprocess.Popen([str(ROOT/'artifacts/native-vm/wm-client'),'--app-id','native-clipboard','--control',str(fifo)],env=xenv,stdout=log,stderr=log);clients.append(native)
  wait(lambda:any(w['app_id']=='native-clipboard'and w['placement_ready']for w in state()['windows']))
  def command(text):
   with fifo.open('w')as f:f.write(text+'\n')
  command('text native to X11 clipboard');time.sleep(.12);text_path=tmp/'text.txt';probe('text',text_path);assert text_path.read_text()=='native to X11 clipboard'
  # High-entropy PNG forces X11 INCR transfers rather than fitting one property.
  width,height=512,256;pixels=bytearray(os.urandom(width*height*4));pixels[3::4]=b'\xff'*(width*height)
  source_image=Image.frombytes('RGBA',(width,height),bytes(pixels));png=tmp/'large.png';source_image.save(png);assert png.stat().st_size>65536
  command('image '+str(png));time.sleep(.12);received_raw=ART/'raw-native.png';subprocess.run([str(ROOT/'artifacts/x11-selection'),'image/png',str(received_raw)],env=xenv,check=True,timeout=10);assert received_raw.read_bytes()==png.read_bytes(),(len(received_raw.read_bytes()),png.stat().st_size,received_raw.read_bytes()[:24]);meta=json.loads(probe('image',raw));assert (meta['width'],meta['height'])==(width,height)and raw.read_bytes()==bytes(pixels)
  passed.append('Native Wayland text and large PNG copy to the X11 arboard backend with byte-exact image pixels, including INCR-sized data')
  raw.write_bytes(pixels)
  owner=subprocess.Popen([str(PROBE),'set-image',str(width),str(height),str(raw)],env=xenv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True);clients.append(owner)
  assert owner.stdout.readline().strip()=='ready';time.sleep(.15)
  received=tmp/'from-x11.png';command('clipboard '+str(received))
  def image_ready():
   try:im=Image.open(received).convert('RGBA');im.load();return im
   except (FileNotFoundError,OSError):return None
  assert wait(image_ready).tobytes()==bytes(pixels)
  received.unlink();command('clipboard '+str(received));assert wait(image_ready).tobytes()==bytes(pixels)
  owner.stdin.write('done\n');owner.stdin.flush();owner.wait(timeout=5)
  owner=subprocess.Popen([str(PROBE),'set-text','X11 to native clipboard'],env=xenv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True);clients.append(owner)
  assert owner.stdout.readline().strip()=='ready';time.sleep(.12);text_path.unlink();command('clipboard-text '+str(text_path));wait(lambda:text_path.exists()and text_path.stat().st_size);assert text_path.read_text()=='X11 to native clipboard'
  owner.stdin.write('done\n');owner.stdin.flush();owner.wait(timeout=5)
  passed.append('X11 arboard image/text owners paste into a focused native Wayland client, including large PNG data')
  command('text restart bridge');time.sleep(.12)
  old_pid=state()['xwayland']['pid'];os.kill(old_pid,9)
  recovered=wait(lambda:state()['xwayland']if state()['xwayland']['pid']not in [None,old_pid]else None)
  assert recovered['display']==initial['display']and recovered['auth']==initial['auth'],recovered
  probe('text',text_path);assert text_path.read_text()=='restart bridge'
  passed.append('An X server crash restarts the bridge on the same DISPLAY/auth path while preserving a native clipboard source')
  report={'passed':passed,'initial_server':initial,'ready_server':ready,'lazy_starter_memory_kib':starter_memory,'scope':'Private nested Wayland/X11 displays, private Xauthority, arboard 3.7 X11 backend; no host clipboard touched'}
  (ROOT/'docs/xwayland-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  for child in clients:
   if child.poll()is None:child.terminate();child.wait(timeout=5)
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();log.close()
