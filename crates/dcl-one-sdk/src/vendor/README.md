# Vendored node_modules

`node_modules.zip` is extracted by `dcl-one-sdk init` so a scaffolded scene builds
and previews with no npm and no network. It contains the scene toolchain the Rust
CLI actually uses: `@dcl/sdk` (with `@dcl/ecs`, `@dcl/react-ecs`, `@dcl/js-runtime`,
`@dcl/ecs-math` and their runtime deps) plus `typescript` for the type check. It is
pure JS — no platform binaries — so one blob serves linux, macOS and Windows.

The current blob is a pure registry install of the released 7.26.0 toolchain — no
overlays. #1450 (single tree-shakeable ecs) shipped upstream, and #1452 (built-in
utf-8 codec) has now landed too: `@dcl/sdk` no longer imports
`text-encoding`, so the 549 KB polyfill is gone from the tree and the last
regression vs the old overlaid blob is closed.

**Build it with `scripts/build-base-blob.py`.** The manual recipe below is kept
as background, but the script is the source of truth: it derives its install
list from `templates/init/scene/package.json` (so the vendored set cannot drift
from the scaffold pin), keeps only what is reachable from the code that ships,
and fails if any kept file has an import that would not resolve. Its module
docstring carries the evidence for every prune. Current output: **422 files, 12.37 MB
unpacked, 2.31 MB zipped** — down from 2,999 / 45.0 MB / 11.0 MB, most of
that from the prebuilt SDK export (chunks + a types rollup replace the SDK
source tree) and from dropping the vendored editor. It was 424 / 12.07 MB /
2.27 MB before the data-layer host landed; that host cost **+23 files,
+0.28 MB unpacked, +0.05 MB zipped**, and replacing upstream `protobufjs` with
our own drop-in then took **-34 files, -0.05 MB unpacked, -0.02 MB zipped**
back off (447 -> 413).

Two versions are deliberately held below latest, both re-checked on every
rebuild:

