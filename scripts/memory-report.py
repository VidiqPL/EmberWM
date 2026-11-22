#!/usr/bin/env python3
"""Read-only RAM report. PSS apportions shared mappings; RSS does not."""
import json
import os
from pathlib import Path
import time


def fields(path):
    result = {}
    for line in path.read_text().splitlines():
        if ':' in line:
            key, value = line.split(':', 1)
            try:
                result[key] = int(value.split()[0])
            except (ValueError, IndexError):
                pass
    return result


def report():
    memory = fields(Path('/proc/meminfo'))
    processes = []
    for p in Path('/proc').iterdir():
        if not p.name.isdecimal():
            continue
        try:
            name = (p / 'comm').read_text().strip()
            rollup = fields(p / 'smaps_rollup')
            processes.append({'pid': int(p.name), 'name': name,
                              'rss_kib': rollup.get('Rss', 0),
                              'pss_kib': rollup.get('Pss', 0),
                              'anonymous_pss_kib': rollup.get('Pss_Anon', 0),
                              'file_pss_kib': rollup.get('Pss_File', 0)})
        except (OSError, ProcessLookupError):
            continue
    return {'timestamp': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
            'measurement': 'live session, not necessarily idle; available-based used = MemTotal - MemAvailable',
            'memory_kib': memory,
            'available_based_used_kib': memory['MemTotal'] - memory['MemAvailable'],
            'processes_readable': len(processes),
            'top_processes_by_pss': sorted(processes, key=lambda p: p['pss_kib'], reverse=True)[:25],
            'ember_processes': [p for p in processes if p['name'] == 'emberwm']}

if __name__ == '__main__':
    print(json.dumps(report(), indent=2))
