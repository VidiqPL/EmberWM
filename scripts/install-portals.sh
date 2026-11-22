#!/bin/sh
# Per-user Ember portal selection; other desktops keep their own configuration.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
copy_config() {
    source_file=$1 destination_file=$2
    mkdir -p "$(dirname -- "$destination_file")"
    if [ -f "$destination_file" ]; then
        if cmp -s "$source_file" "$destination_file"; then return; fi
        cp -p "$destination_file" "$destination_file.backup-$(date +%Y%m%d-%H%M%S)"
    fi
    cp "$source_file" "$destination_file"
}
copy_config "$project_dir/packaging/ember.portal" "$data_dir/xdg-desktop-portal/portals/ember.portal"
copy_config "$project_dir/packaging/ember-portals.conf" "$config_dir/xdg-desktop-portal/ember-portals.conf"
portal_config=$(mktemp)
trap 'rm -f "$portal_config"' EXIT HUP INT TERM
printf '[screencast]\nchooser_type=dmenu\nchooser_cmd="%s/target/release/emberwm" --choose-source\nmax_fps=30\n' "$project_dir" > "$portal_config"
copy_config "$portal_config" "$config_dir/xdg-desktop-portal-wlr/Ember"
printf '%s\n' 'Ember portal configuration installed for this user. No desktop services restarted.'
