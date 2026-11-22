#!/usr/bin/env python3
import json,socket,time,sys,uuid
from pathlib import Path
WORK=Path(__file__).resolve().parents[2]/'artifacts/native-vm'
def shell(command, timeout=15):
    s=socket.socket(socket.AF_UNIX);s.settimeout(timeout);s.connect(str(WORK/'serial.sock'))
    token=uuid.uuid4().hex
    end=('__EMBER_END__'+token).encode()
    s.sendall(("\n"+command+"\nprintf '\\n__EMBER_END__%s\\n' "+token+"\n").encode())
    data=b''
    try:
        while end not in data:
            chunk=s.recv(65536)
            if not chunk:break
            data+=chunk
    finally:s.close()
    text=data.decode(errors='replace')
    with (WORK/'serial-transcript.txt').open('a') as f:f.write(text+'\n')
    return text

def qmp(command, arguments=None):
    s=socket.socket(socket.AF_UNIX);s.settimeout(10);s.connect(str(WORK/'qmp.sock'))
    f=s.makefile('rwb',buffering=0)
    json.loads(f.readline())
    f.write(json.dumps({'execute':'qmp_capabilities'}).encode()+b'\n')
    while 'return' not in json.loads(f.readline()):pass
    f.write(json.dumps({'execute':command,'arguments':arguments or {},'id':1}).encode()+b'\n')
    while True:
        result=json.loads(f.readline())
        if result.get('id')==1:break
    f.close();s.close()
    return result
if __name__=='__main__':
    if sys.argv[1]=='qmp':print(json.dumps(qmp(sys.argv[2],json.loads(sys.argv[3]) if len(sys.argv)>3 else {}),indent=2))
    else:print(shell(' '.join(sys.argv[1:])))
