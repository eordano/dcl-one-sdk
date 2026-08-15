#!/usr/bin/env bash
# Full differential suite. Usage: tests/run-all.sh [seed]
set -u
cd "$(dirname "$0")/.."
SEED="${1:-0xC0FFEE}"
export SEED
fail=0
run() { echo; echo "### $*"; env "${@:2}" node "$1" || fail=1; }

node tests/verify-isolation.js || fail=1
node tests/esm-check.mjs || fail=1
run tests/corpus-diff.js    ITERS=300
run tests/corpus-diff.js    ITERS=300 NO_BUFFER=1
run tests/corpus-diff.js    ITERS=300 NO_LONG=1
run tests/corpus-diff.js    ITERS=300 NO_BUFFER=1 NO_LONG=1
run tests/esm-corpus-diff.mjs ITERS=100
# 41 namespaces instead of 336, so the per-type iteration count goes up to keep the
# instance count in the same order: 41 x 2000 = 82,000 per environment.
run tests/rpc-diff.js       ITERS=2000
run tests/rpc-diff.js       ITERS=2000 NO_BUFFER=1
run tests/rpc-diff.js       ITERS=2000 NO_LONG=1
run tests/rpc-diff.js       ITERS=2000 NO_BUFFER=1 NO_LONG=1
run tests/primitive-diff.js
run tests/fuzz.js           N_RAW=400000 N_MSG=60
echo
node tests/mutation-test.js || fail=1
echo
[ $fail -eq 0 ] && echo "ALL GREEN (seed $SEED)" || echo "FAILURES PRESENT (seed $SEED)"
exit $fail
