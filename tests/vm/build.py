#!/usr/bin/env python3
"""Build a rootless QEMU test fixture using this host's kernel/libraries.
The guest exports only its copied fixture directory, never the real host root.
Tool packages are extracted separately from the Alpine HTTPS repository.
"""
import os, re, shutil, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
WORK=ROOT/'artifacts/native-vm'
GUEST=WORK/'root'
BOOT=WORK/'boot'
TOOLS=Path('/tmp/emberwm-vm-tools')
KERNEL=os.uname().release
for p in [GUEST,BOOT]:p.mkdir(parents=True,exist_ok=True)
for name in ['proc','sys','dev','run','tmp','root','etc','bin','usr/bin','usr/lib64','etc/emberwm','assets','lib64','newroot']:
    (GUEST/name).mkdir(parents=True,exist_ok=True)

def copy(path, dest=None, base=Path('/')):
    path=Path(path)
    dest=dest or GUEST/path.relative_to(base)
    if dest.exists() or dest.is_symlink():return
    dest.parent.mkdir(parents=True,exist_ok=True)
    if path.is_symlink():
        target=os.readlink(path); dest.symlink_to(target)
        copy(path.resolve(),base=base)
    else:shutil.copy2(path,dest)

def elf(path):
    copy(path)
    result=subprocess.run(['ldd',str(path)],capture_output=True,text=True)
    for lib in re.findall(r'(/[^\s()]+)',result.stdout):
        if Path(lib).exists():copy(lib)

# Compile a small client that measures actual native wl_surface frame callbacks.
protocol=Path('/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml')
subprocess.run(['wayland-scanner','client-header',str(protocol),str(WORK/'xdg-shell-client.h')],check=True)
subprocess.run(['wayland-scanner','private-code',str(protocol),str(WORK/'xdg-shell-protocol.c')],check=True)
presentation=Path('/usr/share/wayland-protocols/stable/presentation-time/presentation-time.xml')
for mode,suffix in [('client-header','client.h'),('private-code','protocol.c')]:
    subprocess.run(['wayland-scanner',mode,str(presentation),str(WORK/('presentation-time-'+suffix))],check=True)
flags=subprocess.check_output(['pkg-config','--cflags','--libs','wayland-client'],text=True).split()
subprocess.run(['cc','-O2','-I'+str(WORK),str(ROOT/'tests/vm/frame-client.c'),str(WORK/'xdg-shell-protocol.c'),str(WORK/'presentation-time-protocol.c'),*flags,'-o',str(WORK/'frame-client')],check=True)
elf(WORK/'frame-client');shutil.copy2(WORK/'frame-client',GUEST/'usr/bin/ember-frame-client')
subprocess.run(['cc','-O2','-Wall','-Wextra','-I'+str(WORK),str(ROOT/'tests/vm/wm-client.c'),str(WORK/'xdg-shell-protocol.c'),*flags,'-o',str(WORK/'wm-client')],check=True)
# A sampler measures only the explicitly chosen compositor during native RAM tests.
subprocess.run(['cc','-O2','-Wall','-Wextra','-Werror',str(ROOT/'tests/vm/rss-watch.c'),'-o',str(WORK/'rss-watch')],check=True)
elf(WORK/'rss-watch');shutil.copy2(WORK/'rss-watch',GUEST/'usr/bin/ember-rss-watch')
# Capture protocols use xdg-shell plus the foreign-toplevel/source dependencies.
for filename,xml in [("foreign-toplevel","ext-foreign-toplevel-list"),("image-source","ext-image-capture-source"),("image-copy","ext-image-copy-capture")]:
    for mode,suffix in [("client-header","client.h"),("private-code","protocol.c")]:
        subprocess.run(['wayland-scanner',mode,'/usr/share/wayland-protocols/staging/'+xml+'/'+xml+'-v1.xml',str(WORK/(filename+'-'+suffix))],check=True)
