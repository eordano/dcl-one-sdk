#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
BLOB="$ROOT/../../src/vendor/node_modules.zip"
SCENE_NM="${1:-}"

[ -f "$BLOB" ] || { echo "no blob at $BLOB"; exit 1; }

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

WANT="$(node -p "require('$NM/@dcl/ecs/package.json').version")"
GOT="$(node -p "require('$SCENE_NM/@dcl/ecs/package.json').version")"
[ "$GOT" = "$WANT" ] || echo "WARNING: scene has @dcl/ecs $GOT, blob has $WANT — the ESM phase will test the wrong build"
cp -R "$SCENE_NM/@dcl/ecs/dist" "$ROOT/ecs/dist"

echo "scaffold ready under $ROOT (reference protobufjs $REFV, @dcl/ecs $WANT/$GOT from $SCENE_NM)"
