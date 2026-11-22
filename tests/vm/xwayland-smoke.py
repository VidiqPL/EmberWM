#!/usr/bin/env python3
"""Native DRM/virtual seat X11 window input and clipboard, isolated in QEMU."""
import hashlib,json,shlex,time
from pathlib import Path
from control import shell,qmp
ROOT=Path(__file__).resolve().parents[2]
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
 text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text
 result=json.JSONDecoder().raw_decode(text[start:])[0];assert not result.get('error'),result;return result
def state():return ctl('status')
def wait(fn,seconds=15):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   result=fn()
   if result:return result
  except AssertionError:pass
  time.sleep(.08)
 raise AssertionError('Native X11 test timed out')
def key(*keys):
 for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
  assert 'error'not in qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});time.sleep(.1)
passed=[]
s=wait(lambda:state());assert s['backend']=='native',s['backend'];x=s['xwayland'];assert not x['ready']and x['pid'],x
assert 'xwayland-helper'in guest(f'busybox readlink /proc/{x["pid"]}/exe')
key('meta_l','comma');wait(lambda:state()['tray']['open']);key('esc');wait(lambda:not state()['tray']['open'])
passed.append('Actual Super+Comma opens the native tray UI and Escape closes it')
xenv='DISPLAY='+shlex.quote(x['display'])+' XAUTHORITY='+shlex.quote(x['auth'])
ctl("exec printf '%s\\n%s\\n' \"$DISPLAY\" \"$XAUTHORITY\" > /tmp/x11-launch-env")
wait(lambda:'ENV_READY'in guest('test -s /tmp/x11-launch-env && echo ENV_READY'))
text=guest('cat /tmp/x11-launch-env');assert x['display'] in text and x['auth'] in text,text
passed.append('Configured exec children receive the compositor DISPLAY and XAUTHORITY')
guest(xenv+' ember-x11-window active >/tmp/x11-window.log 2>&1 &')
w=wait(lambda:next((w for w in state()['windows']if w['x11']and w['placement_ready']),None));assert w['focused']and w['visible'],w
wait(lambda:'take_focus'in guest('cat /tmp/x11-window.log'))
key('w');key('esc');key('a');wait(lambda:'"key":38'in guest('cat /tmp/x11-window.log'))
assert '"key":25'in guest('cat /tmp/x11-window.log') and '"key":9'in guest('cat /tmp/x11-window.log')
# Activation requests must bring an existing, covered X11 window forward.
guest('WAYLAND_DISPLAY='+shlex.quote(state()['wayland_display'])+' ember-wm-client --app-id activation-cover --control /tmp/activation-cover.fifo >/tmp/activation-cover.log 2>&1 &')
wait(lambda:any(w['app_id']=='activation-cover'and w['focused']for w in state()['windows']))
guest(xenv+' ember-x11-window request '+str(w['x11_id']))
wait(lambda:next(w for w in state()['windows']if w['x11'])['focused'])
passed.append('A real _NET_ACTIVE_WINDOW request raises and focuses an existing X11 window over a native cover window')
# Wheel events reach X11 with the configured scale, including without pointer movement after focus changes.
qmp('input-send-event',{'events':[{'type':'rel','data':{'axis':'x','value':200}},{'type':'rel','data':{'axis':'y','value':200}}]});time.sleep(.2)
def wheel():
 for down in [True,False]:qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'wheel-down'}}]})
 time.sleep(.3)
def wheel_count():return guest('cat /tmp/x11-window.log').count('"button":5')
guest("sed -i 's/scroll_factor = 2.0/scroll_factor = 1.0/' /tmp/config.toml");ctl('reload');before=wheel_count();wheel();one=wheel_count()-before
assert one==1,(one,guest('cat /tmp/x11-window.log'))
guest("sed -i 's/scroll_factor = 1.0/scroll_factor = 2.0/' /tmp/config.toml");ctl('reload');before=wheel_count();wheel();two=wheel_count()-before
assert two==2*one,(one,two)
passed.append('One virtual wheel notch generates X11 scroll input; the 2x setting doubles the scroll steps')

