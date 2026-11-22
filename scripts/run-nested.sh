#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ ! -x "$project_dir/target/release/emberwm" ]; then
    cargo build --release --locked -j 4 --manifest-path "$project_dir/Cargo.toml"
fi
exec "$project_dir/target/release/emberwm" --nested "$@"
