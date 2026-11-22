#!/usr/bin/env python3
"""Nested popup/idle/reload tests with fake scan results. No host network changes.
Run: dbus-run-session -- python3 tests/wifi-smoke.py
"""
import json,os,re,shutil,socket,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BIN=ROOT/'target/release/emberwm';ART=ROOT/'artifacts/wifi';ART.mkdir(parents=True,exist_ok=True)
assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:path=/tmp/dbus-'), 'use dbus-run-session'
def wait(fn,timeout=15):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        result=fn()
        if result:return result
        time.sleep(.03)
    raise AssertionError('Wi-Fi popup timed out')
def memory(pid):
    data=Path(f'/proc/{pid}/smaps_rollup').read_text()
    return {k:int(v) for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File):\s+(\d+)',data,re.M)}
with tempfile.TemporaryDirectory(prefix='ember-wifi-ui-') as directory:
    tmp=Path(directory);shutil.copy2(ROOT/'tests/wifi-fixture-nmcli.sh',tmp/'nmcli');(tmp/'nmcli').chmod(0o755)
    config=tmp/'config.toml';original=(ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png')).replace('[panels]\nenabled = true','[panels]\nenabled = false');config.write_text(original)
    env=os.environ.copy();env.update(PATH=str(tmp)+':'+env['PATH'],DBUS_SYSTEM_BUS_ADDRESS=env['DBUS_SESSION_BUS_ADDRESS'])
    runtime=Path(env['XDG_RUNTIME_DIR']);before=set(runtime.glob('emberwm-wayland-*.sock'))
    log=(ART/'popup.log').open('w');proc=subprocess.Popen([str(BIN),'--nested','--config',str(config)],env=env,stdout=log,stderr=log)
    path=runtime/f'ember-wifi-test-{os.getpid()}.sock';client=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);client.bind(str(path));client.settimeout(4)
    passed=[]
    try:
        server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-before),None));client.connect(str(server))
        def ctl(command):
            client.send(command.encode());result=json.loads(client.recv(262144));assert not result.get('error'),result;return result
        def wifi():return ctl('wifi')
        def ready():
            s=wifi();return s if s['open'] and not s['busy'] and s['view'] else None
        baseline_idle=memory(proc.pid);assert not wifi()['open'] and not wifi()['busy']
        ctl('wifi-toggle');s=wait(ready);assert len(s['networks'])==2 and s['networks'][1]['ssid']=='Cafe: guest'
        raster=s['rasterizations'];time.sleep(.4);assert wifi()['rasterizations']==raster
        shot=ROOT/'docs/wifi-preview.png';ctl('screenshot '+str(shot));wait(lambda:shot.exists())
        passed.append('Real nested popup renders parsed networks and caches unchanged pixels')
        ctl('shortcuts-toggle');wait(lambda:not wifi()['open']);rows=wait(lambda:ctl('status')['ui']['rows']);assert any(r['keys']=='Alt+.' and r['action']=='wifi-toggle' for r in rows)
        ctl('wifi-toggle');wait(ready);assert not ctl('status')['ui']['shortcuts_open']
        changed=original.replace('width = 540','width = 420').replace('key = "period"','key = "comma"');config.write_text(changed);ctl('reload');wait(lambda:wifi()['view']['width']==420)
        config.write_text(changed.replace('[wifi]\nenabled = true','[wifi]\nenabled = false'));ctl('reload');wait(lambda:not wifi()['open']);ctl('wifi-toggle');assert not wifi()['open']
        passed.append('Help/popup exclusion, live styling reload and disabling Wi-Fi controls work')
        config.write_text(original);ctl('reload')
        for _ in range(3):ctl('wifi-toggle');wait(ready);ctl('wifi-toggle');wait(lambda:not wifi()['busy'])
        baseline=memory(proc.pid)
        for _ in range(30):
            ctl('wifi-toggle');wait(ready);ctl('wifi-toggle');wait(lambda:not wifi()['busy'])
        time.sleep(.3);after=memory(proc.pid);s=wifi()
        assert s['buffer_bytes']==0 and not s['font_loaded'] and not s['networks'] and not s['password_entered']
        assert after['Pss_Anon']-baseline['Pss_Anon']<4096,(baseline,after)
        assert 'ember-wifi' not in ''.join(p.read_text() for p in Path(f'/proc/{proc.pid}/task').glob('*/comm'))
        frames=ctl('status')['frames_submitted'];time.sleep(.5);assert ctl('status')['frames_submitted']==frames
        passed.append('30 open/close cycles release buffers/fonts/network list and leave no Wi-Fi worker or idle redraws')
        result={'passed':passed,'idle_before_first_open_kib':baseline_idle,'before_cycles_kib':baseline,'after_cycles_kib':after,'anonymous_growth_kib':after['Pss_Anon']-baseline['Pss_Anon'],'scope':'Nested Ember on private session bus with fake nmcli; no host network mutation'}
        (ROOT/'docs/wifi-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        if proc.poll() is None:
            try:ctl('quit');proc.wait(timeout=5)
            except Exception:proc.terminate();proc.wait(timeout=5)
        client.close();path.unlink(missing_ok=True);log.close()
