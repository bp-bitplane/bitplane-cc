#!/bin/sh
# Builds the bundled bitplane-sessiond for this host into src-tauri/binaries,
# at the rev src-tauri/Cargo.toml pins, or from ../bitplane-sessiond when the
# local [patch] in .cargo/config.toml points there (see sessiond-src.sh).
set -eu
cd "$(dirname "$0")/.."

triple="$(rustc -vV | sed -n 's/^host: //p')"
out="src-tauri/binaries/bitplane-sessiond-$triple"
rev="$(sed -n 's/^bitplane-sessiond = .*rev = "\([0-9a-f]*\)".*/\1/p' src-tauri/Cargo.toml)"
[ -n "$rev" ] || { echo "no bitplane-sessiond rev in src-tauri/Cargo.toml" >&2; exit 1; }
root="src-tauri/target/sidecar"

if grep -qs 'bitplane-sessiond = { path' .cargo/config.toml; then
	stamp=local
elif [ "$(cat "$out.rev" 2>/dev/null)" != "$rev" ] || [ ! -x "$out" ]; then
	stamp="$rev"
else
	exit 0
fi

src="$(scripts/sessiond-src.sh)"
scripts/build-env.sh cargo install --quiet --locked --force --root "$root" \
	--path "$src/crates/bitplane-sessiond"

mkdir -p src-tauri/binaries
cp "$root/bin/bitplane-sessiond" "$out"
echo "$stamp" >"$out.rev"
