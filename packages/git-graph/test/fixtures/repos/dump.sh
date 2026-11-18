#!/bin/sh
# Records the oldest <limit> commits of a repo's log as "<hash> <parents...>" lines,
# newest first, with the refs and order open-git-graph reads.
# usage: dump.sh <repo> <out-file> [limit]
set -eu
git -C "$1" -c log.showSignature=false log --branches='*' --remotes='*' --tags='*' \
  --date-order --format='%H %P' HEAD | tail -n "${3:-2000}" > "$2"