subprocess.run(['cc','-O2','-Wall','-Wextra','-I'+str(WORK),str(ROOT/'tests/vm/image-capture-client.c'),*[str(WORK/(p+'-protocol.c')) for p in ['xdg-shell','foreign-toplevel','image-source','image-copy']],*flags,'-o',str(WORK/'image-capture-client')],check=True)
elf(WORK/'image-capture-client');shutil.copy2(WORK/'image-capture-client',GUEST/'usr/bin/ember-image-capture-client')
# Optional protocol input fixture, exercised only inside the virtual seat.
for filename,xml in [("relative-pointer","unstable/relative-pointer/relative-pointer-unstable-v1.xml"),("pointer-constraints","unstable/pointer-constraints/pointer-constraints-unstable-v1.xml")]:
    for mode,suffix in [("client-header","client.h"),("private-code","protocol.c")]:
        subprocess.run(['wayland-scanner',mode,'/usr/share/wayland-protocols/'+xml,str(WORK/(filename+'-'+suffix))],check=True)
subprocess.run(['cc','-O2','-Wall','-Wextra','-DEMBER_POINTER_TEST','-I'+str(WORK),str(ROOT/'tests/vm/wm-client.c'),str(WORK/'xdg-shell-protocol.c'),str(WORK/'relative-pointer-protocol.c'),str(WORK/'pointer-constraints-protocol.c'),*flags,'-o',str(WORK/'pointer-client')],check=True)
elf(WORK/'pointer-client');shutil.copy2(WORK/'pointer-client',GUEST/'usr/bin/ember-pointer-client')
elf(WORK/'wm-client');shutil.copy2(WORK/'wm-client',GUEST/'usr/bin/ember-wm-client')
# A real screencopy client exercises shared-memory capture and video frame requests.
wlr_xml=next((Path(os.environ.get('CARGO_HOME', str(Path.home()/'.cargo')))/'registry/src').glob('*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml'))
subprocess.run(['wayland-scanner','client-header',str(wlr_xml),str(WORK/'screencopy-client.h')],check=True)
subprocess.run(['wayland-scanner','private-code',str(wlr_xml),str(WORK/'screencopy-protocol.c')],check=True)
subprocess.run(['cc','-O2','-Wall','-Wextra','-I'+str(WORK),str(ROOT/'tests/vm/screencopy-client.c'),str(WORK/'screencopy-protocol.c'),*flags,'-o',str(WORK/'screencopy-client')],check=True)
elf(WORK/'screencopy-client');shutil.copy2(WORK/'screencopy-client',GUEST/'usr/bin/ember-screencopy-client')
elf(ROOT/'target/release/emberwm')
shutil.copy2(ROOT/'target/release/emberwm',GUEST/'usr/bin/emberwm')
# Preserve the compiled lazy-starter location inside the isolated fixture.
helper_dir=Path(subprocess.check_output([str(ROOT/'target/release/emberwm'),'--xwayland-helper-path'],text=True).strip())
elf(helper_dir/'Xwayland')
subprocess.run(['cc','-O2','-Wall','-Wextra',str(ROOT/'tests/x11-popup.c'),'-lX11','-o',str(WORK/'x11-popup')],check=True)
elf(WORK/'x11-popup');shutil.copy2(WORK/'x11-popup',GUEST/'usr/bin/ember-x11-popup')
# Real Xlib and arboard probes are test tools, never session daemons.
subprocess.run(['cc','-O2','-Wall','-Wextra',str(ROOT/'tests/x11-window.c'),'-lX11','-lXext','-o',str(WORK/'x11-window')],check=True)
elf(WORK/'x11-window');shutil.copy2(WORK/'x11-window',GUEST/'usr/bin/ember-x11-window')
probe=ROOT/'artifacts/x11-probe-target/debug/ember-x11-clipboard-probe'
if probe.exists():
    elf(probe);shutil.copy2(probe,GUEST/'usr/bin/ember-x11-clipboard-probe')
for p in ['/usr/bin/Xwayland','/usr/bin/xkbcomp','/usr/libexec/gio-launch-desktop','/usr/bin/gio','/usr/bin/dbus-daemon','/usr/bin/gdbus','/usr/bin/udevadm','/usr/lib64/libEGL.so.1','/usr/lib64/libGLESv2.so.2','/usr/lib64/libEGL_mesa.so.0','/usr/lib64/libgallium-26.2.4.so',
          '/usr/lib64/dri/libdril_dri.so','/usr/lib64/gbm/dri_gbm.so']:
    elf(p)
