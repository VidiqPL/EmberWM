#!/usr/bin/env python3
"""Notification takeover/retry, independent clients and portal payloads on a private bus."""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
from gi.repository import Gio,GLib
ROOT=Path(__file__).resolve().parents[1];NAME='org.freedesktop.Notifications'
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/dbus-'),'use dbus-run-session'
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
def call(member,args=None,dest=NAME,path='/org/freedesktop/Notifications',iface=NAME,connection=bus):
 return connection.call_sync(dest,path,iface,member,args,None,Gio.DBusCallFlags.NONE,3000,None).unpack()
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
  v=fn()
  if v:return v
  time.sleep(.02)
 raise AssertionError('notification recovery timed out')
call('RequestName',GLib.Variant('(su)',(NAME,4)),dest='org.freedesktop.DBus',path='/org/freedesktop/DBus',iface='org.freedesktop.DBus')
with tempfile.TemporaryDirectory(prefix='ember-notify-recovery-')as folder:
 tmp=Path(folder);config=tmp/'config.toml';config.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')))
 runtime=Path(os.environ['XDG_RUNTIME_DIR']);known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ROOT/'artifacts/notifications-recovery.log').open('w');proc=subprocess.Popen([str(ROOT/'target/release/emberwm'),'--nested','--config',str(config)],stdout=log,stderr=log)
 peer=runtime/f'ember-notify-recovery-{os.getpid()}.sock';sock=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);sock.bind(str(peer));sock.settimeout(4)
 try:
  server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));sock.connect(str(server))
  def ctl(cmd):sock.send(cmd.encode());return json.loads(sock.recv(262144))
  wait(lambda:ctl('notifications')['server_error']);assert not ctl('notifications')['server_running']
  call('ReleaseName',GLib.Variant('(s)',(NAME,)),dest='org.freedesktop.DBus',path='/org/freedesktop/DBus',iface='org.freedesktop.DBus')
  wait(lambda:ctl('notifications')['server_running'])
  other=Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
  def notify(connection,summary,replace=0):return call('Notify',GLib.Variant('(susssasa{sv}i)',('Fixture',replace,'',summary,'',[],{},0)),connection=connection)[0]
  first=notify(bus,'First client');second=notify(other,'Second client',first);assert first!=second
  assert {n['summary']for n in ctl('notifications')['items']}=={'First client','Second client'}
  ctl('notifications-clear')
  replies=[];errors=[];durations=[]
  def received(connection,result,*_):
   try:replies.append(connection.call_finish(result).unpack()[0])
   except GLib.Error as error:errors.append(str(error))
  for batch in range(30):
   replies=[];started=time.monotonic()
   for n in range(100):
    bus.call(NAME,'/org/freedesktop/Notifications',NAME,'Notify',GLib.Variant('(susssasa{sv}i)',('Burst',0,'',str(n),'',[],{},0)),None,Gio.DBusCallFlags.NONE,3000,None,received,None)
   wait(lambda:len(replies)==100 or errors);elapsed=time.monotonic()-started;assert not errors,errors;assert len(set(replies))==100 and elapsed<2.5,elapsed;durations.append(elapsed)
   ctl('notifications-clear')
  ctl('notifications-clear')
  portal='org.freedesktop.impl.portal.Notification';dest='org.freedesktop.impl.portal.desktop.ember';path='/org/freedesktop/portal/desktop'
  for app,title in [('test.App','Portal first'),('test.Other','Portal other'),('test.App','Portal replacement')]:
   call('AddNotification',GLib.Variant('(ssa{sv})',(app,'shared-id',{'title':GLib.Variant('s',title),'body':GLib.Variant('s','Sandbox notification path'),'default-action':GLib.Variant('s','app.open'),'default-action-target':GLib.Variant('s','target-value')})),dest,path,portal)
  activated=[]
  xml='<node><interface name="org.freedesktop.Application"><method name="ActivateAction"><arg type="s" direction="in"/><arg type="av" direction="in"/><arg type="a{sv}" direction="in"/></method></interface></node>'
  def activate(_bus,_sender,_path,_iface,_method,args,invocation):activated.append(args.unpack());invocation.return_value(GLib.Variant('()',()))
  bus.register_object('/test/App',Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0],activate,None,None)
  call('RequestName',GLib.Variant('(su)',('test.App',4)),dest='org.freedesktop.DBus',path='/org/freedesktop/DBus',iface='org.freedesktop.DBus')
  target=next(n['id']for n in ctl('notifications')['items']if n['app']=='test.App')
  ctl(f'notification-action {target} default');wait(lambda:activated);assert activated[0][0]=='open'and activated[0][1]==['target-value'],activated
  # Recreate it for the replacement/withdrawal assertions below.
  call('AddNotification',GLib.Variant('(ssa{sv})',('test.App','shared-id',{'title':GLib.Variant('s','Portal replacement')})),dest,path,portal)
  items=ctl('notifications')['items'];assert len(items)==2 and {n['summary']for n in items}=={'Portal other','Portal replacement'}
  call('RemoveNotification',GLib.Variant('(ss)',('test.App','shared-id')),dest,path,portal);assert len(ctl('notifications')['items'])==1
  report={'passed':['Notification name conflict recovers automatically without reload','One client cannot replace another client notification ID','Thirty batches of 100 asynchronous calls drain without lost wakeups or stalled replies','Portal notifications replace by app/id and withdraw independently','Portal default action activates the app with its original target'],'requests_delivered':3000,'slowest_100_seconds':round(max(durations),3),'scope':'Real nested Ember and real D-Bus clients on private session bus; host notification owner untouched'}
  (ROOT/'docs/notifications-recovery-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:
  if proc.poll()is None:
   try:ctl('quit');proc.wait(timeout=5)
   except Exception:proc.terminate();proc.wait(timeout=5)
  sock.close();peer.unlink(missing_ok=True);log.close()
