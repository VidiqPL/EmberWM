#!/bin/bash
# Run the user's existing PTT script for exactly one Ember session.
set -u
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
wm_pid=${1:?compositor PID required}
ptt_script=${2-${EMBER_PTT_SCRIPT-$HOME/.local/bin/ptt.sh}}
[[ "$wm_pid" =~ ^[0-9]+$ && "$wm_pid" -gt 1 ]] || exit 2
[[ -n "$ptt_script" && -x "$ptt_script" ]] || exit 0
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/emberwm"
mkdir -p "$state_dir" || exit 1
exec >>"$state_dir/ptt-session.log" 2>&1 || exit 1
ptt_pid=
watch_pid=
cleanup() {
    trap - EXIT
    trap '' INT TERM HUP
    if [[ -n "$ptt_pid" ]]; then
        # The script and its evtest/subshell workers have their own process group.
        kill -TERM -- "-$ptt_pid" 2>/dev/null || :
        for ((i=0; i<20; i++)); do
            kill -0 "$ptt_pid" 2>/dev/null || break
            sleep 0.05
        done
        kill -KILL -- "-$ptt_pid" 2>/dev/null || :
        wait "$ptt_pid" 2>/dev/null || :
    fi
    if [[ -n "$watch_pid" ]]; then
        kill "$watch_pid" 2>/dev/null || :
        wait "$watch_pid" 2>/dev/null || :
    fi
}
trap cleanup EXIT
trap 'exit 143' TERM HUP
trap 'exit 130' INT

# Respect the original script's global single-instance lock, including a copy
# started by another desktop. Never stop or modify that external copy.
lock_path="${XDG_RUNTIME_DIR:-/tmp}/ptt.lock"
if ! flock -n "$lock_path" true; then
    echo "PTT lock already held or unavailable; leaving the existing owner alone."
    exit 0
fi

# A PipeWire socket alone can appear before the default microphone is available.
# Wait for this compositor's IPC loop and for a usable default source.
ready=false
for ((attempt=0; attempt<30; attempt++)); do
    kill -0 "$wm_pid" 2>/dev/null || exit 0
    compositor_ready=false
    for control_socket in "${XDG_RUNTIME_DIR:?}"/emberwm-wayland-*.sock; do
        [[ -S "$control_socket" ]] || continue
        status=$("$project_dir/target/release/emberwm" ctl --socket "$control_socket" status 2>/dev/null) || continue
        if [[ "$status" =~ \"pid\":[[:space:]]*$wm_pid[[:space:]]*[,}] ]]; then
            compositor_ready=true
            break
        fi
    done
    if "$compositor_ready" && timeout 2 wpctl get-volume @DEFAULT_AUDIO_SOURCE@ >/dev/null 2>&1; then
        ready=true
        break
    fi
    sleep 1
done
if ! "$ready"; then
    echo "PTT not started: the compositor/default microphone did not become ready."
    exit 1
fi
kill -0 "$wm_pid" 2>/dev/null || exit 0
setsid -- "$ptt_script" &
ptt_pid=$!
tail --pid="$wm_pid" --sleep-interval=0.5 -f /dev/null &
watch_pid=$!
echo "PTT session started: script=$ptt_script pid=$ptt_pid compositor=$wm_pid"
# If the compositor exits or the script fails, clean up all owned workers.
wait -n "$ptt_pid" "$watch_pid" || :
