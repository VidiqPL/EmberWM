#!/usr/bin/env python3
"""Window rules and client fullscreen requests with real native Wayland clients.
Run after tests/vm/build.py builds the small C protocol client.
"""
import json,os,socket,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BIN=ROOT/'target/release/emberwm';HELPER=ROOT/'artifacts/native-vm/wm-client';ART=ROOT/'artifacts';ART.mkdir(exist_ok=True)
def wait(fn,timeout=10):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.035)
    raise AssertionError('rule/fullscreen condition timed out')
RULES='''
[[window_rules]]
name = "Background floating"
app_id = "rule-a"
workspace = 10
width = 640
height = 360
x = 90
y = 90

[[window_rules]]
app_id = "rule-a"
title = "*Dialog*"
workspace = 2
width = 420
height = 240
center = true

[[window_rules]]
app_id = "follow-app"
workspace = 4
follow = true
fullscreen = true

[[window_rules]]
app_id = "ember-terminal-*"
workspace = 9
width = 300
height = 200
fullscreen = true
'''
with tempfile.TemporaryDirectory(prefix='ember-rules-') as directory:
    temp=Path(directory);config=temp/'config.toml';original=(ROOT/'config/ember.toml').read_text().replace('../assets/abbey-oakwood.jpg',str(ROOT/'assets/abbey-oakwood.jpg')).replace('../assets/red-dusk.png',str(ROOT/'assets/red-dusk.png'))
    start=original.index('terminal = [');end=original.index('\n]',start)+2
    original=original[:start]+'terminal = '+json.dumps([str(HELPER),'--app-id','{app_id}'])+original[end:]
    text=original+RULES;config.write_text(text)
    test_env=os.environ.copy();host_runtime=Path(test_env['XDG_RUNTIME_DIR']);host_display=test_env.get('WAYLAND_DISPLAY','wayland-0');runtime=temp/'run';runtime.mkdir(mode=0o700);test_env['XDG_RUNTIME_DIR']=str(runtime);test_env['WAYLAND_DISPLAY']=host_display if host_display.startswith('/')else str(host_runtime/host_display)
    known=set(runtime.glob('emberwm-wayland-*.sock'));log=(ART/'rules.log').open('w');proc=subprocess.Popen([str(BIN),'--nested','--config',str(config)],env=test_env,stdout=log,stderr=log)
    path=runtime/f'ember-rules-test-{os.getpid()}.sock';client=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);client.bind(str(path));client.settimeout(4)
    clients=[];passed=[]
    try:
        server=wait(lambda:next(iter(set(runtime.glob('emberwm-wayland-*.sock'))-known),None));client.connect(str(server))
        def ctl(command,error=False):client.send(command.encode());s=json.loads(client.recv(262144));assert bool(s.get('error'))==error,(command,s);return s
        def state():return ctl('status')
        def win(app):return next((w for w in state()['windows'] if w['app_id']==app),None)
        def spawn(app,title='Normal'):
            control=temp/f'{app}.fifo';out=temp/f'{app}.log';handle=out.open('w');env=test_env.copy();env['WAYLAND_DISPLAY']=state()['wayland_display'];env.pop('DISPLAY',None)
            child=subprocess.Popen([str(HELPER),'--app-id',app,'--title',title,'--control',str(control)],stdout=handle,stderr=handle,env=env);clients.append((child,handle))
            wait(lambda:control.exists() and (w:=win(app)) and w['placement_ready'] and out.stat().st_size)
            return control,out
        def command(control,cmd):
            with control.open('w') as f:f.write(cmd+'\n')
        def events(out):return [json.loads(line)for line in out.read_text().splitlines() if line.startswith('{')]
        assert state()['workspace_count']==10
        for n in range(1,11):ctl(f'workspace {n}');assert state()['workspace']==n
        ctl('workspace 11',True);assert state()['workspace']==10;ctl('workspace 1')
        passed.append('all ten workspaces switch and out-of-range workspace requests are rejected')
        b,bout=spawn('ordinary');ctl('fullscreen');wait(lambda:win('ordinary')['fullscreen'])
        a,aout=spawn('rule-a')
        assert state()['workspace']==1 and win('ordinary')['focused'] and win('ordinary')['fullscreen']
        w=win('rule-a');assert w['workspace']==10 and not w['visible'] and w['floating'] and w['geometry']=={'x':90,'y':90,'width':640,'height':360},w
        wait(lambda:events(aout)[-1]['width']==640 and events(aout)[-1]['height']==360)
        assert events(aout)[0]['width']==640 and events(aout)[0]['height']==360
        passed.append('background rules configure hidden clients without stealing focus or breaking current fullscreen')
        ctl('workspace 10');normal=win('rule-a')['geometry']
        command(a,'fullscreen');wait(lambda:win('rule-a')['fullscreen'] and events(aout)[-1]['fullscreen'])
        size=state()['output'];assert win('rule-a')['geometry']=={'x':0,'y':0,'width':size['width'],'height':size['height']}
        assert not state()['panels']['items']
        command(a,'fullscreen');time.sleep(.12);command(a,'unfullscreen')
        wait(lambda:not win('rule-a')['fullscreen'] and not events(aout)[-1]['fullscreen'])
        assert win('rule-a')['geometry']==normal
        passed.append('real xdg fullscreen/unfullscreen configures clients and restores exact floating geometry, including repeated requests')
        ctl('workspace 1');command(a,'fullscreen');wait(lambda:win('rule-a')['fullscreen'] and events(aout)[-1]['fullscreen'])
        assert state()['workspace']==1 and win('ordinary')['focused'] and not win('rule-a')['visible']
        command(a,'unfullscreen');wait(lambda:not win('rule-a')['fullscreen'])
        passed.append('background client fullscreen requests receive configure state without switching workspaces')
        ctl('workspace 10');command(a,'title Settings Dialog');wait(lambda:win('rule-a')['workspace']==2)
        w=win('rule-a');size=state()['output'];top=max((p['y']+p['height'] for p in ctl('panels')['items']),default=0);assert w['geometry']=={'x':(size['width']-420)//2,'y':top+(size['height']-top-240)//2,'width':420,'height':240} and w['rules_applied']==2,w
        ctl('workspace 2');ctl('send 3');command(a,'title Another Dialog');time.sleep(.15);assert win('rule-a')['workspace']==3
        ctl('rules-apply');wait(lambda:win('rule-a')['workspace']==2)
        passed.append('late title rules combine in order, match once, preserve later manual moves and can be explicitly reapplied')
        f,fout=spawn('follow-app');wait(lambda:state()['workspace']==4 and win('follow-app')['fullscreen'])
        assert win('follow-app')['focused']
        changed=text.replace('workspace = 4\nfollow = true\nfullscreen = true','workspace = 5\nfollow = true\nfullscreen = false\nwidth = 500\nheight = 300\ncenter = true')
        config.write_text(changed);ctl('reload');wait(lambda:state()['workspace']==5 and not win('follow-app')['fullscreen'])
        assert win('follow-app')['geometry']['width']==500 and win('follow-app')['floating'] and win('follow-app')['focused']
        ctl('send 6');config.write_text(changed.replace('#cf4763','#d54c68'));ctl('reload');assert win('follow-app')['workspace']==6
        passed.append('follow/fullscreen rules work on launch; changed rules update live windows while unrelated reloads preserve manual placement')
        generation=state()['theme_generation'];config.write_text(changed+'\n[[window_rules]]\napp_id="bad"\nworkspace=11\n');ctl('reload',True);assert state()['theme_generation']==generation
        passed.append('invalid rules reject reload and preserve the active configuration')
        config.write_text(changed);ctl('workspace 1');ctl('draw 200 150 560 360')
        w=wait(lambda:next((w for w in state()['windows'] if w['drawn']),None))
        assert w['workspace']==1 and w['visible'] and w['focused'] and not w['fullscreen'] and w['geometry']=={'x':200,'y':150,'width':560,'height':360},w
        ctl('rules-apply');assert next(w for w in state()['windows'] if w['drawn'])['geometry']=={'x':200,'y':150,'width':560,'height':360}
        ctl('workspace 1');ctl('close');wait(lambda:not any(w['drawn']for w in state()['windows']))
        passed.append('drawn terminals retain explicit geometry, workspace and focus even when general rules match')
        config.write_text(original);ctl('reload');ctl('workspace 10')
        time.sleep(.2);baseline=state()['frames_submitted'];time.sleep(1.5);assert state()['frames_submitted']-baseline<=1
        report={'passed':passed,'scope':'Real nested compositor and dedicated Wayland protocol clients, with the installed Sober app ID verified separately. Sober gameplay itself was not launched.'}
        (ROOT/'docs/rules-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    finally:
        for child,handle in clients:
            if child.poll() is None:child.terminate();child.wait(timeout=5)
            handle.close()
        if proc.poll() is None:
            try:ctl('quit');proc.wait(timeout=5)
            except Exception:proc.terminate();proc.wait(timeout=5)
        client.close();path.unlink(missing_ok=True);log.close()
