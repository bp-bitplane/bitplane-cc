#!/bin/sh
# Runs a command with what building libghostty-vt needs, which cc links
# through the bitplane-sessiond lib as well as in its sidecar: Zig 0.15.2 on
# PATH, plus sessiond's scripts/build-env.sh (the vendored ghostty source and,
# on macOS, the SDK shim). See sessiond's docs/build.md.
set -eu

src="$("$(dirname "$0")/sessiond-src.sh")"
exec "$src/scripts/build-env.sh" "$@"
