#!/bin/sh
# SDDM runs this as the logged-in user. No daemon or elevated process is needed.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "${WAYLAND_DISPLAY:-}" ] || [ -n "${DISPLAY:-}" ]; then
    echo 'Start the standalone session from the login screen or an unused local TTY. Use run-nested.sh inside a desktop.' >&2
    exit 1
fi
binary="$project_dir/target/release/emberwm"
if [ ! -x "$binary" ]; then
    echo 'Build EmberWM first: cargo build --release --locked -j 4' >&2
    exit 1
fi
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=Ember
export XDG_CURRENT_DESKTOP=Ember
export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-wayland;xcb}"
export GDK_BACKEND="${GDK_BACKEND:-wayland,x11}"
export SDL_VIDEO_DRIVER="${SDL_VIDEO_DRIVER:-wayland,x11}"
export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-wayland,x11}"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/emberwm"
mkdir -p "$state_dir"
# Rotate one previous session log, keeping failures easy to inspect from Plasma.
if [ -f "$state_dir/session.log" ]; then cp "$state_dir/session.log" "$state_dir/session.previous.log"; fi
# Read-only diagnostics must not start session helpers or touch the microphone.
for argument in "$@"; do
    case "$argument" in
        --help|-h|--version|--check-config|--list-modes|--check-audio|--print-default-config|--xwayland-helper-path)
            exec dbus-run-session -- "$binary" --native "$@" >> "$state_dir/session.log" 2>&1 ;;
    esac
done
exec dbus-run-session -- "$project_dir/scripts/session-with-ptt.sh" "$binary" --native "$@" >> "$state_dir/session.log" 2>&1
