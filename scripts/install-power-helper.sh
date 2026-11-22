#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$(id -u)" != 0 ]; then
    echo 'Run with sudo: sudo sh scripts/install-power-helper.sh' >&2
    exit 1
fi
install -d -o root -g root -m 0755 /usr/local/libexec /usr/share/polkit-1/actions
install -o root -g root -m 0755 "$project_dir/scripts/emberwm-power" /usr/local/libexec/emberwm-power
install -o root -g root -m 0644 "$project_dir/packaging/org.emberwm.power.policy" /usr/share/polkit-1/actions/org.emberwm.power.policy
printf '%s\n' 'Installed fixed-purpose CPU helper and authorization policy. No service enabled.'
