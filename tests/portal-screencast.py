#!/usr/bin/env python3
"""Full portal -> native consent -> screencopy -> private PipeWire video test.
Run: dbus-run-session -- python3 tests/portal-screencast.py
"""
import json,os,re,shutil,socket,subprocess,tempfile,time
from pathlib import Path
from gi.repository import Gio,GLib
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];WINDOW_TEST=os.environ.get('EMBER_TEST_WINDOW')=='1';ART=ROOT/('artifacts/portal-window-test' if WINDOW_TEST else 'artifacts/portal-test');ART.mkdir(parents=True,exist_ok=True)
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/dbus-'),'use dbus-run-session'
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None);PORTAL='org.freedesktop.portal.Desktop';PATH='/org/freedesktop/portal/desktop';IFACE='org.freedesktop.portal.ScreenCast'
def dbus_call(dest,path,iface,member,args=None):return bus.call_sync(dest,path,iface,member,args,None,Gio.DBusCallFlags.NONE,5000,None).unpack()
def wait(fn,timeout=20):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  v=fn()
  if v:return v
  time.sleep(.025)
 raise AssertionError('portal operation timed out')
def owner(name):return dbus_call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner',GLib.Variant('(s)',(name,)))[0]
responses={}
def response(_bus,_sender,path,_iface,_signal,parameters,_data):responses[path]=parameters.unpack()
bus.signal_subscribe(None,'org.freedesktop.portal.Request','Response',None,None,Gio.DBusSignalFlags.NONE,response,None)
with tempfile.TemporaryDirectory(prefix='ember-portal-')as folder:
 tmp=Path(folder);config=tmp/'config';data=tmp/'data';pwrun=tmp/'pwrun';pwrun.mkdir(mode=0o700);state=tmp/'state';state.mkdir()
 for p in [config/'xdg-desktop-portal',config/'xdg-desktop-portal-wlr',data/'xdg-desktop-portal/portals',config/'wireplumber/wireplumber.conf.d']:p.mkdir(parents=True,exist_ok=True)
 shutil.copy2(ROOT/'packaging/ember.portal',data/'xdg-desktop-portal/portals/ember.portal');shutil.copy2(ROOT/'packaging/ember-portals.conf',config/'xdg-desktop-portal/ember-portals.conf')
 (config/'xdg-desktop-portal-wlr/Ember').write_text(f'[screencast]\nchooser_type=dmenu\nchooser_cmd="{ROOT}/target/release/emberwm" --choose-source\nmax_fps=30\n')
 (config/'wireplumber/wireplumber.conf.d/99-test.conf').write_text('wireplumber.profiles = { main = { hardware.audio = disabled hardware.bluetooth = disabled hardware.video-capture = disabled monitor.alsa = disabled monitor.alsa-midi = disabled monitor.bluez = disabled monitor.v4l2 = disabled monitor.libcamera = disabled support.logind = disabled support.reserve-device = disabled support.portal-permissionstore = disabled } }')
 cfg=tmp/'config.toml';cfg.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')))
 env=os.environ.copy();env.update(XDG_CURRENT_DESKTOP='Ember',XDG_SESSION_TYPE='wayland',XDG_CONFIG_HOME=str(config),XDG_DATA_HOME=str(data),XDG_STATE_HOME=str(state),XDG_CACHE_HOME=str(tmp/'cache'),XDG_DATA_DIRS=str(data)+':'+env.get('XDG_DATA_DIRS','/usr/local/share:/usr/share'),PIPEWIRE_RUNTIME_DIR=str(pwrun),PIPEWIRE_REMOTE='pipewire-0',GDK_BACKEND='wayland',QT_QPA_PLATFORM='wayland',NO_AT_BRIDGE='1')
 host_runtime=Path(env['XDG_RUNTIME_DIR']);host_display=env.get('WAYLAND_DISPLAY','wayland-0');runtime=tmp/'run';runtime.mkdir(mode=0o700);env['XDG_RUNTIME_DIR']=str(runtime);env['WAYLAND_DISPLAY']=str(host_runtime/host_display) if not host_display.startswith('/') else host_display;known=set();processes=[];logs=[];sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);peer=runtime/f'ember-portal-{os.getpid()}.sock';sock.bind(str(peer));sock.settimeout(4)
 def start(args,name,extra=None):
  log=(ART/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(args,env=extra or env,stdout=log,stderr=log,start_new_session=True);processes.append(p);return p
 def ctl(cmd):sock.send(cmd.encode());r=json.loads(sock.recv(262144));assert not r.get('error'),r;return r
 def rpc(member,args):return dbus_call(PORTAL,PATH,IFACE,member,args)[0]
 try:
  wm=start([str(ROOT/'target/release/emberwm'),'--nested','--config',str(cfg)],'ember')
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server));status=ctl('status');env['WAYLAND_DISPLAY']=status['wayland_display'];env['EMBER_SOCKET']=str(server);env.pop('DISPLAY',None)
  update={k:v for k,v in env.items()if k in ('XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_DATA_DIRS','XDG_STATE_HOME','XDG_CACHE_HOME','WAYLAND_DISPLAY','XDG_CURRENT_DESKTOP','XDG_SESSION_TYPE','PIPEWIRE_RUNTIME_DIR','PIPEWIRE_REMOTE','GDK_BACKEND','QT_QPA_PLATFORM','EMBER_SOCKET','NO_AT_BRIDGE')};update['DISPLAY']=''
  dbus_call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','UpdateActivationEnvironment',GLib.Variant('(a{ss})',(update,)))
  fifo=tmp/'client.fifo';start([str(ROOT/'artifacts/native-vm/wm-client'),'--control',str(fifo)],'pattern');wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('fullscreen')
  with fifo.open('w')as f:f.write('pattern\n')
  if WINDOW_TEST:
   ctl('fullscreen');ctl('float')
   start([str(ROOT/'artifacts/native-vm/wm-client'),'--app-id','share-occluder','--title','Must never appear in window share'],'occluder');wait(lambda:len(ctl('status')['windows'])==2);ctl('fullscreen')
  raw=subprocess.run([str(ROOT/'artifacts/native-vm/screencopy-client')],env=env,capture_output=True,timeout=5);assert raw.returncode==4,raw.returncode
  media_env=env
  if os.environ.get('EMBER_TEST_REUSED_PIPEWIRE_BUS')=='1':
   media_bus=tmp/'media-bus';start(['dbus-daemon','--session','--nofork','--address=unix:path='+str(media_bus)],'media-bus');wait(lambda:media_bus.exists());media_env=env.copy();media_env['DBUS_SESSION_BUS_ADDRESS']='unix:path='+str(media_bus)
  start(['pipewire'],'pipewire',media_env);wait(lambda:(pwrun/'pipewire-0').exists());start(['wireplumber'],'wireplumber',media_env)
  start(['/usr/libexec/xdg-desktop-portal-wlr','-l','DEBUG','-c',str(config/'xdg-desktop-portal-wlr/Ember')],'wlr');wait(lambda:owner('org.freedesktop.impl.portal.desktop.wlr'))
  start(['/usr/libexec/xdg-desktop-portal','--verbose'],'portal');wait(lambda:owner(PORTAL))
  def session(token,allow):
   handle=rpc('CreateSession',GLib.Variant('(a{sv})',({'handle_token':GLib.Variant('s',token+'_create'),'session_handle_token':GLib.Variant('s',token)},)));code,result=wait(lambda:responses.get(handle));assert code==0,(code,result);session=result['session_handle']
   handle=rpc('SelectSources',GLib.Variant('(oa{sv})',(session,{'handle_token':GLib.Variant('s',token+'_select'),'types':GLib.Variant('u',2 if WINDOW_TEST else 1),'cursor_mode':GLib.Variant('u',2),'persist_mode':GLib.Variant('u',0)})))
   wait(lambda:ctl('screencast')['prompt_open']);request_token=ctl('screencast')['prompt_token']
   if WINDOW_TEST:
    choices=ctl('screencast')['sources'];choice=next(c for c in choices if c['value'].startswith('Window: ')and 'Rule test' in c['label']);ctl('share-select '+choice['value'])
   ctl(f'share-respond {request_token} '+('allow' if allow else 'deny'))
   code,result=wait(lambda:responses.get(handle))
   if not allow:assert code!=0;return session,None
   assert code==0,(code,result)
   handle=rpc('Start',GLib.Variant('(osa{sv})',(session,'',{'handle_token':GLib.Variant('s',token+'_start')})));code,result=wait(lambda:responses.get(handle));assert code==0,(code,result);return session,result
  canceled,_=session('ember_cancel',False)
  sharing,result=session('ember_share',True);node=result['streams'][0][0]
  reply,fdlist=bus.call_with_unix_fd_list_sync(PORTAL,PATH,IFACE,'OpenPipeWireRemote',GLib.Variant('(oa{sv})',(sharing,{})),None,Gio.DBusCallFlags.NONE,5000,None,None);fd=fdlist.get(reply.unpack()[0])
  file=ART/'video.ppm';log=(ART/'consumer.log').open('w');logs.append(log);consumer=subprocess.Popen([str(ROOT/'artifacts/pipewire-video-client'),str(fd),str(node),str(file)],env=env,pass_fds=[fd],stdout=log,stderr=log,start_new_session=True);processes.append(consumer);os.close(fd)
  wait(lambda:consumer.poll()is not None,25);assert consumer.returncode==0,(consumer.returncode,(ART/'consumer.log').read_text())
  video=json.loads((ART/'consumer.log').read_text().splitlines()[-1]);im=Image.open(file);expected=lambda x,y:(x%251,y%251,48 if y<im.height/2 else 208)
  for x,y in[(100,120),(200,im.height-100)]:assert im.getpixel((x,y))==expected(x,y),(video,(x,y),im.getpixel((x,y)),expected(x,y))
  if WINDOW_TEST:
   assert result['streams'][0][1]['source_type']==2,result
   assert im.width<1280 or im.height<800,im.size
   assert ctl('status')['windows'][0]['visible']==False,ctl('status')['windows']
  # Exercise the actual desktop portal notification frontend as well as its native backend.
  dbus_call(PORTAL,PATH,'org.freedesktop.portal.Notification','AddNotification',GLib.Variant('(sa{sv})',('fixture-notification',{'title':GLib.Variant('s','Portal frontend test'),'body':GLib.Variant('s','Delivered to Ember without GTK notification rendering')})))
  wait(lambda:any(n['summary']=='Portal frontend test'for n in ctl('notifications')['items']))
  assert 'Using ember.portal for org.freedesktop.impl.portal.Notification' in (ART/'portal.log').read_text()
  dbus_call(PORTAL,sharing,'org.freedesktop.portal.Session','Close');wait(lambda:ctl('screencast')['pending_frames']==0);time.sleep(2.2)
  report={'passed':['Direct capture hidden from ordinary clients by default','Real ScreenCast portal rejects canceled native prompt','Explicit native approval creates a PipeWire stream','Portal-provided remote FD delivers at least 12 actual video frames','PipeWire frame pixels match synthetic pattern orientation/channels','Isolated single-window capture excludes fullscreen occluder and compositor panels' if WINDOW_TEST else 'Real Notification portal frontend delivers to native Ember backend','Closing session releases pending capture frames'],'video':video,'media_bus_matches_desktop':not bool(os.environ.get('EMBER_TEST_REUSED_PIPEWIRE_BUS')),'scope':'Private session bus, isolated PipeWire/WirePlumber without hardware monitors, nested Ember with synthetic fullscreen client; host screen/audio/network untouched'}
  (ROOT/('docs/portal-window-results.json' if WINDOW_TEST else ('docs/portal-reused-media-results.json' if os.environ.get('EMBER_TEST_REUSED_PIPEWIRE_BUS') else 'docs/portal-results.json'))).write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  for name in ['org.freedesktop.impl.portal.desktop.gtk','org.freedesktop.portal.Documents','org.freedesktop.impl.portal.PermissionStore']:
   try:
    if owner(name):
     pid=dbus_call('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','GetConnectionUnixProcessID',GLib.Variant('(s)',(name,)))[0];os.kill(pid,15)
   except (GLib.Error,ProcessLookupError):pass
  for p in reversed(processes):
   if p.poll()is None:
    p.terminate()
    try:p.wait(timeout=4)
    except subprocess.TimeoutExpired:p.kill();p.wait(timeout=2)
  sock.close();peer.unlink(missing_ok=True)
  for log in logs:log.close()
