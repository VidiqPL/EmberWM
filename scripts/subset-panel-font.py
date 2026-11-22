#!/usr/bin/env python3
"""Rebuild the small embedded font. Developer-only dependency: fonttools 4.60.1.

Usage: python3 scripts/subset-panel-font.py [/path/to/DejaVuSansMono.ttf]
Source license is retained beside the generated font. No runtime dependency.
"""
from pathlib import Path
import sys
from fontTools import subset
from fontTools.ttLib import TTFont

source = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('/usr/share/fonts/dejavu/DejaVuSansMono.ttf')
target = Path(__file__).resolve().parents[1] / 'assets/fonts/EmberPanelMono.ttf'
font = TTFont(source)
options = subset.Options()
options.hinting = False
options.layout_features = []
options.notdef_outline = True
options.recommended_glyphs = True
tool = subset.Subsetter(options=options)
# Latin including Polish, Greek, Cyrillic, punctuation, arrows and terminal shapes.
ranges = [(0x20, 0x24f), (0x370, 0x52f), (0x2000, 0x206f), (0x2190, 0x21ff), (0x2500, 0x259f)]
tool.populate(unicodes=[n for a, b in ranges for n in range(a, b + 1)])
tool.subset(font)
names = {1: 'Ember Panel Mono', 2: 'Regular', 3: 'Ember Panel Mono Regular 1.0',
         4: 'Ember Panel Mono', 6: 'EmberPanelMono-Regular'}
for record in font['name'].names:
    if record.nameID in names:
        record.string = names[record.nameID].encode(record.getEncoding())
font.save(target)
print(f'{target.name}: {len(font.getBestCmap())} mapped characters, {target.stat().st_size} bytes')
