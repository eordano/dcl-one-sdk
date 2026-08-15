#!/usr/bin/env bash
# Rebuild the scaffold `run-all.sh` needs. Nothing it creates is committed — see
# `.gitignore` beside this directory's parent — because every byte of it is a copy of
# something already in the tree.
#
#   node_modules/       symlinks, so `require('protobufjs/minimal')` and `require('pbmin')`
#                       resolve from anywhere under the experiment root. They must be
#                       symlinks into `ref/` and `pbmin/`: node resolves symlinks, so the
#                       require-cache key is the same file `harness.js:loadImpl()` requires
#                       by absolute path. Two cache entries for one implementation would
#                       reintroduce exactly the cross-binding bug `verify-isolation.js`
#                       exists to catch.
#   ref/node_modules/   the REFERENCE upstream protobufjs 7.2.4 + its @protobufjs/* + long.
#                       It CANNOT come from the blob any more: since `add_pbmin()` the
#                       blob's `node_modules/protobufjs` IS the implementation under test,
#                       and a differential suite whose reference is the thing being tested
#                       reports a perfect score and means nothing. It comes from a scene's
#                       own `node_modules`, where the npm flow puts upstream 7.2.4, and
#                       the version is checked below.
#   pbmin/              the implementation under test (a copy of ../index.js & co).
#   mutant/             `mutation-test.js` writes a bugged copy of pbmin here.
#   ecs/dist-cjs/       the CJS @dcl/ecs corpus — also from the blob.
#   ecs/dist/           the ESM @dcl/ecs corpus. NOT in the blob (the prebuilt chunks
#                       replaced it), so it comes from any scene's node_modules at the
#                       same version; the script checks that version matches.
#   rpc/                the two catalogues phase 1c covers, both from the blob:
#                       @dcl/rpc/dist/protocol/index.js and
#                       @dcl/inspector/data-layer.gen.js.
#
# Usage: tests/setup.sh [path-to-a-scene-node_modules]
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
BLOB="$ROOT/../../src/vendor/node_modules.zip"
SCENE_NM="${1:-}"

[ -f "$BLOB" ] || { echo "no blob at $BLOB"; exit 1; }

# A scene's own node_modules: source of the reference protobufjs and of the ESM ecs build.
if [ -z "$SCENE_NM" ]; then
    for c in "$ROOT"/../../../../../third-party/sdk7-test-scenes/scenes/*/node_modules; do
        [ -d "$c/protobufjs/src" ] && [ -d "$c/@dcl/ecs/dist" ] && { SCENE_NM="$c"; break; }
    done
fi
if [ -z "$SCENE_NM" ] || [ ! -d "$SCENE_NM/protobufjs/src" ]; then
    echo "need a scene node_modules with upstream protobufjs and @dcl/ecs/dist."
    echo "usage: tests/setup.sh <path-to-scene/node_modules>"
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q "$BLOB" -d "$WORK"
NM="$WORK/node_modules"

rm -rf "$ROOT/ref" "$ROOT/ecs" "$ROOT/pbmin" "$ROOT/rpc" "$ROOT/mutant" "$ROOT/node_modules"
mkdir -p "$ROOT/ref/node_modules" "$ROOT/ecs" "$ROOT/pbmin" "$ROOT/mutant" \
         "$ROOT/rpc/protocol" "$ROOT/rpc/datalayer" "$ROOT/node_modules"

# The reference, from the scene tree — NOT from the blob, which now ships pbmin under the
# name `protobufjs`. `EXTRA_INSTALL` in build-base-blob.py pins 7.2.4; so does the npm flow
# that filled this scene, so the two agree. If they ever stop, this refuses rather than
# quietly testing against a version nothing ships.
REFV="$(node -p "require('$SCENE_NM/protobufjs/package.json').version")"
[ "$REFV" = "7.2.4" ] || { echo "reference protobufjs is $REFV, expected 7.2.4"; exit 1; }
cp -R "$SCENE_NM/protobufjs" "$SCENE_NM/@protobufjs" "$SCENE_NM/long" "$ROOT/ref/node_modules/"
cp -R "$NM/@dcl/ecs/dist-cjs" "$ROOT/ecs/dist-cjs"
cp "$NM/@dcl/rpc/dist/protocol/index.js" "$ROOT/rpc/protocol/index.gen.js"
cp "$NM/@dcl/inspector/data-layer.gen.js" "$ROOT/rpc/datalayer/data-layer.gen.js"

cp "$ROOT/index.js" "$ROOT/index.mjs" "$ROOT/index.d.ts" "$ROOT/package.json" "$ROOT/pbmin/"
cp "$ROOT/index.js" "$ROOT/mutant/index.js"
sed 's/@dcl\/pbmin/@dcl\/pbmutant/' "$ROOT/package.json" > "$ROOT/mutant/package.json"

ln -sfn ../ref/node_modules/protobufjs  "$ROOT/node_modules/protobufjs"
ln -sfn ../ref/node_modules/@protobufjs "$ROOT/node_modules/@protobufjs"
ln -sfn ../ref/node_modules/long        "$ROOT/node_modules/long"
ln -sfn ../pbmin                        "$ROOT/node_modules/pbmin"
ln -sfn ../mutant                       "$ROOT/node_modules/pbmutant"

# The ESM corpus. `esm-corpus-diff.mjs` is the only phase that needs it, and it is the one
# thing here the blob cannot supply — the prebuilt chunks replaced `@dcl/ecs/dist`.
WANT="$(node -p "require('$NM/@dcl/ecs/package.json').version")"
GOT="$(node -p "require('$SCENE_NM/@dcl/ecs/package.json').version")"
[ "$GOT" = "$WANT" ] || echo "WARNING: scene has @dcl/ecs $GOT, blob has $WANT — the ESM phase will test the wrong build"
cp -R "$SCENE_NM/@dcl/ecs/dist" "$ROOT/ecs/dist"

echo "scaffold ready under $ROOT (reference protobufjs $REFV, @dcl/ecs $WANT/$GOT from $SCENE_NM)"