for p in ['/usr/lib64/dri/kms_swrast_dri.so','/usr/lib64/dri/swrast_dri.so','/etc/ld.so.cache']:
    copy(p)
for src in ['/usr/share/glvnd/egl_vendor.d','/usr/share/X11/xkb','/usr/share/libinput','/usr/lib/udev/rules.d','/usr/lib/udev/hwdb.d']:
    if Path(src).exists():shutil.copytree(src,GUEST/src.lstrip('/'),dirs_exist_ok=True,copy_function=lambda src,dst: str(dst) if Path(dst).exists() else shutil.copy2(src,dst))
copy('/usr/lib/udev/hwdb.bin') if Path('/usr/lib/udev/hwdb.bin').exists() else None
(GUEST/'bin/busybox').write_bytes((TOOLS/'bin/busybox.static').read_bytes())
(GUEST/'bin/busybox').chmod(0o755)
for app in ['sh','mount','mkdir','sleep','cat','echo','ls','free','ps','grep','sed','awk','setsid','cttyhack','poweroff','sync','chmod','cp','date','head','tail','kill','env','uname','touch']:
    (GUEST/'bin'/app).unlink(missing_ok=True);(GUEST/'bin'/app).symlink_to('busybox')
(GUEST/'usr/bin/systemd-udevd').unlink(missing_ok=True)
(GUEST/'usr/bin/systemd-udevd').symlink_to('udevadm')
# Install the small musl-based guest terminal and seatd with only their own dependencies.
loader=TOOLS/'lib/ld-musl-x86_64.so.1'
for name in ['seatd','foot']:
    binary=TOOLS/'usr/bin'/name
    deps=subprocess.check_output([str(loader),'--library-path',f'{TOOLS}/lib:{TOOLS}/usr/lib','--list',str(binary)],text=True)
    copy(binary,base=TOOLS)
    for path in re.findall(r'(/[^\s()]+)',deps):
        p=Path(path)
        if p.exists() and p.is_relative_to(TOOLS):copy(p,base=TOOLS)
copy(loader,base=TOOLS)
for dirname in ['usr/share/fonts','etc/fonts','usr/share/terminfo']:
    if (TOOLS/dirname).exists():shutil.copytree(TOOLS/dirname,GUEST/dirname,dirs_exist_ok=True,copy_function=lambda src,dst: str(dst) if Path(dst).exists() else shutil.copy2(src,dst))
