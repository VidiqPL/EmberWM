#!/usr/bin/env python3
"""Matched nested-process comparison before/after the 0.5 wallpaper optimization.
Run on a private bus: dbus-run-session -- python3 tests/memory-comparison.py
"""
import json,os,re,socket,statistics,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/dbus-')
runtime=Path(os.environ['XDG_RUNTIME_DIR'])
def mem(pid):
    text=Path(f'/proc/{pid}/smaps_rollup').read_text()
    return {k:int(v) for k,v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|Private_Dirty):\s+(\d+)',text,re.M)}
samples={'before':[],'after':[]}
with tempfile.TemporaryDirectory(prefix='ember-memory-') as directory:
    temp=Path(directory);config=temp/'config.toml'
    config.write_text((ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')))
    for i,variant in enumerate(['before','after','before','after']):
        binary=ROOT/('artifacts/emberwm-0.5-before-memory-review' if variant=='before' else 'target/release/emberwm')
        known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ROOT/f'artifacts/memory-{variant}-{i}.log').open('w')
        proc=subprocess.Popen([str(binary),'--nested','--config',str(config)],stdout=log,stderr=log)
        path=runtime/f'ember-memory-test-{os.getpid()}-{i}.sock';client=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);client.bind(str(path));client.settimeout(4)
        try:
            end=time.monotonic()+10
            while True:
                sockets=set(runtime.glob('emberwm-wayland-*.sock'))-known
                if sockets:break
                assert proc.poll() is None and time.monotonic()<end
                time.sleep(.05)
            client.connect(str(next(iter(sockets))))
            def ctl(command):client.send(command.encode());return json.loads(client.recv(262144))
            time.sleep(1.3);state=ctl('status');assert state['notifications']['server_running']
            m=[mem(proc.pid)]
            for _ in range(2):time.sleep(.15);m.append(mem(proc.pid))
            samples[variant].append({k:int(statistics.median(s[k] for s in m)) for k in m[0]})
            frame=state['frames_submitted'];time.sleep(1);assert ctl('status')['frames_submitted']-frame<=1
        finally:
            if proc.poll() is None:
                try:ctl('quit');proc.wait(timeout=5)
                except Exception:proc.terminate();proc.wait(timeout=5)
            client.close();path.unlink(missing_ok=True);log.close()
medians={variant:{k:int(statistics.median(s[k] for s in rows)) for k in rows[0]}for variant,rows in samples.items()}
report={'samples_kib':samples,'median_kib':medians,'anonymous_saving_kib':medians['before']['Pss_Anon']-medians['after']['Pss_Anon'],'pss_saving_kib':medians['before']['Pss']-medians['after']['Pss'],'scope':'Matched 1280x800 nested compositor processes, same 0.5 config/features/private bus, two cold runs per binary with three smaps samples each. Before binary retains CPU wallpaper pixels; after keeps the renderer texture only and reclaims freed heap pages after image/UI changes. Excludes host desktop/kernel/services and GPU memory outside process accounting.'}
assert report['anonymous_saving_kib']>3*1024,report
(ROOT/'docs/memory-optimization-0.5.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
