#!/bin/sh
# Vendors bitplane-core's generated bp-contract.ts into src/ipc at a bitplane-cli
# rev, recorded in src/ipc/bp-contract.rev.
#   bp-contract.sh [<rev>]   copy it at <rev> (default: the checkout's HEAD)
#   bp-contract.sh --check   fail when the copy differs from the recorded rev
set -eu
cd "$(dirname "$0")/.."

cli="${BITPLANE_CLI:-../bitplane-cli}"
file=crates/bitplane-core/bp-contract.ts
out=src/ipc/bp-contract.ts
pin=src/ipc/bp-contract.rev

if [ "${1:-}" = --check ]; then
	rev="$(cat "$pin")"
	git -C "$cli" show "$rev:$file" | cmp -s - "$out" || {
		echo "$out differs from bitplane-cli $rev; run pnpm bp-contract $rev" >&2
		exit 1
	}
	exit 0
fi

rev="$(git -C "$cli" rev-parse --verify "${1:-HEAD}^{commit}")"
git -C "$cli" show "$rev:$file" >"$out"
echo "$rev" >"$pin"
