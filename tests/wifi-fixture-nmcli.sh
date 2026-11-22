#!/bin/sh
# Test-only scan output. This executable never forwards to the host nmcli.
case "$*" in
  *"device status") printf '%s\n' 'wlan0:wifi:connected' 'eth0:ethernet:connected';;
  *"radio wifi") printf '%s\n' enabled;;
  *"device wifi rescan"*) exit 0;;
  *"device wifi list"*) printf '%s\n' '*:Ember Test:AA\:BB\:CC\:DD\:EE\:01:85:WPA2:/org/freedesktop/NetworkManager/AccessPoint/1' ':Cafe\: guest:AA\:BB\:CC\:DD\:EE\:02:62:--:/org/freedesktop/NetworkManager/AccessPoint/2';;
  *) exit 1;;
esac
