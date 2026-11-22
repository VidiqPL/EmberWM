import concurrent.futures, io, tarfile, urllib.request, pathlib, re, json
root=pathlib.Path('/tmp/emberwm-vm-tools')
root.mkdir(parents=True,exist_ok=True)
packages={};providers={}
base='https://dl-cdn.alpinelinux.org/alpine/v3.23'
for repo in ['main','community']:
    data=urllib.request.urlopen(f'{base}/{repo}/x86_64/APKINDEX.tar.gz',timeout=30).read()
    with tarfile.open(fileobj=io.BytesIO(data)) as t:
        index=t.extractfile('APKINDEX').read().decode()
    for block in index.split('\n\n'):
        record=dict(line.split(':',1) for line in block.splitlines() if ':' in line)
        if 'P' not in record: continue
        record['repo']=repo;packages[record['P']]=record
        for provider in record.get('p','').split(): providers[re.split('[=<>~]',provider)[0]]=record['P']
wanted=['qemu-system-x86_64','busybox-static','seatd','foot','font-dejavu','qemu-hw-display-virtio-gpu-pci'];chosen={}
while wanted:
    name=re.split('[=<>~]',wanted.pop())[0]
    if name.startswith('!'):continue
    if name not in packages:name=providers.get(name,name)
    if name in chosen:continue
    if name not in packages:raise Exception('missing '+name)
    p=packages[name];chosen[name]=p
    wanted.extend(p.get('D','').split())
print('Downloading tool packages:',len(chosen), flush=True)
cache=root/'packages';cache.mkdir(exist_ok=True)
def fetch(p):
    fn=f"{p['P']}-{p['V']}.apk";path=cache/fn
    if not path.exists():path.write_bytes(urllib.request.urlopen(f"{base}/{p['repo']}/x86_64/{fn}",timeout=60).read())
    return path
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
    for path in pool.map(fetch,chosen.values()):
        import subprocess
        subprocess.run(['bsdtar','--no-same-owner','-xf',str(path),'-C',str(root)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
(root/'manifest.json').write_text(json.dumps(chosen,indent=2))
print('Tools extracted',flush=True)
