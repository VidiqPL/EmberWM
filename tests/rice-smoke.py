#!/usr/bin/env python3
"""Live complete profiles: isolation, save/reload/startup, resource validation and memory."""
import json, os, re, shutil, socket, stat, subprocess, tempfile, time, tomllib
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1]
ART=ROOT/'artifacts/rice';ART.mkdir(parents=True,exist_ok=True)
STYLE={'theme':['background','active_border','inactive_border','selection','selection_opacity','border_width','wallpaper','wallpaper_mode','wallpaper_palette'],
       'panels':None,'layout':['gap','master_ratio'],'shortcuts':['width','font_size'],
       'notifications':['width','margin','gap','compact','respect_panel_space'],'wifi':['width','font_size'],'launcher':['width','font_size','rows']}
def look(config):return {s:(config[s] if keys is None else {k:config[s][k]for k in keys})for s,keys in STYLE.items()}
def controls(config):
 return {s:config[s]for s in ['keyboard','bindings','terminal','audio','display','screencast','window_rules']}|{
  'features':{s:config[s]['enabled']for s in ['shortcuts','notifications','wifi','launcher']},
  'notification_policy':{k:config['notifications'][k]for k in ['do_not_disturb','critical_bypass_dnd','max_visible','max_queued','timeout_ms']},
  'workspace_policy':{k:config['layout'][k]for k in ['workspaces','default_floating']}}
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  result=fn()
  if result:return result
  time.sleep(.03)
 raise AssertionError('rice operation timed out')
