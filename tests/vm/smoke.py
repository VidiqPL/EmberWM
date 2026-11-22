#!/usr/bin/env python3
"""Test the native backend using QEMU virtual hardware; leave the host desktop alone."""
import json,re,time
from pathlib import Path
from PIL import Image
from control import shell,qmp,WORK
ROOT=Path(__file__).resolve().parents[2]
passed=[]
def guest(command):return shell(command).replace('\r','')
def status():
    text=guest('emberwm ctl status')
    start=text.find('{\n')
    assert start>=0,text
    return json.JSONDecoder().raw_decode(text[start:])[0]
def ctl(command):
    text=guest('emberwm ctl '+command)
    assert '"error"' not in text,text
    return text
def key(*keys):
    result=qmp('send-key',{'keys':[{'type':'qcode','data':key} for key in keys],'hold-time':100})
    assert 'error' not in result,result
    time.sleep(.2)
def mouse(events):
    result=qmp('input-send-event',{'events':events})
    assert 'error' not in result,result
    time.sleep(.12)
def wait(fn,timeout=8):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.1)
    raise AssertionError('timed out waiting for native compositor')
def process_memory():
    text=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/smaps_rollup')
    return {k:int(v) for k,v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|Private_Dirty):\s+(\d+)',text,re.M)}
def system_memory():
    text=guest('cat /proc/meminfo')
    return {k:int(v) for k,v in re.findall(r'^(MemTotal|MemAvailable|MemFree|Cached|SReclaimable|SUnreclaim|Shmem|KernelStack|PageTables):\s+(\d+)',text,re.M)}
def screenshot(name):
    ppm=WORK/f'{name}.ppm';png=WORK/f'{name}.png'
    assert 'error' not in qmp('screendump',{'filename':str(ppm)})
    im=Image.open(ppm).convert('RGB');im.save(png)
    return im
guest('if /bin/busybox pidof emberwm >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/config.toml; emberwm --native --config /tmp/config.toml > /tmp/ember.log 2>&1 &')
time.sleep(1)
s=status()
assert abs(s['output']['refresh_hz']-144.0)<0.1,s
assert s['backend']=='native' and s['session_active'] and not s['windows'],s
passed.append('native DRM/KMS boot on virtual GPU, libseat session and libinput devices')
time.sleep(1)
frames=status()['frames_submitted'];mem=process_memory();system=system_memory()
start=time.monotonic()
stat1=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/stat')
time.sleep(3)
stat2=guest('p=$(/bin/busybox pidof emberwm); cat /proc/$p/stat')
def ticks(text):
    line=next(line for line in text.splitlines() if re.match(r'^\d+ \(emberwm\)',line))
    parts=line.split();return int(parts[13])+int(parts[14])
cpu=(ticks(stat2)-ticks(stat1))/(time.monotonic()-start)
idle_frames=status()['frames_submitted']-frames
assert idle_frames<=1, idle_frames  # minute clock may tick once
assert mem['Pss']<128*1024,mem
used=system['MemTotal']-system['MemAvailable']
assert used<256*1024,system
passed.append('empty native session stays idle without new frames and fits process/guest RAM budgets')
def benchmark():
    display=status()['wayland_display']
    text=guest(f'WAYLAND_DISPLAY={display} ember-frame-client')
    start=text.find('{"frames"')
    assert start>=0,text
    return json.JSONDecoder().raw_decode(text[start:])[0]
rate144=benchmark();assert 125.0<rate144['fps']<155.0,rate144
guest("sed -i 's/refresh_hz = 144.0/refresh_hz = 60.0/' /tmp/config.toml");ctl('reload')
wait(lambda:abs(status()['output']['refresh_hz']-60.0)<0.1)
rate60=benchmark();assert 50.0<rate60['fps']<70.0,rate60
guest("sed -i 's/refresh_hz = 60.0/refresh_hz = 144.0/' /tmp/config.toml");ctl('reload')
wait(lambda:abs(status()['output']['refresh_hz']-144.0)<0.1)
passed.append('144 Hz native frame callbacks exceed 125 FPS; live rate reload switches to 60 Hz and back')