* **`typescript` 6.0.3, not 7.0.2.** TS 7 is the Go port: `lib/getExePath.js`
  resolves a per-platform native package (`@typescript/typescript-darwin-arm64`
  and friends), which would end this blob's one-zip-serves-every-OS property.
  TS 6 is pure JS and works. It reports three deprecations on the scaffold:
  `baseUrl` (the scaffold's own tsconfig) and `downlevelIteration` +
  `moduleResolution=node10` (inherited from
  `@dcl/sdk/types/tsconfig.ecs7.json`, which every scaffolded scene extends).
  Two are fixed in `templates/init/scene/tsconfig.json` — `baseUrl` deleted,
  `moduleResolution: "bundler"` set — and the remaining one needs a change to
  the upstream ecs7 tsconfig. Do **not** paper over any of them with
  `"ignoreDeprecations": "6.0"`: TS 7 *removes* these options, so the mute buys
  nothing, and TS 5.9.3 rejects the option outright (TS5103). See
  `docs/ts7-migration.md` for the exact upstream diff and the evidence.
* **`protobufjs` 7.2.4, not 8.7.1.** No upstream protobufjs *runtime* reaches
  the blob any more — not as a package, and since `swap_pbmin_into_tree()` not
  inside `prebuilt/core.js` either. It stays installed for its declarations,
  which `build_types_rollup()` needs because `@dcl/ecs/dist` imports
  `protobufjs/minimal` without declaring it, and as the reference the
  replacement is differentially tested against. That second use is what pins the
  version: the npm flow resolves 7.2.4 through `@dcl/sdk` -> `@dcl/sdk-commands`
  -> `@dcl/protocol`, whose manifest pins it exactly, so 7.2.4 is what a scene
  built the npm way actually runs. Installing 8.x would aim the suite at a
  reference nothing uses and quietly void its evidence.

**`protobufjs` is not upstream's any more.** The blob ships
`node_modules/protobufjs` from
`experiments/protobufjs-minimal-replacement/index.js` — a dependency-free
reimplementation of the `Reader` / `Writer` / `util.Long` / `configure` surface,
4 files / 48,340 B, where upstream's `minimal` closure was 13 files / 58,872 B
and dragged six `@protobufjs/*` micro-packages (25 files / 41,117 B) behind it.
Those six now fall out of the BFS on their own, exactly the way
`@protobufjs/{codegen,path,fetch}` already had: `reachable()` only walks files
the allowlist keeps, and protobufjs keeps none. `@protobufjs/utf8` is the one
that stays — `@dcl/ecs/dist-cjs` requires it *directly*
(`serialization/ByteBuffer/index.js`, `components/component-number.js`), not
through protobufjs, so the replacement does not subsume it. `long` stays too:
`avatar_shape.gen.js` and `descriptor.gen.js` name it.

Net: **-34 files, -51,649 B unpacked, -25,873 B zipped** (447 -> 413 files,
12,954,435 -> 12,902,786 B unpacked, 2,429,940 -> 2,404,067 B zipped).

An earlier revision of this file said of the old prune: *"The 53 KB that stays
is the entire CRDT wire codec; nothing in it is safe to hand-port."* That was
the right instinct and the wrong conclusion. It is not safe to hand-port on a
reading; it is safe to replace on a differential suite, and there is one —
`experiments/protobufjs-minimal-replacement/tests`, run with `tests/setup.sh &&
bash tests/run-all.sh <seed>`. Per seed, with upstream 7.2.4 loaded side by side
as the reference and every corpus module bound to exactly one implementation
(asserted each run by `verify-isolation.js`), asserting byte-identical encodings
in *both* directions, deep-equal decodes and throw-parity down to the error
message:

* all **336 `@dcl/ecs` message namespaces** (142 unique types) x 300 instances x
  4 environments (Buffer / no-Buffer x Long / no-Long) = **403,200** instances;
* the ESM `dist/` build of the same corpus, 244 namespaces x 100 = 24,400;
* all **41 namespaces of the two catalogues that are not `@dcl/ecs`** —
  `@dcl/rpc/dist/protocol/index.js` (11: the request/response/stream/port
  framing every byte of the data-layer socket passes through) and the vendored
  22-method data-layer descriptor `@dcl/inspector/data-layer.gen.js` (30) — x
  2000 x the same 4 environments = **328,000** instances, plus an assertion
  that every one of the 22 methods' request and response types was among them;
* **800,000 fuzz operations** on random, truncated and corrupted input, where
  the two implementations must agree on *which* inputs throw.

**Zero divergences, six seeds** (`0xC0FFEE`, `0x1`, `0xDEADBEEF`, `0x5EED`,
`0xBADF00D`, `0x2A`). Mutation-tested: 13 injected wire-format bugs, 13 caught —
and the rpc/descriptor phase independently catches two of them
(`varint-length-boundary-off-by-one`, `fixed32-byte-order`) that the `@dcl/ecs`
corpus phase misses, so it is not riding on the older phase.

Two upstream inconsistencies are reproduced **on purpose** and must not be
"fixed": `BufferWriter.string` and `Writer.string` encode lone surrogates
differently, and `BufferReader.string` clamps a truncated length with `Math.min`
where `Reader.string` throws. The suite pairs like for like.

**The scene runtime uses it too.** `swap_pbmin_into_tree()` redirects the
install tree's `minimal.js` at the replacement *before* `build_chunks()`
resolves it, so `prebuilt/core.js` bundles this codec and every scene runs it
inside QuickJS. `check_chunk_pbmin()` fails the build if the marker
`dcl-one-sdk-pbmin.1` ever stops appearing in that chunk — the regression it
guards is silent, since the swap works by file overwrite and restores upstream
merely by being skipped.

So there is exactly one protobuf codec in the toolchain. There used to be two:
this one for the node consumers — `@dcl/ecs/dist-cjs` (69 files, the CRDT wire
format), `@dcl/rpc` (6 files, the socket framing) and the data-layer descriptor
(1 file) — and upstream's, minified inside the chunk, for the scene runtime.

Say the cost plainly: **QuickJS is still the axis the suite does not cover** (it
is all node 24 / V8), and now that axis is load-bearing, where before it was
only reached by upstream's copy. That is a genuine risk increase, accepted
because the alternative was two implementations of one wire format with the
untested, unowned one on the path that runs in production.

The behaviour difference — same class as the `typescript` prune, one step
further — is a scene that imports `protobufjs` *itself*, the reflection library
rather than the wire codec. That scene must declare protobufjs in its own
`package.json` (the scaffold does not), so `npm install` would hand it a full
private copy anyway; and the shipped manifest reads
`"version": "7.2.4-dcl-one-sdk-pbmin.1"`, so `npm ls` in an extracted scene
cannot mistake this for upstream 7.2.4.

**`ethers` is no longer vendored.** Earlier revisions of this file called it
"imported by `@dcl/sdk/ethereum-provider` but undeclared in its manifest". It is
not imported. A specifier scan of every `.js`/`.ts` in the blob outside
`ethers/` itself returns exactly three hits, and all three are the same line of
one JSDoc comment (`@dcl/sdk/ethereum-provider/index.js:8`, its `.d.ts`, and its
`src/` original):

    * import { ethers } from 'ethers'

Neither `@dcl/sdk`'s manifest nor the scaffold's `package.json` declares
`ethers`, so `npm install` in a scaffolded scene never installed it either —
shipping it was the divergence. Dropping it and the four packages only it
reached (`@adraffy/ens-normalize`, `aes-js`, `@noble/curves`, `@noble/hashes`)
removed 13.0 MB. A scene that wants ethers adds it to its own `package.json`,
exactly as in the npm flow. A verification scene importing
`@dcl/sdk/ethereum-provider` still bundles and type-checks without it.
`build-base-blob.py` now strips comments before scanning, so this class of
phantom dependency cannot come back.

## `@dcl/inspector`: the protocol, not the UI

`templates/data-layer-host.mjs` needs `@dcl/inspector` for two jobs.
`inspector-shim/` does both; what it does not do is carry the browser editor.

**What ships (0.18 MB).** Split by file:

| file | what it is |
| --- | --- |
| `engine-to-composite.js` | verbatim ports of `dumpEngineToCrdtCommands` (what `dump-crdt`, build step 4/5, calls) and `dumpEngineToComposite` (what `save()` calls), both from upstream `src/lib/data-layer/host/utils/engine-to-composite.ts` |
| `engine.js` | upstream `host/utils/engine.ts` — an `@dcl/ecs` engine with the editor's whole component set defined on it |
| `host.js` | the data-layer host: 4 of 22 rpc methods live, 18 inert stubs |
| `data-layer.gen.js` | the service descriptor, transpiled from upstream's protoc output at blob-build time |
| `component-schemas.json` | 57 component schemas, a pinned snapshot of a real `@dcl/inspector` |
| `minimal-composite.json` | the `assets/scene/main.composite` written at boot when a scene has none |

**The four live methods** are `crdtStream` (the engine ⟷ editor wire, both
directions), `save`, `getInspectorPreferences` and `setInspectorPreferences`.
That is enough for a live editing session: an editor connects, receives the
whole engine state as the first stream message, pushes CRDT edits, and the host
writes them back to `assets/scene/main.composite` — which the preview server's
watcher turns into `main.crdt`.

The other 18 return well-formed empty responses. They are not optional:
`@dcl/rpc`'s `codegen.registerService` binds *every* key in the descriptor, so
a missing one is a `TypeError` that kills the connection, and
`serverProcedureUnary` rejects a falsy result outright. Against a real browser
editor the stubs mean an empty asset panel, inert drag-and-drop import and dead
undo/redo — reachable and empty, never throwing, and truthful about failing
(`removeFiles` reports every path in `failed` rather than claiming success).
Deliberately not ported: undo/redo (a 978-line state machine with 100 ms
transaction batching), the 8 load-time migrations in `composite-provider.ts`
(we pass composites through verbatim — a `SceneMetadata-v4` stays v4 and
Creator Hub migrates it when it next opens the scene, which is the safe
direction), and `SceneProvider`, so **scene-metadata edits made against this
host do not reach `scene.json`**.

**What does not ship: the editor UI.** `inspector.zip` (7.36 MB zipped /
10.4 MB unpacked), `init --inspector`, `install_vendored_inspector()` and
`scripts/build-inspector-blob.py` were all removed. `start --data-layer` runs
the vendored host and serves `/data-layer`; `/inspector/*` answers 404 with the
install line until the package comes from `npm install --save-dev
@dcl/inspector` or `DCL_ONE_INSPECTOR_DIR`. That split is deliberate — a
missing UI used to be *fatal* to `--data-layer`, which meant the vendored host
could only ever start in the one case where a real `@dcl/inspector` was
installed, and in that case the real one wins at `require()` time. The fallback
was unreachable.

A real `@dcl/inspector` in the scene is still preferred for everything:
`req()` in the template resolves against the scene first. This is a fallback,
not a takeover.

### The two pinned snapshots, and how they go stale

`component-schemas.json` and `minimal-composite.json` are generated by
`scripts/dump-inspector-tables.cjs` from an installed `@dcl/inspector`
(currently **7.36.3**) — they cannot be derived from the blob, because the
`inspector::*` schemas live in creator-hub's own source and the
`asset-packs::*` ones come from `@dcl/asset-packs`, which the blob installs and
then drops. **Re-run that script on every inspector bump.**

Why a table at all: `@dcl/ecs`'s CRDT receive loop
(`dist-cjs/systems/crdt/index.js`) looks up `getComponentOrNull(componentId)`
and, when that is null, only re-broadcasts the message — the value never enters
the engine, so the next `save()` cannot see it. An editor writing a component
the host never defined would have its edit accepted, echoed back, and then
**silently dropped**. Going stale is therefore a data-loss bug, not a feature
gap.

Two details in the table are load-bearing:

* **`defineFrom`.** 26 components come from `@dcl/ecs`'s own factories and 31
  from `Schemas.fromJson(jsonSchema)`. It is not a name-prefix rule: the
  generator probes each factory and compares `jsonSchema`. `core::Material`'s
  schema is `{serializationType: 'protocol-buffer'}` — not rebuildable from
  JSON, so the factory is mandatory — while `core::ParticleSystem` *looks* core
  and newer `@dcl/ecs` releases do ship a factory for it, but the inspector
  defines it from its own `ParticleSystemSchema`, so the factory is wrong there.
* **Definition order.** `dumpEngineToComposite` walks `componentsIter()`, so
  definition order is the order component groups land in the saved file.
  Matching upstream keeps scenes from churning in git. Verified: the `gather`
  scene's 201,674-byte `main.composite` (26 component groups, including a
  scene-local `cube-id` component) loads and dumps **byte-identical**, order
  included.

