#!/usr/bin/env python3
"""X11 focus/grab recovery after an independently focused capture window crashes."""
import json,shlex,sys,time
from pathlib import Path
from control import shell,qmp
ROOT=Path(__file__).resolve().parents[2];baseline='--baseline'in sys.argv
def guest(c):return shell(c).replace('\r','')
def ctl(c):
 t=guest('emberwm ctl '+shlex.quote(c));i=t.find('{\n');assert i>=0,t
 d=json.JSONDecoder().raw_decode(t[i:])[0];assert not d.get('error'),d;return d
def wait(f):
 end=time.monotonic()+12
 while time.monotonic()<end:
  try:
   r=f()
   if r:return r
  except (AssertionError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('crash recovery timed out')
def move(x,y):
 for _ in range(45):
  p=ctl('status')['pointer'];dx=x-p['x'];dy=y-p['y']
  if abs(dx)<2 and abs(dy)<2:return
  events=[{'type':'rel','data':{'axis':a,'value':int(v/2)or(1 if v>0 else -1)}}for a,v in [('x',dx),('y',dy)]if abs(v)>1]
  qmp('input-send-event',{'events':events});time.sleep(.04)
 raise AssertionError('pointer positioning failed')
def button(down):qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});time.sleep(.08)
def key(k):
 for down in [True,False]:qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}]});time.sleep(.08)
try:
 guest('emberwm ctl quit; sleep 1; '+('/usr/bin/emberwm-crash-baseline'if baseline else '/usr/bin/emberwm')+' --native --config /tmp/config.toml >/tmp/crash-server.log 2>&1 &')
 s=wait(lambda:ctl('status'));x=s['xwayland'];env='DISPLAY='+shlex.quote(x['display'])+' XAUTHORITY='+shlex.quote(x['auth'])
 def focus():
  t=guest(env+' ember-x11-window focus-state');return next(json.loads(l)['focus']for l in t.splitlines()if l.startswith('{'))
 guest(env+' ember-x11-window active >/tmp/crash-parent.log 2>&1 &')
 parent=wait(lambda:next((w for w in ctl('status')['windows']if w['app_id']=='EmberX11Test'and w['focused']),None))
 wait(lambda:focus()==parent['x11_id']);move(180,180)
 guest(env+' ember-x11-window orphan >/tmp/crash-child.log 2>&1 & echo $! >/tmp/crash-child.pid')
 child=wait(lambda:next((w for w in ctl('status')['windows']if w['app_id']=='CrashFixture'),None))
 wait(lambda:focus()==child['x11_id']);move(190,190);button(True)
 guest('kill -9 $(cat /tmp/crash-child.pid)')
 wait(lambda:all(w['app_id']!='CrashFixture'for w in ctl('status')['windows']));time.sleep(.3)
 if baseline:
  assert focus()==0,focus();button(False);button(True);button(False)
  assert focus()==0,focus();print(json.dumps({'baseline':'X11 capture-window crash leaves focus None; clicking logically focused parent does not recover'}));sys.exit(0)
 wait(lambda:focus()==parent['x11_id'])
 s=ctl('status');assert not s['pointer_grabbed'] and not s['pointer_buttons'],s
 button(False);button(True);button(False);key('w')
 assert '"key":25'in guest('cat /tmp/crash-parent.log')
 # An explicit click also repairs empty X focus when no unmap callback occurred.
 guest(env+' ember-x11-window focus-none');assert focus()==0
 button(True);button(False);wait(lambda:focus()==parent['x11_id']);key('a')
 assert '"key":38'in guest('cat /tmp/crash-parent.log')
 guest(env+' ember-x11-window focus-root')
 button(True);button(False);wait(lambda:focus()==parent['x11_id'])
 # A managed fullscreen game window must release its grab on a process crash too.
 guest(env+' ember-x11-window game >/tmp/crash-game.log 2>&1 & echo $! >/tmp/crash-game.pid')
 wait(lambda:any(w['app_id']=='CrashFixture'and w['focused']for w in ctl('status')['windows']))
 ctl('fullscreen');move(300,300);button(True)
 guest('kill -9 $(cat /tmp/crash-game.pid)')
 wait(lambda:all(w['app_id']!='CrashFixture'for w in ctl('status')['windows']))
 wait(lambda:focus()==parent['x11_id'])
 s=ctl('status');assert not s['pointer_grabbed'] and not s['pointer_buttons'],s
 button(False);button(True);button(False);key('d')
 assert '"key":40'in guest('cat /tmp/crash-parent.log')
 report={'passed':['Capture window killed while holding a mouse button returns input focus to its managed parent','Dead window leaves no pointer grab or tracked button presses','Parent receives real clicks and W after crash without restarting','Clicking an already logically focused X11 window repairs empty/root X focus and delivers A','Managed fullscreen game-style window crash also clears held input and returns D to the parent'],'scope':'Native virtual seat and separate X11 processes; no actual Steam game was crashed'}
 (ROOT/'docs/crash-focus-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:button(False);ctl('quit')
 except Exception:pass