with tempfile.TemporaryDirectory(prefix='ember-rice-')as folder:
 tmp=Path(folder);rices=tmp/'rices';rices.mkdir()
 Image.new('RGB',(48,32),(30,55,75)).save(rices/'wallpaper.jpeg')
 shutil.copy2(ROOT/'assets/fonts/EmberPanelMono.ttf',rices/'font.ttf')
 (rices/'custom.toml').write_text("version=1\n[theme]\nbackground='#203040'\nwallpaper='wallpaper.jpeg'\n[panels]\nenabled=true\nfont='font.ttf'\nfont_size=14.0\n[[panels.items]]\nname='bottom'\nanchor='bottom-center'\nmodules=['clock','date']\n[layout]\ngap=20\nmaster_ratio=0.65\n[shortcuts]\nfont_size=15.0\n[launcher]\nfont_size=17.0\nrows=6\n")
 cfg=tmp/'config.toml';text=(ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')).replace('do_not_disturb = false','do_not_disturb = true').replace('[audio]\nenabled = true','[audio]\nenabled = false');cfg.write_text(text)
 original=cfg.read_bytes();test_env=os.environ.copy();host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0');runtime=tmp/'run';runtime.mkdir(mode=0o700);test_env['XDG_RUNTIME_DIR']=str(runtime);test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display);known=set(runtime.glob('emberwm-wayland-*.sock'))
 log=(ART/'server.log').open('w');wm=None;sock=None;peer=runtime/f'ember-rice-{os.getpid()}.sock'
 def start():
  global wm,sock
  known=set(runtime.glob('emberwm-wayland-*.sock'))
  wm=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],env=test_env,stdout=log,stderr=log)
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None))
  peer.unlink(missing_ok=True);sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(peer));sock.settimeout(4);sock.connect(str(server))
 def rpc(cmd):sock.send(cmd.encode());return json.loads(sock.recv(262144))
 def ctl(cmd):result=rpc(cmd);assert not result.get('error'),result;return result
 def stop():
  if wm and wm.poll()is None:
   try:ctl('quit');wm.wait(timeout=5)
   except Exception:wm.terminate();wm.wait(timeout=5)
  if sock:sock.close()
  peer.unlink(missing_ok=True)
 def memory():
  pid=ctl('status')['pid'];text=Path(f'/proc/{pid}/smaps_rollup').read_text()
  return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',text,re.M)}
 def apply(name):ctl('rice '+name);time.sleep(.04);return ctl('config')
 passed=[]
 try:
  start();base=ctl('config');protected=controls(base)
  assert ctl('rice list')['available'][:3]==['red-dusk','midnight','minimal']
  initial=ctl('rice save original');assert Path(initial['path']).exists()
  midnight=apply('midnight');wait(lambda:len(ctl('panels')['items'])==3)
  assert len(midnight['panels']['items'])==3 and midnight['layout']['gap']==16 and controls(midnight)==protected
  ctl('screenshot '+str(ART/'midnight.png'));wait(lambda:(ART/'midnight.png').exists());assert Image.open(ART/'midnight.png').getpixel((100,400))[:3]==(16,21,38)
  minimal=apply('minimal');wait(lambda:ctl('panels')['buffer_bytes']==0)
  assert not minimal['panels']['enabled']and not minimal['panels']['items']and not minimal['theme']['wallpaper']
  assert not ctl('panels')['font_loaded']and not ctl('panels')['polling']
  back=apply('midnight');assert look(back)==look(midnight)
  passed.append('Full profiles replace panel list/state, spacing and popup fonts deterministically; three native panels render, minimal releases panel pixels/font/polling')
  custom=apply('custom');assert custom['theme']['wallpaper']==str(rices/'wallpaper.jpeg')and custom['panels']['font']==str(rices/'font.ttf');wait(lambda:len(ctl('panels')['items'])==1)
  ctl('theme forest');ctl('panels-toggle');snapshot=ctl('config');saved=ctl('rice save my-look');file=Path(saved['path']);content=file.read_bytes();doc=tomllib.loads(content.decode())
  assert stat.S_IMODE(file.stat().st_mode)==0o600
  assert doc['version']==1 and 'audio'not in doc and 'bindings'not in doc and 'rice'not in doc
  assert ctl('config')==snapshot
  apply('minimal');restored=apply('my-look');assert look(restored)==look(snapshot) and controls(restored)==protected
  assert cfg.read_bytes()==original
  assert rpc('rice save my-look').get('error')and file.read_bytes()==content
  assert rpc('rice save ../escape').get('error')and rpc('rice save red-dusk').get('error')
  (rices/'linked.toml').symlink_to(file);assert rpc('rice save linked').get('error')and file.read_bytes()==content
  assert not list(rices.glob('.rice-*.tmp'))
  passed.append('Current palette/panels/fonts/gaps save and roundtrip exactly; snapshot is private, main config untouched, existing files/symlinks never overwritten')
  for name,content in {'bad-colour':"version=1\n[theme]\nbackground='invalid'",'bad-font':"version=1\n[panels]\nfont='missing.ttf'",'bad-wallpaper':"version=1\n[theme]\nwallpaper='missing.png'",'bad-scope':"version=1\n[audio]\nenabled=true",'bad-version':"version=42",'bad-large':"#"*65537}.items():
   (rices/f'{name}.toml').write_text(content)
   before=ctl('config');generation=ctl('status')['theme_generation'];assert rpc('rice '+name).get('error'),name
   assert ctl('config')==before and ctl('status')['theme_generation']==generation,name
  os.mkfifo(rices/'not-a-file.toml');assert rpc('rice not-a-file').get('error');assert ctl('config')==before
  passed.append('Invalid colours/fonts/wallpapers, unknown sections/version, oversized and nonregular profiles fail atomically')
  apply('red-dusk');assert controls(ctl('config'))==protected
  before=ctl('config');ctl('notifications-toggle-dnd');notify=ctl('notify kept|Rice changes keep delivery state')['id'];ctl('notifications-toggle-dnd');apply('minimal');assert any(n['id']==notify for n in ctl('notifications')['items']);assert controls(ctl('config'))==protected
  for name in ['midnight','minimal','custom','my-look']:apply(name)
  apply('minimal');time.sleep(.2);before_mem=memory()
  for _ in range(10):
   for name in ['midnight','minimal','custom','my-look']:apply(name)
  apply('minimal');time.sleep(.2);after_mem=memory();assert after_mem['Pss_Anon']-before_mem['Pss_Anon']<6144,(before_mem,after_mem)
  assert ctl('launcher')['buffer_bytes']==ctl('wifi')['buffer_bytes']==ctl('screencast')['buffer_bytes']==0
  default=apply('default');assert look(default)==look(base)and controls(default)==protected
  passed.append('Forty repeated switches have bounded memory, closed popup buffers stay zero, notifications and controls survive, default restores startup appearance')
  stop();cfg.write_text(text.replace('profile = ""','profile = "custom"'))
  start();startup=ctl('config');assert look(startup)==look(custom)and controls(startup)==protected
  (rices/'custom.toml').write_text((rices/'custom.toml').read_text().replace('gap=20','gap=22'));ctl('reload');assert ctl('config')['layout']['gap']==22
  passed.append('Configured rice loads on a fresh compositor start and reload reads edited profile without recursion')
  report={'passed':passed,'before_cycles_kib':before_mem,'after_cycles_kib':after_mem,'anonymous_growth_kib':after_mem['Pss_Anon']-before_mem['Pss_Anon'],'scope':'Nested Ember, private bus, generated wallpaper and copied bundled font; no host config, audio or network settings changed'}
  (ROOT/'docs/rice-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:stop();log.close()