### `@dcl/rpc` and `mitt`

`@dcl/rpc` is the wire protocol the data layer speaks — port multiplexing,
request/response framing, bidirectional streaming — vendored rather than
reimplemented. Pruned from 66 files / 0.31 MB to **14 files / 0.11 MB** by a
`require()` walk from the three entry points anything names
(`@dcl/rpc`, `@dcl/rpc/dist/codegen`, `@dcl/rpc/dist/transports/WebSocket`)
plus `dist/push-channel` for its `AsyncQueue`. Its entire runtime `require()`
surface is `mitt` and `protobufjs/minimal`; `ts-proto` is in its
`dependencies` and is never required at runtime. What goes: every `.d.ts` and
`.js.map`, `dist/rpc.api.json` (46,818 B of api-extractor report), and three
unreachable runtime files — `dist/transports/{Memory,WebWorker}.js` and
`dist/codegen-types.js` (a 118 B `__esModule` marker for a declarations-only
module).

`@well-known-components/pushable-channel` is **not** vendored even though
upstream's `stream.ts` imports `AsyncQueue` from it:
`@dcl/rpc/dist/push-channel` already exports an `AsyncQueue` of the same shape.

`mitt` was in `DROP_PACKAGES` (its runtime is inside the prebuilt chunks) and
had to come back, because `@dcl/rpc` requires it in **node**, outside every
chunk. Only `dist/mitt.js` ships — 349 B — and the `.d.ts` still does not, so
the rollup's ambient `declare module "mitt"` keeps owning the types. Same trick
as `long`.

