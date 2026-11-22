#!/usr/bin/env python3
"""Wheel-only input must not synthesize motion; stationary popups still receive scrolling."""
import json,shlex,sys,time
from pathlib import Path
from control import shell,qmp
ROOT=Path(__file__).resolve().parents[2]
baseline='--baseline' in sys.argv
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
 text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text
 result=json.JSONDecoder().raw_decode(text[start:])[0];assert not result.get('error'),result;return result
def wait(fn):
 end=time.monotonic()+10
 while time.monotonic()<end:
  result=fn()
  if result:return result
  time.sleep(.05)
 raise AssertionError('scroll operation timed out')
def events():return [json.loads(line)for line in guest('cat /tmp/scroll-pointer.log').splitlines()if line.startswith('{')]
def count(name):return sum(e['event']==name for e in events())
def button(name,down):qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':name}}]})
def wheel():
 button('wheel-down',True);button('wheel-down',False);time.sleep(.2)
try:
 s=ctl('status');assert not s['windows']
 guest('WAYLAND_DISPLAY='+shlex.quote(s['wayland_display'])+' ember-pointer-client --app-id scroll-pointer --control /tmp/scroll-pointer-control >/tmp/scroll-pointer.log 2>&1 &')
 wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready'])
 qmp('input-send-event',{'events':[{'type':'rel','data':{'axis':'x','value':100}},{'type':'rel','data':{'axis':'y','value':100}}]})
 wait(lambda:count('pointer-enter'));time.sleep(.2)
 before=count('pointer-motion');axes=count('pointer-axis');wheel();extra=count('pointer-motion')-before
 if baseline:
  assert extra>0,extra;print(json.dumps({'baseline_extra_motion_per_notch':extra}));sys.exit(0)
 assert extra==0,extra;assert count('pointer-axis')==axes+1,events()
 button('middle',True);time.sleep(.1);before=count('pointer-motion');axes=count('pointer-axis');wheel();button('middle',False)
 assert count('pointer-motion')==before and count('pointer-axis')==axes+1
 # An input-taking X11 popup appears over the unchanged pointer. Its map must
 # preserve managed keyboard focus, while the next wheel reaches the popup.
 x=s['xwayland'];xenv='DISPLAY='+shlex.quote(x['display'])+' XAUTHORITY='+shlex.quote(x['auth'])
 guest(xenv+' ember-x11-window popup >/tmp/scroll-popup.log 2>&1 &')
 wait(lambda:len(ctl('status')['windows'])==2)
 assert ctl('status')['keyboard_focus_app']=='scroll-pointer'
 pointer=ctl('status')['pointer'];assert 40<=pointer['x']<460 and 50<=pointer['y']<310,pointer
 wheel();wait(lambda:'"button":5'in guest('cat /tmp/scroll-popup.log'))
 assert ctl('status')['keyboard_focus_app']=='scroll-pointer'
 report={'ordinary_wheel_extra_motion':extra,'passed':['Ordinary wheel input sends one axis event without synthetic pointer motion','Middle-button wheel input sends one axis event without synthetic pointer motion','Input-taking X11 popup under stationary pointer receives scrolling without stealing keyboard focus'],'scope':'Native virtual seat only; Steam usability requires user check'}
 (ROOT/'docs/scroll-focus-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
