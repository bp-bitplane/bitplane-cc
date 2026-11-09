#!/bin/sh
# Prints the bitplane-sessiond checkout cc builds against: ../bitplane-sessiond
# when the local [patch] in .cargo/config.toml points there, else a checkout
# of the rev src-tauri/Cargo.toml pins, fetched into src-tauri/target.
set -eu
cd "$(dirname "$0")/.."

if grep -qs 'bitplane-sessiond = { path' .cargo/config.toml; then
	(cd ../bitplane-sessiond && pwd)
	exit 0
fi

rev="$(sed -n 's/^bitplane-sessiond = .*rev = "\([0-9a-f]*\)".*/\1/p' src-tauri/Cargo.toml)"
[ -n "$rev" ] || { echo "no bitplane-sessiond rev in src-tauri/Cargo.toml" >&2; exit 1; }
dir=src-tauri/target/sessiond-src

if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$rev" ]; then
	rm -rf "$dir"
	mkdir -p "$dir"
	git -C "$dir" init --quiet
	git -C "$dir" fetch --quiet --depth 1 https://github.com/bp-bitplane/bitplane-sessiond.git "$rev"
	git -C "$dir" -c advice.detachedHead=false checkout --quiet FETCH_HEAD
fi
echo "$(pwd)/$dir"