key('meta_l','d')
wait(lambda:status()['draw_armed'])
mouse([{'type':'rel','data':{'axis':'x','value':100}},{'type':'rel','data':{'axis':'y','value':100}}])
mouse([{'type':'btn','data':{'down':True,'button':'left'}}])
mouse([{'type':'rel','data':{'axis':'x','value':400}},{'type':'rel','data':{'axis':'y','value':250}}])
mouse([{'type':'btn','data':{'down':False,'button':'left'}}])
s=wait(lambda:(s if len(s['windows'])==1 and s['pending_draws']==0 else None) if (s:=status()) else None)
assert s['windows'][0]['floating'] and s['windows'][0]['geometry']['width']>80
passed.append('physical key + mouse events through libinput draw and launch a real floating terminal')
key('meta_l','ret')
s=wait(lambda:(s if len(s['windows'])==2 else None) if (s:=status()) else None)
assert any(not w['floating'] for w in s['windows'])
passed.append('Super+Enter launches a real Wayland terminal through the native keyboard path')
ctl('send 2');ctl('workspace 2');s=status()
assert s['workspace']==2 and sum(w['visible'] for w in s['windows'])==1
ctl('fullscreen');assert next(w for w in status()['windows'] if w['visible'])['fullscreen']
ctl('fullscreen');ctl('workspace 1')
passed.append('native workspaces, sending windows and fullscreen restoration')
key('ctrl','alt','f2')
wait(lambda:not status()['session_active'])
key('ctrl','alt','f1')
wait(lambda:status()['session_active'])
ctl('draw')
wait(lambda:status()['draw_armed'])
key('esc');wait(lambda:not status()['draw_armed'])
passed.append('Ctrl+Alt+F2/F1 pauses and resumes GPU/input ownership; keyboard works afterward')
# Close only this guest's test clients before wallpaper pixel checks.
ctl('close');ctl('workspace 2');ctl('close')
wait(lambda:not status()['windows'])
ctl('workspace 1')
for suffix,mode in [('png','fill'),('jpg','fit'),('jpeg','center')]:
    picture=ROOT/'artifacts/native-vm/root/assets'/f'fixture.{suffix}'
    Image.new('RGB',(80,40),(30,120,80)).save(picture)
    guest(f'''sed -i 's|wallpaper = .*|wallpaper = "/assets/fixture.{suffix}"|; s|wallpaper_mode = .*|wallpaper_mode = "{mode}"|' /tmp/config.toml''')
    ctl('reload');time.sleep(.3)
    image=screenshot('wallpaper-'+suffix)
    center=image.getpixel((image.width//2,image.height//2))
    assert max(abs(a-b) for a,b in zip(center,(30,120,80)))<=3,(suffix,center)
    if mode!='fill':assert image.getpixel((5,5))==(25,14,20),(suffix,image.getpixel((5,5)))
passed.append('PNG/JPG/JPEG files render directly on KMS scanout; fit/center modes pass pixel checks')
guest('cp /etc/emberwm/config.toml /tmp/config.toml');ctl('reload');time.sleep(.3)
before=process_memory()
for _ in range(30):ctl('reload')
after=process_memory()
assert after['Pss_Anon']-before['Pss_Anon']<10*1024,(before,after)
passed.append('30 repeated config/theme reloads stay within a 10 MiB anonymous-memory growth limit')
preview=screenshot('native-preview')
ctl('draw 200 150 560 360');wait(lambda:len(status()['windows'])==1 and status()['pending_draws']==0)
time.sleep(.3)
guest("sed -i 's/active_border = .*#cf4763.*/active_border = \"#35d0a0\"/' /tmp/config.toml")
ctl('reload');time.sleep(.3)
image=screenshot('native-border-green')
assert image.getpixel((220,148))==(53,208,160), image.getpixel((220,148))
passed.append('active border colour hot reload passes an actual native scanout pixel check')
guest('cp /etc/emberwm/config.toml /tmp/config.toml');ctl('reload');time.sleep(.3)
screenshot('native-terminal')
ctl('quit')
time.sleep(.3)
text=guest('/bin/busybox pidof emberwm; ls /run/user/0/emberwm-*.sock 2>/dev/null; echo native_session_stopped')
assert not re.search(r'^\d+\s*$',text,re.M),text
passed.append('native logout terminates cleanly and releases the session')
report={'passed':passed,'native_process_memory_kib':mem,'native_guest_memory_kib':system,
        'guest_available_based_used_kib':used,'idle_cpu_percent':round(cpu,3),'idle_frames_in_three_seconds':idle_frames,'frame_benchmark_144hz':rate144,'frame_benchmark_60hz':rate60,
        'memory_before_reload_loop_kib':before,'memory_after_reload_loop_kib':after,
        'scope':'Isolated 512 MiB QEMU VM, same Gentoo kernel and host libraries, virtio GPU, software GLES, minimal test userspace. Not a measurement of the physical Gentoo login session.'}
(WORK/'results.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
