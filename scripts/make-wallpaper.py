#!/usr/bin/env python3
"""Generate Ember's editable vector wallpaper. Rasterize with rsvg-convert."""
from pathlib import Path
import math
import random

root=Path(__file__).resolve().parents[1]
assets=root/'assets'
assets.mkdir(exist_ok=True)
rng=random.Random(6024)
parts=['''<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 1600 1000">
<defs>
  <linearGradient id="sky" x2="0" y2="1"><stop stop-color="#210f1c"/><stop offset="0.48" stop-color="#703040"/><stop offset="1" stop-color="#180f18"/></linearGradient>
  <radialGradient id="glow"><stop stop-color="#a54959" stop-opacity="0.3"/><stop offset="1" stop-color="#a54959" stop-opacity="0"/></radialGradient>
  <linearGradient id="mist" x2="0" y2="1"><stop stop-color="#9b4958" stop-opacity="0"/><stop offset="0.5" stop-color="#9b4958" stop-opacity="0.12"/><stop offset="1" stop-color="#9b4958" stop-opacity="0"/></linearGradient>
  <radialGradient id="vignette" r="0.75"><stop offset="0.3" stop-color="#09080f" stop-opacity="0"/><stop offset="1" stop-color="#09080f" stop-opacity="0.78"/></radialGradient>
</defs>
<rect width="1600" height="1000" fill="url(#sky)"/>
<ellipse cx="1030" cy="370" rx="650" ry="420" fill="url(#glow)"/>
<circle cx="1135" cy="255" r="62" fill="#c16a72" opacity="0.28"/>
<circle cx="1153" cy="240" r="61" fill="#482132"/>
''']
for layer,base,amplitude,colour in [(0,520,55,'#3b1e2b'),(1,650,75,'#2a1622'),(2,810,55,'#1e121c')]:
    points=[]
    for x in range(-20,1641,20):
        y=base+math.sin(x/195+layer*1.8)*amplitude+math.sin(x/71+layer)*amplitude/5
        points.append(f'{x},{y:.1f}')
    parts.append(f'<polygon points="-20,1000 {" ".join(points)} 1640,1000" fill="{colour}"/>')

def branch(x,y,angle,length,width,depth,opacity):
    x2=x+math.cos(angle)*length
    y2=y+math.sin(angle)*length
    cx=x+(x2-x)*0.38+rng.uniform(-10,10)
    cy=y+(y2-y)*0.6
    parts.append(f'<path d="M{x:.1f},{y:.1f} Q{cx:.1f},{cy:.1f} {x2:.1f},{y2:.1f}" fill="none" stroke="#160f19" stroke-width="{width:.2f}" stroke-linecap="round" opacity="{opacity}"/>')
    if depth:
        branch(x2,y2,angle+rng.uniform(-0.24,0.25),length*0.75,width*0.68,depth-1,opacity)
        sign=-1 if depth%2 else 1
        branch(x2,y2,angle+sign*rng.uniform(0.4,0.95),length*0.58,width*0.48,depth-1,opacity)

for x,y,length,width,depth,opacity in [(80,850,170,22,6,0.9),(340,710,125,14,5,0.8),(1440,880,185,24,6,0.95),(1260,740,105,12,5,0.7),(720,660,80,9,4,0.5)]:
    branch(x,y,-math.pi/2+rng.uniform(-0.14,0.14),length,width,depth,opacity)
parts.append('<rect y="580" width="1600" height="220" fill="url(#mist)"/>')
parts.append('<rect width="1600" height="1000" fill="url(#vignette)"/></svg>')
(assets/'red-dusk.svg').write_text('\n'.join(parts)+'\n')
