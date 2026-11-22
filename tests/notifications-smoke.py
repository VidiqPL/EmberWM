#!/usr/bin/env python3
"""Real D-Bus notification clients and a real nested compositor on a private bus.
Run: dbus-run-session -- python3 tests/notifications-smoke.py
"""
import json, os, re, socket, subprocess, tempfile, time
from pathlib import Path
from gi.repository import Gio, GLib
ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/'target/release/emberwm'
ART=ROOT/'artifacts'; ART.mkdir(exist_ok=True)
NAME='org.freedesktop.Notifications'; OBJECT='/org/freedesktop/Notifications'
# Refuse to test against the user's real desktop bus.
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/dbus-'), 'use dbus-run-session for a private test bus'
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
def dbus_call(member,args=None,interface=NAME,obj=OBJECT,dest=NAME):
    return bus.call_sync(dest,obj,interface,member,args,None,Gio.DBusCallFlags.NONE,3000,None).unpack()
def wait(fn,timeout=10):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        while GLib.MainContext.default().pending(): GLib.MainContext.default().iteration(False)
        v=fn()
        if v: return v
        time.sleep(.04)
    raise AssertionError('notification/shortcut operation timed out')
def memory(pid):
    data=Path(f'/proc/{pid}/smaps_rollup').read_text()
    return {k:int(v) for k,v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|Private_Dirty):\s+(\d+)',data,re.M)}
