#!/usr/bin/env python3
"""Exercise CPU helper transactions against synthetic sysfs, never host settings."""
import runpy
import tempfile
from pathlib import Path
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
apply=runpy.run_path(str(ROOT/'scripts/emberwm-power'))['apply_profile']
with tempfile.TemporaryDirectory() as directory:
    root=Path(directory)
    for i in range(2):
        p=root/f'policy{i}';p.mkdir()
        for name,value in {'scaling_available_governors':'performance powersave','energy_performance_available_preferences':'performance balance_performance power','scaling_governor':'powersave','energy_performance_preference':'balance_performance','boost':'1','scaling_max_freq':'4668750','scaling_min_freq':'2390625'}.items():
            (p/name).write_text(value)
    before={p:p.read_text() for p in root.rglob('*') if p.is_file()}
    for profile in ['../../performance','performance; echo bad','', 'balanced extra']:
        try:apply(profile,root)
        except ValueError:pass
        else:raise AssertionError('invalid profile accepted')
    assert all(p.read_text()==value for p,value in before.items())
    assert apply('performance',root)=={'profile':'performance','policies':2}
    assert all((root/f'policy{i}/energy_performance_preference').read_text()=='performance' for i in range(2))
    assert all(p.read_text()==v for p,v in before.items() if p.name not in ['scaling_governor','energy_performance_preference'])
    apply('balanced',root)
    (root/'policy1/energy_performance_available_preferences').write_text('performance')
    try:apply('power-saver',root)
    except RuntimeError:pass
    else:raise AssertionError('unsupported policy changed')
    assert (root/'policy0/energy_performance_preference').read_text()=='balance_performance'
    (root/'policy1/energy_performance_available_preferences').write_text('performance balance_performance power')
    write=Path.write_text
    def fail_once(path,text,*args,**kwargs):
        if path==root/'policy1/energy_performance_preference' and text=='power':
            raise OSError('synthetic second-policy write failure')
        return write(path,text,*args,**kwargs)
    with patch.object(Path,'write_text',fail_once):
        try:apply('power-saver',root)
        except RuntimeError as error:assert 'previous settings restored' in str(error)
        else:raise AssertionError('partial failure was not reported')
    assert all(p.read_text()==value for p,value in before.items())
    print('PASS: input rejection, full preflight, two-policy updates, unchanged boost/limits, rollback after partial failure')
