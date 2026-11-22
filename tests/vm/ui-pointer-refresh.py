#!/usr/bin/env python3
"""UI refreshes must not fake pointer motion; moved-surface coordinates must stay correct."""
import json,shlex,sys,time
from control import shell,qmp
baseline='--baseline' in sys.argv
def guest(c):return shell(c).replace('\r','')
def ctl(c):
 t=guest('emberwm ctl '+shlex.quote(c));i=t.find('{\n');assert i>=0,t
 r=json.JSONDecoder().raw_decode(t[i:])[0];assert not r.get('error'),r;return r
def wait(f):
 end=time.monotonic()+12
 while time.monotonic()<end:
  try:
   r=f()
   if r:return r
  except (AssertionError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('UI pointer refresh timed out')
def events():return [json.loads(l)for l in guest('cat /tmp/ui-pointer.log').splitlines()if l.startswith('{')]
def count(name):return sum(e['event']==name for e in events())
def move(x,y):
 for _ in range(45):
  p=ctl('status')['pointer'];dx=x-p['x'];dy=y-p['y']
  if abs(dx)<2 and abs(dy)<2:return
  qmp('input-send-event',{'events':[{'type':'rel','data':{'axis':a,'value':int(v/2)or(1 if v>0 else -1)}}for a,v in [('x',dx),('y',dy)]if abs(v)>1]});time.sleep(.03)
 raise AssertionError('pointer positioning failed')
try:
 binary='/usr/bin/emberwm-audit-baseline'if baseline else'/usr/bin/emberwm'
 guest('emberwm ctl quit; sleep 1; '+binary+' --native --config /tmp/config.toml >/tmp/ui-pointer-server.log 2>&1 &')
 s=wait(lambda:ctl('status'))
 guest('WAYLAND_DISPLAY='+shlex.quote(s['wayland_display'])+' ember-pointer-client --app-id ui-pointer >/tmp/ui-pointer.log 2>&1 &')
 wait(lambda:any(w['app_id']=='ui-pointer'and w['placement_ready']for w in ctl('status')['windows']))
 move(180,180);wait(lambda:count('pointer-enter'));time.sleep(.25)
 before=count('pointer-motion');ctl('notify Unrelated card|Pointer stays over the app')
 wait(lambda:ctl('status')['ui']['cards']);time.sleep(.15)
 notify_motion=count('pointer-motion')-before
 assert notify_motion>0 if baseline else notify_motion==0,notify_motion
 before=count('pointer-motion');ctl('notifications-clear');wait(lambda:not ctl('status')['ui']['cards']);time.sleep(.15)
 clear_motion=count('pointer-motion')-before
 assert clear_motion>0 if baseline else clear_motion==0,clear_motion
 # Hiding a reserved panel moves the SAME surface under the stationary pointer.
 before=count('pointer-motion');ctl('panels-toggle')
 wait(lambda:count('pointer-motion')>before)
 s=ctl('status');w=next(w for w in s['windows']if w['app_id']=='ui-pointer');p=s['pointer']
 motion=[e for e in events()if e['event']=='pointer-motion'][-1]
 assert abs(motion['x']-(p['x']-w['geometry']['x']))<2,(motion,p,w['geometry'])
 assert abs(motion['y']-(p['y']-w['geometry']['y']))<2,(motion,p,w['geometry'])
 # A card appearing UNDER the pointer must still take input away from the app.
 ctl('notify Locate card|Synthetic test');wait(lambda:ctl('status')['ui']['cards'])
 r=ctl('status')['ui']['cards'][0];ctl('notifications-clear');wait(lambda:not ctl('status')['ui']['cards'])
 move(r['x']+45,r['y']+40);time.sleep(.1);before=count('pointer-leave')
 ctl('notify Under pointer|This card should receive the click');wait(lambda:count('pointer-leave')>before)
 before=count('pointer-button')
 for down in [True,False]:qmp('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':down}}]});time.sleep(.08)
 wait(lambda:not ctl('status')['ui']['cards']);assert count('pointer-button')==before
 print(json.dumps({'baseline':baseline,'unrelated_notification_motion_events':notify_motion,'notification_dismissal_motion_events':clear_motion,'moved_surface_coordinates':'correct','card_under_pointer':'takes input without click leakage'},indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
