#!/usr/bin/env python3
"""Real native selection gesture, PNG clipboard paste and replacement by text."""
import json,shlex,time
from pathlib import Path
from PIL import Image,ImageChops
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2];passed=[]
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
    text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text
    d=json.JSONDecoder().raw_decode(text[start:])[0];assert not d.get('error'),d;return d
def state():return ctl('status')
def wait(fn,timeout=10):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.05)
    raise AssertionError('screenshot/clipboard operation timed out')
def key(*keys):
    for down,seq in [(True,keys),(False,tuple(reversed(keys)))]:
        assert 'error' not in qmp('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':k}}}for k in seq]});time.sleep(.12)
def button(down):
    assert 'error' not in qmp('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});time.sleep(.06)
def move(x,y):
    for _ in range(35):
        p=state()['pointer'];dx=x-p['x'];dy=y-p['y']
        if abs(dx)<2 and abs(dy)<2:return p
        events=[{'type':'rel','data':{'axis':axis,'value':int(v/2)or(1 if v>0 else -1)}}for axis,v in [('x',dx),('y',dy)]if abs(v)>=1]
        assert 'error' not in qmp('input-send-event',{'events':events});time.sleep(.035)
    raise AssertionError('mouse did not converge')
def command(cmd):guest('printf \'%s\\n\' '+shlex.quote(cmd)+' >/tmp/clipboard-test.fifo')
def copy_file(path):
    command('clipboard '+path);time.sleep(.3)
    # The fixture root is readonly; transfer a screenshot through serial as hex.
    text=guest('/bin/busybox od -An -v -tx1 '+shlex.quote(path))
    data=b''.join(bytes.fromhex(line.strip()) for line in text.splitlines() if line.strip() and all(c in '0123456789abcdef 'for c in line.strip()))
    return data
try:
    guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/config.toml; emberwm --native --config /tmp/config.toml >/tmp/ember-capture.log 2>&1 &')
    time.sleep(1.5);s=state();display=s['wayland_display']
    guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id clipboard-test --control /tmp/clipboard-test.fifo >/tmp/clipboard-client.log 2>&1 &')
    wait(lambda:len(state()['windows'])==1 and state()['windows'][0]['placement_ready'])
    ctl('fullscreen');wait(lambda:state()['windows'][0]['fullscreen']);time.sleep(.2)
    a=move(620,430);key('meta_l','shift','s');assert state()['screenshot_armed']
    button(True);b=move(220,170);button(False)
    wait(lambda:state()['clipboard']['bytes']>0)
    assert not state()['draw_armed'] and len(state()['windows'])==1
    assert state()['clipboard']['allocated_bytes']==state()['clipboard']['bytes']
    png=copy_file('/tmp/region.png');target=ROOT/'docs/screenshot-region-test.png';target.write_bytes(png)
    im=Image.open(target).convert('RGB');roundp=lambda v:int(v+.5)
    expected=(abs(roundp(a['x'])-roundp(b['x'])),abs(roundp(a['y'])-roundp(b['y'])))
    assert im.size==expected,(im.size,expected)
    assert im.getextrema()==((39,39),(24,24),(36,36)),im.getextrema()
    print("Native screenshot case passed", flush=True)
    passed.append('real Super+Shift+S and reversed mouse drag copy an exact PNG region; border and cursor are excluded')
    count=state()['clipboard']['bytes'];key('meta_l','shift','s');key('esc');assert not state()['draw_armed'] and state()['clipboard']['bytes']==count
    print("Native screenshot case passed", flush=True)
    passed.append('Escape cancels without replacing the clipboard')
    for _ in range(8):ctl('screenshot-region 100 100 120 80');wait(lambda:not state()['clipboard']['error'])
    time.sleep(.3);assert state()['clipboard']['active_transfers']==0
    png=copy_file('/tmp/repeated.png');target2=WORK/'repeated-region.png';target2.write_bytes(png);assert Image.open(target2).size==(120,80)
    print("Native screenshot case passed", flush=True)
    passed.append('repeated clipboard screenshots remain pasteable and do not leave transfer workers idle')
    command('pattern');time.sleep(.3)
    def assert_region(image,x,y,width,height):
        assert image.size==(width,height)
        expected=Image.new('RGB',(width,height))
        expected.putdata([(xx%251,yy%251,48 if yy<540 else 208)for yy in range(y,y+height)for xx in range(x,x+width)])
        assert ImageChops.difference(image.convert('RGB'),expected).getbbox() is None, ('screenshot orientation/crop mismatch',x,y,image.getpixel((0,0)),expected.getpixel((0,0)))
    for label,x,y in [('top',100,120),('bottom',900,700)]:
        ctl(f'screenshot-region {x} {y} 320 240');time.sleep(.3)
        data=copy_file(f'/tmp/{label}-pattern.png');path=ROOT/f'docs/screenshot-{label}-pattern.png';path.write_bytes(data)
        assert_region(Image.open(path),x,y,320,240)
    print("Native screenshot case passed", flush=True)
    passed.append('asymmetric top and bottom regions match every pixel in screen coordinates')
    ctl('screenshot /tmp/orientation-full.png');time.sleep(.4)
    text=guest('/bin/busybox od -An -v -tx1 /tmp/orientation-full.png')
    data=b''.join(bytes.fromhex(line.strip()) for line in text.splitlines() if line.strip() and all(c in '0123456789abcdef 'for c in line.strip()))
    path=WORK/'orientation-full.png';path.write_bytes(data);full=Image.open(path).convert('RGB')
    for x,y in [(50,50),(80,950),(1100,800)]:assert full.getpixel((x,y))==(x%251,y%251,48 if y<540 else 208),(x,y,full.getpixel((x,y)))
    print("Native screenshot case passed", flush=True)
    passed.append('full-frame file screenshots preserve asymmetric top/bottom orientation too')
    ctl('workspace 2');ctl('workspace 1');time.sleep(.2);command('text replaced by normal copy');wait(lambda:state()['clipboard']['bytes']==0)
    print("Native screenshot case passed", flush=True)
    passed.append('a normal client text copy replaces the image and releases Ember clipboard ownership')
    ctl('screenshot '+str('/tmp/full-after.png'));time.sleep(.2)
    assert not state()['clipboard']['error']
    report={'passed':passed,'png_size':list(im.size),'scope':'Private native VM with virtual keyboard/mouse and a real wl_data_device client receiving image/png. No host clipboard or user desktop contents read.'}
    (ROOT/'docs/screenshot-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
    try:ctl('quit')
    except Exception:pass
