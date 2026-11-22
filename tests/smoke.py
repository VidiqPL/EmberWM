#!/usr/bin/env python3
"""Exercise a real nested compositor and real Wayland terminals, then clean up."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT/'target/release/emberwm'
ARTIFACTS = ROOT/'artifacts'
ARTIFACTS.mkdir(exist_ok=True)


def wait_for(fn, timeout=12):
    end = time.monotonic()+timeout
    while time.monotonic()<end:
        value=fn()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('timed out waiting for compositor state')


def memory(pid):
    data={}
    for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines():
        if ':' in line:
            k,v=line.split(':',1)
            data[k]=int(v.split()[0])
    return {k:data.get(k,0) for k in ['Rss','Pss','Pss_Anon','Pss_File','Private_Dirty']}


with tempfile.TemporaryDirectory(prefix='emberwm-smoke-') as temporary:
    temp=Path(temporary)
    config=temp/'config.toml'
    original=(ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png', str(ROOT/'assets/red-dusk.png'))
    config.write_text(original)
    before=set(Path(os.environ['XDG_RUNTIME_DIR']).glob('emberwm-wayland-*.sock'))
    log=(ARTIFACTS/'smoke.log').open('w')
    proc=subprocess.Popen([str(BIN),'--nested','--config',str(config)],stdout=log,stderr=log)
    server=None
    client_path=Path(os.environ['XDG_RUNTIME_DIR'])/f'embertest-{os.getpid()}.sock'
    client=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM)
    client.bind(str(client_path))
    client.settimeout(3)
    children=set()
    results=[]
    try:
        def discover():
            if proc.poll() is not None:
                raise AssertionError(f'compositor exited: {proc.returncode}; see artifacts/smoke.log')
            paths=set(Path(os.environ['XDG_RUNTIME_DIR']).glob('emberwm-wayland-*.sock'))-before
            return next(iter(paths),None)
        server=wait_for(discover)
        client.connect(str(server))
        def ctl(command, error=False):
            client.send(command.encode())
            data=json.loads(client.recv(262144))
            assert ('error' in data)==error, (command,data)
            return data
        def state(): return ctl('status')
        def windows(n):
            return wait_for(lambda:(s if len(s['windows'])==n and all(w['app_id'] for w in s['windows']) else None) if (s:=state()) else None)
        assert state()['windows']==[]
        time.sleep(1.5)
        idle=memory(proc.pid)
        frames_before=state()["frames_submitted"]
        tick_before=int(Path(f'/proc/{proc.pid}/stat').read_text().split()[13])
        started=time.monotonic()
        time.sleep(2)
        frames_after=state()["frames_submitted"]
        # Host exposure/resize events can legitimately request a frame. Reject a continuous redraw loop.
        assert frames_after-frames_before<=2, (frames_before,frames_after)
        tick_after=int(Path(f'/proc/{proc.pid}/stat').read_text().split()[13])
        idle_cpu=100*(tick_after-tick_before)/os.sysconf('SC_CLK_TCK')/(time.monotonic()-started)
        results.append('empty compositor starts and idles')
        ctl('terminal')
        first=windows(1)['windows'][0]
        assert first['visible'] and not first['floating']
        ctl('draw 200 150 560 360')
        s=wait_for(lambda:(s if len(s['windows'])==2 and s['pending_draws']==0 else None) if (s:=state()) else None)
        draw=next(w for w in s['windows'] if w['floating'])
        assert draw['geometry']=={'x':200,'y':150,'width':560,'height':360}, draw
        assert draw['focused']
        results.append('real terminal maps and drawn terminal receives exact requested rectangle')
        ctl('fullscreen')
        s=state()
        assert sum(w['visible'] for w in s['windows'])==1
        assert next(w for w in s['windows'] if w['focused'])['fullscreen']
        ctl('fullscreen')
        s=state()
        assert next(w for w in s['windows'] if w['floating'])['geometry']==draw['geometry']
        results.append('fullscreen isolates a window and restores floating geometry')
        ctl('send 2')
        assert sum(w['visible'] for w in state()['windows'])==1
        ctl('workspace 2')
        s=state()
        assert s['workspace']==2 and sum(w['visible'] for w in s['windows'])==1
        assert next(w for w in s['windows'] if w['visible'])['app_id']==draw['app_id']
        ctl('workspace 1')
        results.append('workspace switching and sending windows preserve visibility and identity')
        generation=state()['theme_generation']
        config.write_text(original.replace('#cf4763','#35d0a0').replace('gap = 12','gap = 20'))
        ctl('reload')
        assert state()['theme_generation']==generation+1
        assert ctl('config')['theme']['active_border']=='#35d0a0'
        config.write_text('this is not valid toml !')
        ctl('reload',error=True)
        assert state()['theme_generation']==generation+1
        assert ctl('config')['theme']['active_border']=='#35d0a0'
        config.write_text(original)
        ctl('reload')
        results.append('theme/layout reload works; malformed reload keeps working configuration')
        from PIL import Image
        ctl('workspace 3')
        for suffix,mode in [('png','fill'),('jpg','fit'),('jpeg','center')]:
            picture=temp/f'wallpaper.{suffix}'
            Image.new('RGB',(80,40),(30,120,80)).save(picture)
            sample=original.replace(str(ROOT/'assets/red-dusk.png'),str(picture)).replace('wallpaper_mode = "fill"', f'wallpaper_mode = "{mode}"')
            config.write_text(sample)
            ctl('reload')
            capture=temp/f'wallpaper-{suffix}.png'
            ctl(f'screenshot {capture}')
            wait_for(lambda:capture.exists() and capture.stat().st_size>100)
            pixels=Image.open(capture).convert('RGB')
            center=pixels.getpixel((pixels.width//2,pixels.height//2))
            assert max(abs(a-b) for a,b in zip(center,(30,120,80)))<=3, (suffix,center)
            if mode in ['fit','center']:
                assert pixels.getpixel((5,5))==(25,14,20), (mode,pixels.getpixel((5,5)))
        config.write_text(original.replace(str(ROOT/'assets/red-dusk.png'),str(temp/'missing.jpg')))
        ctl('reload',error=True)
        config.write_text(original)
        ctl('reload')
        ctl('workspace 1')
        results.append('PNG/JPG/JPEG wallpapers render natively; fit/center modes and missing-file rejection work')

        ctl('workspace 2')
        shot=ARTIFACTS/'smoke.png'
        if shot.exists(): shot.unlink()
        ctl(f'screenshot {shot}')
        wait_for(lambda:shot.exists() and shot.stat().st_size>100)
        time.sleep(0.2)
        assert proc.poll() is None
        assert state()['windows']
        results.append('PNG screenshot renders without losing the graphics context')
        ctl('draw')
        assert state()['draw_armed']
        ctl('workspace 1')
        assert not state()['draw_armed']
        ctl('draw -1 0 400 300',error=True)
        ctl('workspace 99',error=True)
        ctl('not-a-command',error=True)
        results.append('draw arming/cancellation and invalid IPC requests handled')
        ctl('close')
        windows(1)
        ctl('workspace 2')
        ctl('close')
        windows(0)
        results.append('client close cleans window state and layout')
        after=memory(proc.pid)
        ctl('quit')
        assert proc.wait(timeout=5)==0
        assert not server.exists()
        results.append('clean exit removes IPC socket')
        server=None
        proc=subprocess.Popen([str(BIN),'--nested','--config',str(config)],stdout=log,stderr=log)
        server=wait_for(discover)
        time.sleep(0.2)
        proc.terminate()
        assert proc.wait(timeout=5)==0
        assert not server.exists()
        results.append('SIGTERM also shuts down gracefully and removes the socket')
        report={'passed':results,'idle_memory_kib':idle,'idle_user_cpu_percent':round(idle_cpu,3), 'idle_frames_submitted_in_two_seconds':frames_after-frames_before,
                'memory_after_clients_closed_kib':after,
                'scope':'Nested compositor process only. Host desktop/kernel/services are excluded. Not a whole-system idle measurement.'}
        (ARTIFACTS/'smoke-results.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report,indent=2))
    finally:
        # These clients exist only on this test compositor; shutting it down disconnects them.
        if proc.poll() is None:
            try:
                if server is not None:
                    client.send(b'quit')
                    proc.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                pass
        if proc.poll() is None:
            proc.terminate()
            try: proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill();proc.wait()
        if server is not None and proc.poll() is not None:
            server.unlink(missing_ok=True)
        client.close()
        client_path.unlink(missing_ok=True)
        log.close()
