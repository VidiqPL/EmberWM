#!/usr/bin/env python3
"""Matched native before/after idle PSS and screenshot RSS peaks, virtual hardware only."""
import json,re,shlex,statistics,time
from pathlib import Path
from control import shell
ROOT=Path(__file__).resolve().parents[2];GUEST=ROOT/'artifacts/native-vm/root'
def guest(cmd):return shell(cmd).replace('\r','')
def ctl(cmd):
 text=guest('emberwm ctl '+shlex.quote(cmd));start=text.find('{\n');assert start>=0,text;data=json.JSONDecoder().raw_decode(text[start:])[0];assert not data.get('error'),data;return data
def wait(fn,timeout=15):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except (AssertionError,KeyError):pass
  time.sleep(.07)
 raise AssertionError('native memory benchmark timed out')
def stop():
 try:ctl('quit');time.sleep(.4)
 except AssertionError:pass
def start(binary,version,config):
 stop();guest(f'{binary} --native --config {config} >/tmp/memory-audit-server.log 2>&1 &');return wait(lambda:ctl('status')if ctl('status')['version']==version else None)
def sample(pid):
 text=guest(f'cat /proc/{pid}/smaps_rollup; cat /proc/meminfo')
 values={k:int(v)for k,v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|MemTotal|MemAvailable):\s+(\d+)',text,re.M)}
 values['guest_used_available']=values['MemTotal']-values['MemAvailable'];return values
def measure(binary,version,config):
 state=start(binary,version,config);pid=state['pid'];time.sleep(3.2)
 results=[]
 for _ in range(5):
  if version=='0.11.1':wait(lambda:not ctl('panels')['font_loaded'])
  results.append(sample(pid));time.sleep(.15)
 panels=ctl('panels');assert panels['items']or'minimal'in config
 state=ctl('status');assert not state['windows']and state['audio']['state']=='disabled'
 result={k:int(statistics.median(row[k]for row in results))for k in results[0]};result.update(panel_font_loaded=panels['font_loaded'],panel_bytes=panels['buffer_bytes'],panel_count=len(panels['items']),refresh_hz=state['output']['refresh_hz']);stop();return result
base=(GUEST/'etc/emberwm/baseline-0.8.toml').read_text()
(GUEST/'etc/emberwm/memory-capture.toml').write_text(base.replace('do_not_disturb = false','do_not_disturb = true'))
def peak(binary,version):
 state=start(binary,version,'/etc/emberwm/memory-capture.toml');display=state['wayland_display'];pid=state['pid']
 guest(f'WAYLAND_DISPLAY={display} ember-wm-client --app-id memory-pattern --control /tmp/memory-pattern >/tmp/memory-pattern.log 2>&1 &');wait(lambda:len(ctl('status')['windows'])==1 and ctl('status')['windows'][0]['placement_ready']);ctl('fullscreen');guest("printf '%s\\n' pattern >/tmp/memory-pattern");time.sleep(2.5)
 values=[]
 for _ in range(3):
  guest(f'ember-rss-watch {pid} 1.5 >/tmp/rss-watch-result &');time.sleep(.08);ctl('screenshot-region 100 120 320 240');time.sleep(1.6)
  text=guest('cat /tmp/rss-watch-result');line=next(l for l in text.splitlines()if l.startswith('{"rss_start_kib"'));values.append(json.loads(line));assert ctl('status')['clipboard']['bytes']>0
 stop();return values
def main():
 try:
  old='/usr/bin/ember-baseline-0.11.0';new='/usr/bin/emberwm';config='/etc/emberwm/baseline-0.8.toml'
  # Alternate both binaries to expose warm/cold driver/cache differences rather than hiding them.
  runs={'baseline':[],'optimized':[]}
  for label,binary,version in [('baseline',old,'0.11.0'),('optimized',new,'0.11.1'),('optimized',new,'0.11.1'),('baseline',old,'0.11.0')]:runs[label].append(measure(binary,version,config))
  matched={label:{k:(int(statistics.median(v[k]for v in values))if type(values[0][k])is int else values[0][k])for k in values[0]}for label,values in runs.items()}
  lean=measure(new,'0.11.1','/etc/emberwm/minimal-rice-test.toml')
  before=peak(old,'0.11.0');after=peak(new,'0.11.1')
  report={'runs_kib':runs,'matched_medians_kib':matched,'minimal_rice_kib':lean,'idle_pss_saved_kib':matched['baseline']['Pss']-matched['optimized']['Pss'],'idle_anonymous_saved_kib':matched['baseline']['Pss_Anon']-matched['optimized']['Pss_Anon'],'region_capture_peak_samples':{'baseline':before,'optimized':after},'region_capture_median_peak_growth_saved_kib':int(statistics.median(v['rss_peak_delta_kib']for v in before)-statistics.median(v['rss_peak_delta_kib']for v in after)),'scope':'Same 512 MiB native VM, 1920x1080/143.981 Hz, matched wallpaper/panels, no windows for idle, audio disabled. ABBA process order, five settled samples per run; synthetic fullscreen pattern for screenshot peaks. PSS compositor-only, guest_used_available whole guest; physical Gentoo/audio/portal helper cost excluded.'}
  assert not matched['optimized']['panel_font_loaded']and matched['optimized']['panel_bytes']==matched['baseline']['panel_bytes']
  assert report['idle_anonymous_saved_kib']>1024,report
  (ROOT/'docs/memory-audit-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
 finally:stop()

if __name__=='__main__':main()