### The service descriptor

`codegen.registerService` needs a descriptor: 22 methods, each with a name,
stream flags and a request/response type that can `encode`/`decode`. Upstream
generates it from
`packages/inspector/src/lib/data-layer/proto/data-layer.proto` with
`protoc-gen-dcl_ts_proto` — a plugin from **`@dcl/ts-proto`, a DCL fork**, not
the `ts-proto` on npm.

We vendor the *output*. `inspector-shim/data-layer.gen.ts` (75,703 B) is
checked in verbatim beside the `.proto` it came from; neither ships.
`build_service_descriptor()` in `build-base-blob.py` transpiles the `.ts` to
83,418 B of CommonJS with the already-vendored `typescript` and fails the build
unless the result still declares 22 methods. `--noCheck` makes that a
transpile, not a type check: the file's only imports are `long` and
`protobufjs/minimal`, whose `.d.ts` this blob deliberately drops.

Reproducing the codegen toolchain instead would mean vendoring a ~5 MB Node
plugin plus `ts-proto-descriptors` / `case-anything` / `dprint-node` to
regenerate a file that changes about once a year (the proto's last edit is
2025-11-13, creator-hub `36c4130`). Hand-writing the descriptor is technically
possible — `codegen.js` reads only five fields per method — but would be 22
hand-maintained message codecs matching a fork's exact wire choices
(`oneof=unions`, `useMapType=true`), for no size win over 83 KB.

