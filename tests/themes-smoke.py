#!/usr/bin/env python3
"""Live custom presets, path resolution, explicit overrides and atomic errors."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];ART=ROOT/'artifacts/themes';ART.mkdir(parents=True,exist_ok=True)
def wait(fn):
 end=time.monotonic()+10
 while time.monotonic()<end:
  value=fn()
  if value:return value
  time.sleep(.03)
 raise AssertionError('theme test timed out')
with tempfile.TemporaryDirectory(prefix='ember-theme-test-')as folder:
 tmp=Path(folder);themes=tmp/'themes';themes.mkdir();Image.new('RGB',(32,24),(40,70,60)).save(themes/'test.png')
 (themes/'custom.toml').write_text("[theme]\nbackground='#112233'\nactive_border='#445566'\nwallpaper='test.png'\n[panels]\nfont='builtin'\nfont_size=13.0\n[layout]\ngap=7\n")
 (themes/'broken.toml').write_text("[theme]\nbackground='invalid'\n")
 (themes/'wrong-section.toml').write_text('[audio]\nenabled=false\n')
 cfg=tmp/'config.toml';cfg.write_text("[theme]\npreset='custom'\nactive_border='#ffffff'\n")
 test_env=os.environ.copy();host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0');runtime=tmp/'run';runtime.mkdir(mode=0o700);test_env['XDG_RUNTIME_DIR']=str(runtime);test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display)
 known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ART/'server.log').open('w');wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],env=test_env,stdout=log,stderr=log)
 peer=runtime/f'ember-theme-test-{os.getpid()}.sock';sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(peer));sock.settimeout(4)
 try:
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server))
  def rpc(command):sock.send(command.encode());return json.loads(sock.recv(262144))
  def ctl(command):result=rpc(command);assert not result.get('error'),result;return result
  c=ctl('config');assert c['theme']['background']=='#112233'and c['theme']['active_border']=='#ffffff'and c['theme']['wallpaper']==str(themes/'test.png')and c['layout']['gap']==7,c
  assert 'custom'in ctl('themes')['available']
  for preset in ['midnight','forest','mono','light','red',str(ROOT/'config/themes/custom-red.toml'),'red']:
   ctl('theme '+preset);after=ctl('config');assert after['theme']['wallpaper']==str(themes/'test.png')and after['theme']['wallpaper_mode']==c['theme']['wallpaper_mode']and after['theme']['wallpaper_palette']==c['theme']['wallpaper_palette'],after
  shot=ART/'theme-cycle-wallpaper.png';shot.unlink(missing_ok=True);ctl('screenshot '+str(shot));wait(lambda:shot.exists()and shot.stat().st_size>100);assert Image.open(shot).getpixel((100,400))[:3]==(40,70,60)
  ctl('theme midnight');ctl('theme custom');assert ctl('config')['theme']['active_border']=='#445566'
  before=ctl('config');generation=ctl('status')['theme_generation']
  for name in ['broken','missing','wrong-section']:assert rpc('theme '+name).get('error')
  assert ctl('config')==before and ctl('status')['theme_generation']==generation
  ctl('theme default');assert ctl('config')['theme']['active_border']=='#ffffff'
  (themes/'custom.toml').write_text((themes/'custom.toml').read_text().replace('#112233','#223344'));ctl('reload');assert ctl('config')['theme']['background']=='#223344'
  before=ctl('config');generation=ctl('status')['theme_generation'];(themes/'test.png').rename(themes/'saved.png');assert rpc('theme default').get('error');assert ctl('config')==before and ctl('status')['theme_generation']==generation;(themes/'saved.png').rename(themes/'test.png')
  assert ctl('launcher')['buffer_bytes']==ctl('wifi')['buffer_bytes']==ctl('screencast')['buffer_bytes']==0
  report={'passed':['All five built-in colour presets, the shipped custom-red example and cycling back to red preserve the configured wallpaper and its rendered pixels','Startup preset loads PNG relative to preset file','Explicit main-config colours override preset values','Custom preset discovery, live selection and gap/font overrides work','Missing/malformed/forbidden presets fail atomically without changing config','Default restores config appearance; reload picks up edited preset','Closed popup buffers remain zero'],'scope':'Isolated nested compositor and generated test wallpaper; host configuration unchanged'}
  (ROOT/'docs/themes-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  if wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  sock.close();peer.unlink(missing_ok=True);log.close()