# A per-app fractional rule overrides the global X11 scale and accumulates steps.
guest("printf '\n[[window_rules]]\napp_id = \"EmberX11Test\"\nscroll_factor = 1.5\n' >>/tmp/config.toml")
ctl('reload');before=wheel_count();wheel();wheel();fractional=wheel_count()-before
assert fractional==3,(one,two,fractional)
passed.append('A 1.5x app rule overrides global 2x and yields three X11 wheel steps for two notches')

# Close the native cover without leaving it in later clipboard checks.
ctl('focus-next');ctl('close');wait(lambda:all(w['app_id']!='activation-cover'for w in state()['windows']))
key('f11');wait(lambda:next(w for w in state()['windows']if w['x11'])['fullscreen'])
key('f11');wait(lambda:not next(w for w in state()['windows']if w['x11'])['fullscreen'])
passed.append('Native DRM Xwayland performs WM_TAKE_FOCUS for a globally active X11 client, delivers W/Escape/A, renders the window and toggles fullscreen with F11')
g=next(w for w in state()['windows']if w['x11'])['geometry'];px=g['x']+40;py=g['y']+40
ctl(f'screenshot-region {px} {py} 80 60');time.sleep(.25)
text=guest(xenv+' ember-x11-clipboard-probe image /tmp/from-native.rgba');assert '"width":80,"height":60'in text,text
expected=hashlib.sha256(bytes([0x34,0x56,0x78,255])*80*60).hexdigest();assert expected in guest('busybox sha256sum /tmp/from-native.rgba')
passed.append('Native GPU screenshot of the X11 window pastes through real X11 arboard with all 4800 pixels matching exactly')
display=state()['wayland_display'];guest('WAYLAND_DISPLAY='+shlex.quote(display)+' ember-wm-client --app-id native-x11-paste --control /tmp/native-x11.fifo >/tmp/native-x11.log 2>&1 &')
wait(lambda:any(w['app_id']=='native-x11-paste'and w['focused']for w in state()['windows']))
guest('busybox mkfifo /tmp/x11-owner; exec 3<>/tmp/x11-owner; '+xenv+' ember-x11-clipboard-probe set-image 80 60 /tmp/from-native.rgba </tmp/x11-owner >/tmp/x11-owner.log 2>&1 & echo $! >/tmp/x11-owner.pid')
wait(lambda:'ready'in guest('cat /tmp/x11-owner.log'));time.sleep(.2)
guest("printf '%s\\n' 'clipboard /tmp/from-x11.png' >/tmp/native-x11.fifo")
wait(lambda:'PASTE_READY'in guest('test -s /tmp/from-x11.png && echo PASTE_READY'))
# Decode the received PNG on the host and compare every pixel via serial hex.
text=guest('busybox od -An -v -tx1 /tmp/from-x11.png');data=b''.join(bytes.fromhex(line.strip())for line in text.splitlines()if line.strip()and all(c in '0123456789abcdef 'for c in line.strip()))
from PIL import Image
import io
image=Image.open(io.BytesIO(data)).convert('RGBA');assert image.size==(80,60)and image.tobytes()==bytes([0x34,0x56,0x78,255])*80*60
passed.append('An X11 arboard image owner pastes into a focused native Wayland client through a regular-file destination, with exact decoded pixels')
guest('kill $(cat /tmp/x11-owner.pid); exec 3>&-')
ctl('close');wait(lambda:all(w['app_id']!='native-x11-paste'for w in state()['windows']));ctl('close');wait(lambda:not state()['windows'])
report={'passed':passed,'wheel_steps':{'factor_1':one,'factor_2':two,'per_app_1_5_two_notches':fractional},'scope':'Native DRM backend, 512 MiB QEMU fixture, virtual GPU and keyboard. No physical user session or clipboard touched.'}
(ROOT/'docs/xwayland-native-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