### Autosave

`getInspectorPreferences` reports upstream's defaults,
`{freeCameraInvertRotation: false, autosaveEnabled: true}`, persisted at
`inspector-preferences.json` as `{"version": 1, "data": {...}}`. Reporting
`autosaveEnabled: true` and only ever writing on an explicit `save()` would
lose an editor's work, so `host.js` implements it: a trailing-edge debounce
over engine changes with upstream's own 100 ms floor. Upstream drives the same
guarantee from its state manager's transaction boundaries, which we do not
have.

`ajv` is not vendored for the preferences file. The JTD schema is two optional
booleans; the checks are inlined and every parse failure falls back to the
defaults, which is what upstream does with an `InvalidPreferences` too.

It was dropped because the weight is upstream build output we cannot fix from
here, and it is most of the package:

* `public/bundle.js` is 18.0 MB, and **~10.8 MB of that is Babylon.js**, bundled
  whole rather than tree-shaken. The bundle carries a barrel re-export map of
  **2,037 Babylon symbols** — including 359 NodeMaterial shader-graph blocks, 61
  WebXR symbols, post-processing, particles, sprites and 929 GLSL fragments —
  because the import is a namespace import (`BABYLON.` appears 533 times), which
  defeats `@babylonjs/core`'s own side-effect-free tree-shaking. An editor
  viewport genuinely needs a WebGL engine for its gizmos
  (`PositionGizmo`/`RotationGizmo`/`ScaleGizmo`/`GizmoManager`) and glTF
  loading; it does not need VR session management.
* **2,967 Font Awesome icon records, 1.62 MB of raw SVG path data** — the whole
  solid/regular/brand sets, where an editor uses a few dozen.
* `public/bundle.css` was 5.88 MB of which 86.4% was `url(data:...)`, and
  `public/bin/index.js` was 10.9 MB the browser never requests (proven with a
  CDP request log of a full editing session).

Both fixes — named imports instead of a namespace import, and a lazily-loaded
viewport chunk — are ordinary and belong upstream. `docs/upstream/` carries the
reports. Revisit vendoring the editor if upstream slims it, or if the Bevy
`scene_inspector` (`bevy-explorer/crates/scene_inspector`, which already reads
and writes composites) becomes the editing path.


### The scaffold now requires node >= 24 — a support-policy change

`templates/init/scene/package.json` and
`templates/init/smart-wearable/package.json` declare `engines.node ">=24"`
(and `npm ">=11"`, which is what node 24 ships). 24 is the version this
toolchain is built and tested against.

The blob itself no longer constrains the floor. It used to: the vendored
`@dcl/inspector` host did `require("node-fetch")` at module scope on an ESM-only
`node-fetch` 3.x, which is legal only under node's `require(esm)` support
(unflagged in **20.19.0 / 22.12.0**), so `start --data-layer` genuinely could not
run below that. Reproduced at the time with:

    Error [ERR_REQUIRE_ESM]: require() of ES Module .../node-fetch/src/index.js
    from .../@dcl/inspector/dist/tooling-entrypoint.js not supported.

That was an inherited defect, never ours — and it left with the package. The
data-layer host is now our own, built on the already-vendored `@dcl/ecs`, and it
requires no `node-fetch` at all.

What the scaffold used to claim, `">=16"`, was simply **false** while that host
shipped: `start --data-layer` had never worked on node 16 or 18. The number is
now a support statement rather than a hard constraint, so keep it honest — 24 is
what CI and this machine build and test against. Lowering it means testing
there, not editing the field.

