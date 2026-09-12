#!/usr/bin/env bash
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
