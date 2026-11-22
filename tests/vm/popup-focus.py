#!/usr/bin/env python3
"""Reproduce an app-owned dropdown that dismisses itself on X11 focus loss."""
import json,shlex,time,sys
from pathlib import Path
from control import shell,qmp
ROOT=Path(__file__).resolve().parents[2];baseline='--baseline'in sys.argv
passed=[]
def guest(c):return shell(c).replace('\r','')
def ctl(c):
 t=guest('emberwm ctl '+shlex.quote(c));start=t.find('{\n');assert start>=0,t;d=json.JSONDecoder().raw_decode(t[start:])[0];assert not d.get('error'),d;return d
def state():return ctl('status')
def wait(f,seconds=12):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   result=f()
   if result:return result
  except AssertionError:pass
  time.sleep(.05)
 raise AssertionError('Popup regression timed out')
def move(x,y):
 for _ in range(40):
  p=state()['pointer'];dx=x-p['x'];dy=y-p['y']
  if abs(dx)<2 and abs(dy)<2:return
  e=[{'type':'rel','data':{'axis':a,'value':int(v/2)or(1 if v>0 else -1)}}for a,v in [('x',dx),('y',dy)]if abs(v)>1]
  qmp('input-send-event',{'events':e});time.sleep(.04)
 raise AssertionError('Pointer did not converge')
def click():
 for down in [True,False]:qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});time.sleep(.1)
def events():return guest('cat /tmp/popup-focus.log')
try:
 guest('emberwm ctl quit; sleep 1; '+('/usr/bin/emberwm-baseline'if baseline else '/usr/bin/emberwm')+' --native --config /tmp/config.toml >/tmp/popup-server.log 2>&1 &')
 s=wait(lambda:state());x=s['xwayland'];env='DISPLAY='+shlex.quote(x['display'])+' XAUTHORITY='+shlex.quote(x['auth'])
 guest(env+' ember-x11-popup ewmh >/tmp/popup-focus.log 2>&1 & echo $! >/tmp/popup-focus.pid')
 wait(lambda:any(w['app_id']=='PopupFixture'and w['focused']for w in state()['windows']))
 move(500,400);click();wait(lambda:'popup-open'in events());time.sleep(.4)
 popup=next((w for w in state()['windows']if w['app_id']=='PopupFixtureMenu'),None)
 if popup and not baseline:
  assert state().get('keyboard_focus_app')=='PopupFixture',state().get('keyboard_focus_app')
 if popup:
  move(180,160);click();time.sleep(.5)
 logs=events();dialog=next((w for w in state()['windows']if w['app_id']=='FriendDialog'),None)
 if baseline:
  assert not dialog and ('popup-dismissed-by-focus'in logs or 'popup-dismissed-by-active-window'in logs),logs
  print('Baseline reproduced: redundant parent activation closes the client-owned popup')
 else:
  assert popup,'Dropdown vanished without selecting an item'
  assert dialog and dialog['visible']and dialog['focused']and dialog['floating'],(dialog,logs)
  assert dialog['geometry']['width']==340 and dialog['geometry']['height']==220,dialog
  assert 'popup-item-clicked'in logs and 'friend-dialog-open'in logs,logs
  report={'passed':['App-owned dropdown survives EWMH monitoring and a redundant parent activation request','Override-redirect popup retains app-owned X focus while compositor keyboard/EWMH focus stays on its managed parent','Selecting the menu item opens a visible, focused, floating child dialog with the requested geometry'],'scope':'Native DRM virtual seat and real X11 app, matching focus-out dismissal behavior; Steam itself not driven'}
  (ROOT/'docs/popup-focus-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:guest('kill $(cat /tmp/popup-focus.pid)');ctl('quit')
 except Exception:pass
