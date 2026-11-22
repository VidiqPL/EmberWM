#!/usr/bin/env python3
"""Comparable idle PSS and whole-guest RAM, with no client windows or audio stack."""
import json,re,shlex,statistics,time
from pathlib import Path
from control import shell
ROOT=Path(__file__).resolve().parents[2]
def guest(command):return shell(command).replace('\r','')
def ctl(command):
 text=guest('emberwm ctl '+shlex.quote(command));start=text.find('{\n');assert start>=0,text;data=json.JSONDecoder().raw_decode(text[start:])[0];assert not data.get('error'),data;return data
def wait(fn,timeout=12):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except (AssertionError,KeyError):pass
  time.sleep(.1)
 raise AssertionError('idle memory setup timed out')
def measure(binary,version,config):
 try:ctl('quit');time.sleep(.5)
 except AssertionError:pass
 guest(shlex.quote(binary)+' --native --config '+shlex.quote(config)+' >/tmp/memory-'+version+'.log 2>&1 &')
 state=wait(lambda:(s if s['version']==version else None)if(s:=ctl('status'))else None);assert not state['windows'];pid=state['pid'];time.sleep(3)
 samples=[]
 for _ in range(5):
  data=guest(f'cat /proc/{pid}/smaps_rollup; cat /proc/meminfo')
  values={k:int(v)for k,v in re.findall(r'^(Pss|Pss_Anon|Pss_File|MemTotal|MemAvailable|SwapTotal|SwapFree):\s+(\d+)',data,re.M)}
  values['guest_used_available']=values['MemTotal']-values['MemAvailable'];samples.append(values);time.sleep(.2)
 result={k:int(statistics.median(s[k]for s in samples))for k in samples[0]};s=ctl('status');result.update(windows=len(s['windows']),refresh_hz=s['output']['refresh_hz'],audio=s['audio']['state'])
 if version=='0.9.0':assert s['launcher']['buffer_bytes']==s['wifi']['buffer_bytes']==s['screencast']['buffer_bytes']==0 and not s['launcher']['loading'];result['idle_transient_buffers']=0
 ctl('quit');time.sleep(.3);return result
try:
 results={
  'baseline_0.8.0_kib':measure('/usr/bin/ember-baseline-0.8.0','0.8.0','/etc/emberwm/baseline-0.8.toml'),
  'new_same_config_kib':measure('/usr/bin/emberwm','0.9.0','/etc/emberwm/baseline-0.8.toml'),
  'new_low_memory_preset_kib':measure('/usr/bin/emberwm','0.9.0','/etc/emberwm/low-memory-test.toml')}
 results['scope']='512 MiB native VM at 1920x1080/143.981 Hz, five idle samples after settling, no windows, audio disabled consistently. PSS is compositor-only; guest_used_available is whole guest. Physical Gentoo services and hardware audio are excluded.'
 results['preset_pss_saved_kib']=results['new_same_config_kib']['Pss']-results['new_low_memory_preset_kib']['Pss']
 assert results['new_same_config_kib']['Pss_Anon']-results['baseline_0.8.0_kib']['Pss_Anon']<4096,results
 (ROOT/'docs/memory-0.9.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))
finally:
 try:ctl('quit')
 except AssertionError:pass