with tempfile.TemporaryDirectory(prefix='ember-ui-test-') as directory:
    tmp=Path(directory);config=tmp/'config.toml'
    original=(ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png'))
    config.write_text(original)
    test_env=os.environ.copy();host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0');runtime=tmp/'run';runtime.mkdir(mode=0o700);test_env['XDG_RUNTIME_DIR']=str(runtime);test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display)
    before=set(runtime.glob('emberwm-wayland-*.sock'))
    log=(ART/'notifications.log').open('w');proc=subprocess.Popen([str(BIN),'--nested','--config',str(config)],env=test_env,stdout=log,stderr=log)
    path=runtime/f'ember-notify-test-{os.getpid()}.sock';client=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);client.bind(str(path));client.settimeout(4)
    passed=[];second=None
    try:
        server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-before),None));client.connect(str(server))
        def ctl(command,error=False):
            client.send(command.encode());d=json.loads(client.recv(262144));assert bool(d.get('error'))==error,(command,d);return d
        def state(): return ctl('status')
        wait(lambda:state()['notifications']['server_running'])
        signals=[]
        def receive(_connection,_sender,_path,_interface,member,parameters,_data): signals.append((member,parameters.unpack()))
        subscription=bus.signal_subscribe(NAME,NAME,None,OBJECT,None,Gio.DBusSignalFlags.NONE,receive,None)
        def notify(summary='Hello',body='Native notification',replace=0,timeout=0,actions=(),hints=None):
            return dbus_call('Notify',GLib.Variant('(susssasa{sv}i)',('Ember tests',replace,'',summary,body,list(actions),hints or {},timeout)))[0]
        assert set(dbus_call('GetCapabilities')[0])=={'body','actions'}
        assert dbus_call('GetServerInformation')[0]=='EmberWM'
        introspection=dbus_call('Introspect',interface='org.freedesktop.DBus.Introspectable')[0]
        assert 'CloseNotification' in introspection and 'ActionInvoked' in introspection
        passed.append('standard notification D-Bus methods and introspection work')
        first=notify(actions=('default','Open','copy','Copy'))
        s=wait(lambda:(s if s['ui']['cards'] else None) if (s:=state()) else None)
        assert s['notifications']['items'][0]['id']==first and s['ui']['cards'][0]['buttons'][0]['key']=='copy'
        # Compact cards sit at the configured screen margin, with no app header.
        card=s['ui']['cards'][0]
        assert card['y']==12 and card['width']==360,card
        assert notify(summary='Updated',replace=first)==first
        wait(lambda:state()['notifications']['items'][0]['summary']=='Updated')
        assert state()['notifications']['queued']==1
        dbus_call('CloseNotification',GLib.Variant('(u)',(first,)))
        wait(lambda:('NotificationClosed',(first,3)) in signals)
        passed.append('replacement keeps the ID; explicit close emits NotificationClosed reason 3')
        expiring=notify(timeout=180)
        wait(lambda:('NotificationClosed',(expiring,1)) in signals)
        persistent=notify();time.sleep(.3);assert state()['notifications']['queued']==1
        ctl('notification-dismiss');wait(lambda:('NotificationClosed',(persistent,2)) in signals)
        passed.append('expiry, persistent timeout zero and user dismissal emit correct close reasons')
        ctl('notifications-toggle-dnd');normal=notify();critical=notify(summary='Critical',timeout=-1,hints={'urgency':GLib.Variant('y',2)})
        wait(lambda:state()['notifications']['queued']==1)
        assert state()['notifications']['items'][0]['id']==critical
        assert state()['notifications']['items'][0]['persistent']
        ctl('notifications-toggle-dnd');ctl('notifications-clear')
        passed.append('Do Not Disturb suppresses normal notices while critical notices bypass and persist')
        for i in range(60): notify(summary=f'Bounded notice {i}',body='x'*3000)
        s=state();assert s['notifications']['queued']==32 and len(s['ui']['cards'])<=4
        assert all(len(n['body'])<=1024 for n in s['notifications']['items'])
        ctl('notifications-clear');wait(lambda:not state()['ui']['cards'])
        passed.append('notification storms are bounded to the configured queue, visible cards and text limits')
        try: dbus_call('Notify',GLib.Variant('(s)',('invalid',)))
        except GLib.Error as e: assert 'InvalidArgs' in str(e)
        else: raise AssertionError('malformed Notify unexpectedly accepted')
        passed.append('malformed D-Bus requests return errors without disrupting the compositor')
        ctl('shortcuts-toggle');s=wait(lambda:(s if s['ui']['shortcuts_open'] and s['ui']['help'] else None) if (s:=state()) else None)
        assert any(r['keys']=='Alt+/' and r['action']=='shortcuts-toggle' for r in s['ui']['rows'])
        custom=original.replace('key = "Return"\naction = "terminal"','key = "u"\naction = "terminal"\ndescription = "My terminal"')
        config.write_text(custom);ctl('reload')
        wait(lambda:any(r['keys']=='Alt+u' and r['description']=='My terminal' for r in state()['ui']['rows']))
        config.write_text('broken config !');ctl('reload',True)
        assert any(r['description']=='My terminal' for r in state()['ui']['rows'])
        passed.append('visible shortcut sheet follows live reloaded bindings/descriptions and survives rejected reload')
        notify(summary='Ember notifications',body='Native cards, no separate UI daemon.',actions=('default','Open','copy','Copy'))
        shot=ROOT/'docs/notifications-shortcuts-preview.png';shot.unlink(missing_ok=True);ctl(f'screenshot {shot}');wait(lambda:shot.exists() and shot.stat().st_size>100)
        ctl('shortcuts-toggle');ctl('notifications-clear')
        config.write_text(original.replace('enabled = true\nfont = "builtin"','enabled = false\nfont = "builtin"'));ctl('reload')
        wait(lambda:not state()['ui']['font_loaded'] and not state()['panels']['font_loaded'])
        baseline=memory(proc.pid)
        for _ in range(30):
            ctl('shortcuts-toggle');wait(lambda:state()['ui']['help'])
            ctl('shortcuts-toggle');wait(lambda:not state()['ui']['help'])
        after=memory(proc.pid)
        assert after['Pss_Anon']-baseline['Pss_Anon']<10*1024,(baseline,after)
        s=state();assert s['ui']['buffer_bytes']==0 and not s['ui']['font_loaded'] and not s['panels']['font_loaded']
        passed.append('30 shortcut open/close cycles stay bounded; hidden overlays release buffers and shared fonts')
        # A second Ember must leave the existing owner intact.
        owner=dbus_call('GetNameOwner',GLib.Variant('(s)',(NAME,)),interface='org.freedesktop.DBus',obj='/org/freedesktop/DBus',dest='org.freedesktop.DBus')[0]
        known=set(runtime.glob('emberwm-wayland-*.sock'));second=subprocess.Popen([str(BIN),'--nested','--config',str(config)],env=test_env,stdout=log,stderr=log)
        extra=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None))
        other=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);otherpath=runtime/f'ember-notify-other-{os.getpid()}.sock';other.bind(str(otherpath));other.settimeout(4);other.connect(str(extra))
        time.sleep(.3);other.send(b'status');otherstatus=json.loads(other.recv(262144));assert not otherstatus['notifications']['server_running'] and 'already owns' in otherstatus['notifications']['server_error']
        assert owner==dbus_call('GetNameOwner',GLib.Variant('(s)',(NAME,)),interface='org.freedesktop.DBus',obj='/org/freedesktop/DBus',dest='org.freedesktop.DBus')[0]
        other.send(b'quit');other.recv(65536);second.wait(timeout=5);other.close();otherpath.unlink(missing_ok=True)
        passed.append('another session notification server is respected and never replaced')
        config.write_text(original.replace('[notifications]\nenabled = true','[notifications]\nenabled = false'));ctl('reload')
        wait(lambda:not state()['notifications']['server_running'])
        has_owner=dbus_call('NameHasOwner',GLib.Variant('(s)',(NAME,)),interface='org.freedesktop.DBus',obj='/org/freedesktop/DBus',dest='org.freedesktop.DBus')[0];assert not has_owner
        config.write_text(original);ctl('reload');wait(lambda:state()['notifications']['server_running'])
        passed.append('disabling notifications releases the D-Bus name; reload can re-enable it')
        bus.signal_unsubscribe(subscription)
        report={'passed':passed,'memory_before_cycles_kib':baseline,'memory_after_cycles_kib':after,'scope':'Real nested compositor and persistent Gio D-Bus client on a private dbus-run-session bus. Host desktop notification server and audio are untouched.'}
        (ROOT/'docs/notifications-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    finally:
        if second and second.poll() is None: second.terminate();second.wait(timeout=5)
        if proc.poll() is None:
            try:ctl('quit');proc.wait(timeout=5)
            except Exception:proc.terminate();proc.wait(timeout=5)
        client.close();path.unlink(missing_ok=True);log.close()
