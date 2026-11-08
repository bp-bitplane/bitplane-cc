#!/bin/sh
# Builds the bundled bitplane-sessiond for this host into src-tauri/binaries,
# at the rev src-tauri/Cargo.toml pins, or from ../bitplane-sessiond when the
# local [patch] in .cargo/config.toml points there.
set -eu
cd "$(dirname "$0")/.."

triple="$(rustc -vV | sed -n 's/^host: //p')"
out="src-tauri/binaries/bitplane-sessiond-$triple"
rev="$(sed -n 's/^bitplane-sessiond = .*rev = "\([0-9a-f]*\)".*/\1/p' src-tauri/Cargo.toml)"
[ -n "$rev" ] || { echo "no bitplane-sessiond rev in src-tauri/Cargo.toml" >&2; exit 1; }
root="src-tauri/target/sidecar"

if grep -qs 'bitplane-sessiond = { path' .cargo/config.toml; then
	cargo install --quiet --locked --root "$root" --path ../bitplane-sessiond/crates/bitplane-sessiond
elif [ "$(cat "$out.rev" 2>/dev/null)" != "$rev" ] || [ ! -x "$out" ]; then
	cargo install --quiet --locked --force --root "$root" \
		--git https://github.com/bp-bitplane/bitplane-sessiond --rev "$rev" bitplane-sessiond
else
	exit 0
fi

mkdir -p src-tauri/binaries
cp "$root/bin/bitplane-sessiond" "$out"
echo "$rev" >"$out.rev"
