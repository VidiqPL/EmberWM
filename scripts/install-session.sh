#!/bin/sh
# Install only SDDM's session descriptor; preserve existing login sessions and settings.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
case "${1:-}" in
    --print|'') ;;
    *) echo "Usage: $0 [--print]" >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then echo "Usage: $0 [--print]" >&2; exit 2; fi
descriptor=$(mktemp --suffix=.desktop)
trap 'rm -f "$descriptor"' EXIT HUP INT TERM
python3 "$project_dir/scripts/render-session.py" "$project_dir" > "$descriptor"
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$descriptor"
fi
if [ "${1:-}" = --print ]; then cat "$descriptor"; exit 0; fi
if [ "$(id -u)" -ne 0 ]; then
    echo "Administrator access is required to write /usr/share/wayland-sessions." >&2
    echo "Run: sudo $project_dir/scripts/install-session.sh" >&2
    exit 1
fi
if [ ! -x "$project_dir/target/release/emberwm" ]; then echo 'Build the release first.' >&2; exit 1; fi
if [ -f /usr/share/wayland-sessions/emberwm.desktop ]; then
    if ! cmp -s "$descriptor" /usr/share/wayland-sessions/emberwm.desktop; then
        cp -p /usr/share/wayland-sessions/emberwm.desktop "/usr/share/wayland-sessions/emberwm.desktop.backup-$(date +%Y%m%d-%H%M%S)"
    fi
fi
install -D -m 644 "$descriptor" /usr/share/wayland-sessions/emberwm.desktop
printf '%s\n' 'EmberWM is installed in the login-screen session list. No services were restarted.'