(GUEST/'etc/passwd').write_text('root:x:0:0:root:/root:/bin/sh\n')
(GUEST/'etc/group').write_text('root:x:0:\nvideo:x:27:root\ninput:x:97:root\n')
(GUEST/'etc/nsswitch.conf').write_text('passwd: files\ngroup: files\n')
shutil.copy2(ROOT/'assets/red-dusk.png',GUEST/'assets/red-dusk.png')
shutil.copy2(ROOT/'assets/abbey-oakwood.jpg',GUEST/'assets/abbey-oakwood.jpg')
# Built-in rice assets live at the compiled project path, copied into this fixture only.
project_copy=GUEST/str(ROOT).lstrip('/')
(project_copy/'config/rices').mkdir(parents=True,exist_ok=True)
(project_copy/'assets').mkdir(exist_ok=True)
for rice in (ROOT/'config/rices').glob('*.toml'):shutil.copy2(rice,project_copy/'config/rices'/rice.name)
shutil.copy2(ROOT/'assets/red-dusk.png',project_copy/'assets/red-dusk.png')
shutil.copy2(ROOT/'assets/abbey-oakwood.jpg',project_copy/'assets/abbey-oakwood.jpg')
config=(ROOT/'config/session.toml').read_text()
start=config.index('terminal = [');end=config.index('\n]',start)+2
config=config[:start]+'terminal = ["/usr/bin/foot", "--app-id", "{app_id}"]\n'+config[end:]
config=config.replace('../assets/abbey-oakwood.jpg','/assets/abbey-oakwood.jpg').replace('../assets/red-dusk.png','/assets/red-dusk.png').replace('[audio]\nenabled = true','[audio]\nenabled = false')
(GUEST/'etc/emberwm/config.toml').write_text(config)
# A writable clone of the config is created at boot for reload/rice tests.
(GUEST/'etc/ember-dbus.conf').write_text('<busconfig><type>session</type><listen>unix:path=/tmp/ember-session-bus</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>')
(GUEST/'init-system').write_text('''#!/bin/sh
export PATH=/bin:/usr/bin:/usr/sbin
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
mount -t tmpfs tmpfs /run
mount -t tmpfs tmpfs /tmp
mkdir -p /dev/pts /run/user/0 /tmp/cache /tmp/config
mount -t devpts devpts /dev/pts
chmod 700 /run/user/0
export XDG_RUNTIME_DIR=/run/user/0
export XDG_CACHE_HOME=/tmp/cache
export XDG_CONFIG_HOME=/tmp/config
export HOME=/root
export TERM=xterm
export LIBSEAT_BACKEND=seatd
export LIBSEAT_LOGLEVEL=info
export LP_NUM_THREADS=2
export LIBGL_ALWAYS_SOFTWARE=1
export MESA_LOADER_DRIVER_OVERRIDE=kms_swrast
export GALLIUM_DRIVER=llvmpipe
export RUST_LOG=emberwm=info,smithay=warn
export DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/ember-session-bus
/usr/bin/dbus-daemon --config-file=/etc/ember-dbus.conf --fork --nopidfile
/usr/bin/systemd-udevd --resolve-names=never &
sleep 1
/usr/bin/udevadm trigger --action=add
/usr/bin/udevadm settle --timeout=5
/usr/bin/seatd &
sleep 1
cp /etc/emberwm/config.toml /tmp/config.toml
/usr/bin/emberwm --native --config /tmp/config.toml > /tmp/ember.log 2>&1 &
EMBER_PID=$!
echo EMBER_VM_BOOTED pid=$EMBER_PID
exec /bin/sh
''')
(GUEST/'init-system').chmod(0o755)
# Small initramfs: just busybox and modules needed to mount the readonly 9p fixture.
(BOOT/'bin').mkdir(exist_ok=True)
shutil.copy2(GUEST/'bin/busybox',BOOT/'bin/busybox')
(BOOT/'bin/sh').unlink(missing_ok=True);(BOOT/'bin/sh').symlink_to('busybox')
modules=[]
for name in ['9pnet_virtio','9p','virtio_gpu','virtio_input']:
    result=subprocess.check_output(['modprobe','--show-depends',name],text=True)
    for line in result.splitlines():
        if line.startswith('insmod '):
            path=Path(line.split()[1])
            if path not in modules:modules.append(path)
for path in modules:
    dest=BOOT/path.relative_to('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
init='''#!/bin/sh
/bin/busybox --install -s /bin
mkdir -p /dev /proc /sys /newroot
mount -t devtmpfs dev /dev
mount -t proc proc /proc
mount -t sysfs sys /sys
'''
for p in modules:init+=f'insmod {p}\n'
init+='''mount -t 9p -o trans=virtio,version=9p2000.L,msize=262144,ro emberroot /newroot || exec sh
exec switch_root /newroot /init-system
'''
(BOOT/'init').write_text(init);(BOOT/'init').chmod(0o755)
(BOOT/'lib/firmware/edid').mkdir(parents=True,exist_ok=True)
shutil.copy2(ROOT/'tests/vm/ember-144.edid',BOOT/'lib/firmware/edid/ember-144.bin')
archive=subprocess.run('find . -print0 | cpio --null -o --format=newc',shell=True,cwd=BOOT,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True).stdout
import gzip
(WORK/'initramfs.gz').write_bytes(gzip.compress(archive))
print(f'Guest prepared: {GUEST} (kernel {KERNEL})',flush=True)
