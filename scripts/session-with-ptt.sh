#!/bin/bash
# Keep the native compositor and its optional PTT helper in one session lifetime.
set -u
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
wm_pid=
helper_pid=
stop_child() {
    local child=$1 group=${2:-false}
    if "$group"; then kill -TERM -- "-$child" 2>/dev/null || :
    else kill -TERM "$child" 2>/dev/null || :; fi
    for ((i=0; i<40; i++)); do
        kill -0 "$child" 2>/dev/null || break
        sleep 0.05
    done
    if "$group"; then kill -KILL -- "-$child" 2>/dev/null || :
    else kill -KILL "$child" 2>/dev/null || :; fi
    wait "$child" 2>/dev/null || :
}
cleanup() {
    trap - EXIT
    trap '' INT TERM HUP
    [[ -z "$helper_pid" ]] || stop_child "$helper_pid" true
    [[ -z "$wm_pid" ]] || stop_child "$wm_pid"
}
trap cleanup EXIT
trap 'exit 143' TERM HUP
trap 'exit 130' INT
"$@" &
wm_pid=$!
ptt_script=${EMBER_PTT_SCRIPT-$HOME/.local/bin/ptt.sh}
if [[ -n "$ptt_script" && -x "$ptt_script" ]]; then
    setsid -- "$project_dir/scripts/ptt-session.sh" "$wm_pid" "$ptt_script" &
    helper_pid=$!
fi
wait "$wm_pid"
result=$?
wm_pid=
exit "$result"