After a rebuild, `start --data-layer` must print the `editor:` line and
`HTTP 101` on `/data-layer` (`/inspector/` is `HTTP 200` only when the scene
has its own `@dcl/inspector`, `HTTP 404` otherwise). The cheap end-to-end check
is `cargo test -p dcl-one-sdk --test data_layer_rpc` with
`DCL_ONE_SDK_TEST_NODE_MODULES` pointed at an extracted blob: it drives a real
node RPC client through boot-composite → initial CRDT state → edit → save →
`SCENE_UPDATE` → `main.crdt`, against the blob alone. The stronger check, with
a real browser and a real editor UI, is
`scripts/creator-hub-ui-drive.sh <scene> <evidence-dir>`.

### crdt parity of the stand-in

Measured over the 16 composite scenes in `decentraland/sdk7-test-scenes`: 3
byte-identical to upstream, 10 identical in every component except the
editor-only `composite::root` marker (the sdk-commands data layer sets that when
it opens a composite; scene content — Transform, Material, MeshRenderer,
MeshCollider, Name — matches exactly), 2 with no committed crdt to compare
against, 1 that cannot build for unrelated reasons (missing third-party dep).

## Regenerating

Run `python3 scripts/build-base-blob.py`. It does all of the below; the steps
are recorded so the script stays auditable.

1. Empty dir, `corepack pnpm add --ignore-scripts --config.node-linker=hoisted`
   the scaffold's `devDependencies` verbatim plus the undeclared-but-real
   imports (`protobufjs@7.2.4`, `@protobufjs/utf8`, `ws`, `@dcl/rpc`). The
   `--config.node-linker=hoisted` **flag** is required: an `.npmrc` with the
   same setting is silently ignored by pnpm 11, which yields a `.pnpm` tree
   where nothing resolves.
2. Optionally overlay toolchain packages: `npm pack` each of
   `packages/@dcl/{sdk,ecs,react-ecs,js-runtime}` in the toolchain worktree,
   extract each tarball over `node_modules/@dcl/<name>`, then rewrite the four
   manifests: `version` to the scaffold pin and any `file:` dep specs to that same
   version.
3. Prune by **code** reachability, not manifest reachability: BFS the bare
   specifiers found in the `.js`/`.d.ts` that actually ship, starting from
   `@dcl/sdk`, `@dcl/js-runtime`, `typescript`, `protobufjs`,
   `@protobufjs/utf8` and `ws`. Declarations count — `build --production`
   type-checks the scene, and tsc follows `.d.ts` imports exactly like node
   follows `require`. This is strictly better than the old manifest BFS: it
   drops `loose-envify`/`js-tokens` (react 18 declares them; nothing imports
   them) and it never invents `ethers`. Comments are stripped before scanning,
   and a specifier naming a node builtin is not treated as a package — `ws`
   requires `'buffer'`, which is node's, not npm's.
4. Prune inside kept packages. Two allowlists. `protobufjs` is cut to *nothing*
   — `add_pbmin()` writes the package instead (see above), and the six
   `@protobufjs/*` micro-packages it was the sole importer of then leave the
   closure on their own, the way `@protobufjs/{codegen,path,fetch}` already
   had. The larger allowlist is `typescript`:
   `build.rs` runs exactly `node typescript/lib/tsc.js -p tsconfig.json
   --noEmit`, and that run under a node with `Module._resolveFilename` and the
   `fs` read family hooked touches precisely `lib/tsc.js`, the `lib/_tsc.js` it
   requires, `package.json`, and 45 `lib/lib.*.d.ts`. So `lib/typescript.js`
   (8.7 MB, the programmatic API), `typescript.d.ts`, every `tsserver*` file
   and the 13 locale directories (4.2 MB) all go — 22.5 MB → 9.6 MB. Locales
   are safe to drop: with `lib/ja/` present `tsc --locale ja` prints Japanese,
   with every locale directory deleted the same command exits 0 and prints
   English.
5. Delete symlinks (`find node_modules -type l -delete`), source maps
   (`find node_modules -name '*.map' -delete`), `node_modules/.bin` and
   `node_modules/.package-lock.json` — symlinks break Windows extraction, and a
   stale lockfile misleads later `npm install`s. Nested `node_modules` go too:
   everything kept resolves at top level, and step 6 proves it.
