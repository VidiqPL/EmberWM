#!/usr/bin/env python3
"""Confirm a warm baseline and frame-callback performance after the idle font eviction."""
import importlib.util,json,time
from pathlib import Path
p=Path(__file__).with_name('memory-audit.py');spec=importlib.util.spec_from_file_location('audit',p);audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
results={}
try:
 for label,binary,version in [('baseline','/usr/bin/ember-baseline-0.11.0','0.11.0'),('optimized','/usr/bin/emberwm','0.11.1')]:
  memory=audit.measure(binary,version,'/etc/emberwm/baseline-0.8.toml')
  state=audit.start(binary,version,'/etc/emberwm/baseline-0.8.toml');display=state['wayland_display'];time.sleep(3)
  text=audit.guest(f'WAYLAND_DISPLAY={display} ember-frame-client');line=next(l for l in text.splitlines()if l.startswith('{"frames":'));frames=json.loads(line)
  assert 125<frames['fps']<155,frames
  results[label]={'memory_kib':memory,'frame_callbacks':frames};audit.stop()
 results['warm_pss_saved_kib']=results['baseline']['memory_kib']['Pss']-results['optimized']['memory_kib']['Pss']
 results['warm_anonymous_saved_kib']=results['baseline']['memory_kib']['Pss_Anon']-results['optimized']['memory_kib']['Pss_Anon']
 results['scope']='Warm native baseline confirmation and 3-second synthetic frame-callback benchmark, nominal 144 Hz, 512 MiB virtual machine, unchanged config and audio disabled. This does not measure physical game FPS.'
 (audit.ROOT/'docs/memory-audit-warm-results.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))
finally:audit.stop()
