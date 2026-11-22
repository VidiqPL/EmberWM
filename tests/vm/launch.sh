#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
vm_tools=/tmp/emberwm-vm-tools
export QEMU_MODULE_DIR="$vm_tools/usr/lib/qemu"
exec "$vm_tools/lib/ld-musl-x86_64.so.1" --library-path "$vm_tools/lib:$vm_tools/usr/lib" \
 "$vm_tools/usr/bin/qemu-system-x86_64" -L "$vm_tools/usr/share/qemu" \
 -machine q35,accel=kvm -cpu host -m 512 -smp 2 -nodefaults \
 -device virtio-gpu-pci,xres=1280,yres=800 -device virtio-keyboard-pci -device virtio-mouse-pci \
 -display none -chardev "socket,id=serial,path=$project_dir/artifacts/native-vm/serial.sock,server=on,wait=off" \
 -serial chardev:serial -qmp "unix:$project_dir/artifacts/native-vm/qmp.sock,server=on,wait=off" \
 -kernel "/boot/vmlinuz-$(uname -r)" -initrd "$project_dir/artifacts/native-vm/initramfs.gz" \
 -append 'console=ttyS0 quiet loglevel=3 panic=-1 drm.edid_firmware=edid/ember-144.bin' \
 -virtfs "local,path=$project_dir/artifacts/native-vm/root,mount_tag=emberroot,security_model=none,readonly=on"