6. Re-scan every kept file and resolve each bare specifier against the tree.
   **The scan must come back empty**; the script exits non-zero otherwise. This
   is what catches an undeclared import before it becomes a runtime failure.
   Then zip deterministically (sorted paths, fixed timestamps, deflate per
   entry — python `ZipInfo` defaults to STORED) as `node_modules/...` entries
   at the archive root. Base-blob files are `require`d from disk and read by
   tsc, so unlike `inspector.zip`'s browser bundles they cannot be shipped
   gzipped — pruning and dedupe only.
7. Prove it before committing: `dcl-one-sdk init` in an empty dir, then
   `build --production` (rolldown + type check) with a scene importing
   `@dcl/sdk/players` and `@dcl/sdk/network`, then `start` and probe `/about`.
   A `.composite` scene must still reach `[4/5] main.crdt regenerated`, which
   is the test that the `@dcl/inspector` stand-in survived the rebuild; and
   `cargo test --test data_layer_rpc` with `DCL_ONE_SDK_TEST_NODE_MODULES` set
   to the new tree is the test that its data-layer half did.

When bumping the `@dcl/sdk` or `typescript` pin, update
`src/templates/init/*/package.json` **first** (and re-check
`src/templates/init/scene/tsconfig.json` and `docs/ts7-migration.md` against
the new compiler's deprecation table)
— `build-base-blob.py` reads its install list from the scaffold manifest, so
the scaffold is what drives the blob, and a later `npm install` in a scaffolded
scene converges instead of fighting it.

## Smart items (`@dcl/asset-packs`) are in the base blob — as chunk + rollup

`@dcl/asset-packs/dist/scene-entrypoint` is named two ways: a scene's own
source can import it (`0,0-cube-spawner` in decentraland/sdk7-test-scenes does,
with no composite at all), and `entrypoint.rs` injects the same import for any
scene whose composite carries `asset-packs::` components — which the scaffold's
does. Both paths resolve offline now, and **zero bytes of the package ship**:

* the **runtime** is inside `prebuilt/smart.js` (150,045 B), which publishes the
  registry keys `@dcl/asset-packs` and
  `@dcl/asset-packs/dist/scene-entrypoint`; `build.rs` installs that chunk only
  for a scene that is an editor scene or whose built chunk actually names the
  package, so a scene without smart items pays nothing.
* the **declarations** are inside the ambient rollup at
  `@dcl/js-runtime/index.d.ts` — 86 `declare module "@dcl/asset-packs/…"`
  blocks, entered by `ROLLUP_PKGS` + `ROLLUP_FULL_SUBPATH_PKGS` in
  `scripts/build-base-blob.py` (the package has no `exports` map, so every
  `.d.ts` under `dist/` is a public subpath and every one of them is an entry
  point of the closure walk).

Verified end to end against the *vendored* tree, not a scene's own
`node_modules`: `init --node-modules-only` into a copy of `0,0-cube-spawner`,
then `build --production`, gives `[2/5] Scene chunk saved` and `[5/5] Type check
passed`, with `bin/scene.js` emitting
`require("@dcl/asset-packs/dist/scene-entrypoint")` and `bin/sdk-smart-items.js`
serving it. The same on `3,2-proximity-interactions` (a composite scene) also
reaches `[4/5] main.crdt regenerated (1 composite)`.

**Do not add the `.d.ts` closure to the blob as files.** It was measured — the
package's declarations are 90,874 B against 1,286,817 B of runtime `.js`, so
0.09 MB looks cheap — but it buys nothing the rollup does not already provide
and it actively regresses two things. A `node_modules/@dcl/asset-packs` with a
manifest whose `typings` resolves would make tsc prefer the on-disk tree over
the ambient declarations, which is precisely the failure mode `DROP_PACKAGES`
exists to prevent; and shipping the tree wholesale (rather than the rollup's
reachability walk) drags in declarations nothing public re-exports — the
`admin-toolkit-ui/**` half of the package — the same class of trap that
`@dcl/react-ecs/dist/reconciler/types.d.ts` and its DOM `Document` reference
already sprang once. The TS7016 hazard the old `inspector.zip` note warned
about is real, and the rollup is where it is handled: drop a `.d.ts` the closure
needs and `build_types_rollup()` fails the build with unresolved relative
specifiers before the zip is written.

The one thing the base blob still does not carry is the *visual editor's UI*
(`@dcl/inspector`'s `public/`). Its **protocol** half is here — see the
stand-in section above — so `start --data-layer` works offline and only
`/inspector/*` needs a scene with its own `npm install`.
