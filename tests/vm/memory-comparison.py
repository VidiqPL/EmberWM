#!/usr/bin/env python3
"""Matched cold native compositor RAM before/after wallpaper CPU-copy removal."""
import json,re,statistics,time
from pathlib import Path
from control import shell
ROOT=Path(__file__).resolve().parents[2]
def guest(cmd):return shell(cmd).replace('\r','')
def status():
    text=guest('emberwm ctl status');start=text.find('{\n');assert start>=0,text;return json.JSONDecoder().raw_decode(text[start:])[0]
def memory():
    text=guest('p=$(/bin/busybox pidof emberwm ember-before); cat /proc/$p/smaps_rollup')
    return {k:int(v) for k,v in re.findall(r'^(Rss|Pss|Pss_Anon|Pss_File|Private_Dirty):\s+(\d+)',text,re.M)}
samples={'before':[],'after':[]}
try:
    for variant in ['before','after','before','after']:
        guest('if /bin/busybox pidof emberwm ember-before >/dev/null; then emberwm ctl quit; sleep 1; fi; cp /etc/emberwm/config.toml /tmp/config.toml')
        binary='ember-before' if variant=='before' else 'emberwm'
        guest(f'{binary} --native --config /tmp/config.toml >/tmp/ember-memory.log 2>&1 &');time.sleep(1.5)
        s=status();assert s['notifications']['server_running'] and not s['windows'],s
        m=[memory()]
        for _ in range(2):time.sleep(.2);m.append(memory())
        samples[variant].append({k:int(statistics.median(v[k] for v in m))for k in m[0]})
        guest('emberwm ctl quit');time.sleep(.3)
    medians={v:{k:int(statistics.median(row[k]for row in rows))for k in rows[0]}for v,rows in samples.items()}
    report={'samples_kib':samples,'median_kib':medians,'anonymous_saving_kib':medians['before']['Pss_Anon']-medians['after']['Pss_Anon'],'pss_saving_kib':medians['before']['Pss']-medians['after']['Pss'],'scope':'Matched cold 1920x1080 native compositor processes in one private VM boot, same 0.5 config/features/private bus, two runs per binary, three samples each. Audio disabled. Before retains CPU wallpaper pixels; after keeps the renderer texture only and reclaims freed heap pages after image/UI changes. Process RAM, not physical host system RAM.'}
    assert report['anonymous_saving_kib']>7*1024,report
    (ROOT/'docs/memory-optimization-native-0.5.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:
    try:guest('emberwm ctl quit')
    except Exception:pass
