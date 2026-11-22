#!/usr/bin/env python3
"""Static power samples must reuse pixels; all CPU files are fake inside the guest."""
import json,re,shlex,sys,time
from control import shell
baseline='--baseline'in sys.argv
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
 raise AssertionError('power refresh timed out')
def memory():
 p=ctl('status')['pid'];t=guest('cat /proc/'+str(p)+'/smaps_rollup')
 return {k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon):\s+(\d+)',t,re.M)}
try:
 guest('emberwm ctl quit; sleep 1; mkdir -p /tmp/power-refresh/policy0')
 for name,value in {'scaling_driver':'fixture-epp','scaling_governor':'powersave','energy_performance_preference':'balance_performance','energy_performance_available_preferences':'performance balance_performance power'}.items():
  guest('printf %s '+shlex.quote(value)+' > /tmp/power-refresh/policy0/'+name)
 guest('/bin/busybox mount --bind /tmp/power-refresh /sys/devices/system/cpu/cpufreq')
 config='[panels]\nenabled = false\n[notifications]\nenabled = false\n'
 guest('printf %s '+shlex.quote(config)+' >/tmp/power-refresh.toml')
 binary='/usr/bin/emberwm-audit-baseline'if baseline else'/usr/bin/emberwm'
 guest(binary+' --native --config /tmp/power-refresh.toml >/tmp/power-refresh-server.log 2>&1 &')
 wait(lambda:ctl('status'));time.sleep(.3);idle=memory()
 ctl('power-toggle');wait(lambda:ctl('power')['buffer_bytes']>0 and not ctl('power')['busy'])
 time.sleep(.2);before=ctl('status')['frames_submitted'];raster=ctl('power').get('rasterizations')
 time.sleep(7)
 after=ctl('status')['frames_submitted'];extra=after-before
 assert extra>=2 if baseline else extra==0,extra
 if not baseline:assert ctl('power')['rasterizations']==raster
 open_memory=memory()
 guest('printf %s power >/tmp/power-refresh/policy0/energy_performance_preference')
 wait(lambda:ctl('power')['snapshot']['preference']=='power'and ctl('status')['frames_submitted']>after)
 ctl('menu-toggle');wait(lambda:not ctl('power')['busy']and ctl('power')['buffer_bytes']==0)
 time.sleep(.3);closed=memory()
 assert ctl('menu')['buffer_bytes']==0
 before=ctl('status')['frames_submitted'];time.sleep(3.5);assert ctl('status')['frames_submitted']==before
 print(json.dumps({'baseline':baseline,'redundant_frames_with_static_readings_over_7s':extra,'changed_reading':'redrawn','closed':'no overlay buffers or periodic redraws','idle_PSS_KiB':idle,'open_PSS_KiB':open_memory,'closed_PSS_KiB':closed,'scope':'One native software-GPU VM process, static synthetic CPU files; not whole-system or physical GPU RAM'},indent=2))
finally:
 try:ctl('quit')
 except Exception:pass
 try:guest('/bin/busybox umount /sys/devices/system/cpu/cpufreq')
 except Exception:pass
