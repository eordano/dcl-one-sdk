# dcl-one-sdk golden scene snapshots

A whole built scene, rendered as text and compared byte for byte. Modelled on
upstream js-sdk-toolchain's golden runner, so a reviewer who knows those files
reads these for free: same line names, same CRDT serializer, same short frame
loop, same `UPDATE_*` regeneration habit.

* Test target: `crates/dcl-one-sdk/tests/golden.rs` (8 tests, one per golden).
* Fixtures: `crates/dcl-one-sdk/testdata/golden/<fixture>/` — scene-owned files
  only. No `node_modules`, no `bin/`.
* Goldens: `crates/dcl-one-sdk/testdata/golden/<fixture>.<mode>.golden`, LF-only.
* Runtime harness: `crates/dcl-one-sdk/scripts/golden-runtime.mjs`.
* Regenerate: `UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden`. (In the
  upstream source tree `scripts/update-goldens.sh` at the workspace root wraps
  exactly that and prints the diffstat; that wrapper lives outside this crate
  and is not part of the published tree.)

Cost: ~1.2 s for all eight, offline. `init --node-modules-only` extracts the
vendored blob once per test binary (~75 ms); each fixture then gets a private
copy of itself with that tree symlinked in.

## The two tiers

**STATIC**, in-process Rust. `init` the blob, overlay a fixture, call
`build::build` (this is the first in-process caller in the repo), read the
artifacts back off disk, and run `deploy::prepare` with `timestamp = 0`. The
build's own stdout is deliberately *not* captured: its step lines carry elapsed
times (`Scene chunk saved bin/scene.js (4.99 ms)`) and a timing must never
reach a golden.

**RUNTIME**, one `node` process per fixture. `golden-runtime.mjs` loads the
scene's `main` in a `new Function` CommonJS sandbox whose only door outward is
an injected `require` over a fixed `~system/*` table, then runs `onStart()` and
four frames (0.0, 0.1, 0.1, 0.1). CRDT buffers are decoded with the *scene's
own* vendored `@dcl/ecs/dist-cjs`, so a component id that copy does not know
prints `data=null` exactly as upstream's goldens show it.

node is a hard dependency of this harness, and already a hard dependency of
`build` (the type check, and the `@dcl/inspector` data-layer fallback for
composites carrying their own `jsonSchema`). Missing node fails the test unless
`ALLOW_SKIPPED_INTEGRATION` is set, per workspace convention.

## Line grammar

| line | meaning |
|---|---|
| `(dcl-one-sdk golden v1 <fixture> <mode>)` | header; bump `v1` on a format change |
| `TOOLCHAIN sdk= ecs= bundler= blob=` | upstream's `(start empty vm <version>)` role: one place a blob or bundler bump self-invalidates every golden |
| `SCENE_COMPILED_JS_SIZE_PROD=<k>k bytes` | upstream's exact spelling and unit (thousands of bytes); sum of every emitted JS chunk, sourcemaps excluded. `_DEV` in development mode |
| `THE BUNDLE HAS SOURCEMAPS` + `  SOURCE:` | dev only; upstream's literal line, then the map's `sources` array |
| `  ARTIFACT <rel> <bytes> sha256=<16 hex>` | exact bytes, not upstream's 0.1k rounding — our output *is* byte-stable, so exact is both honest and more sensitive |
| `LOADER sdk= smart= scene= max_composite_entity=` | the split loader's four substituted variables; the 5.8 KB template itself is covered by the `ARTIFACT bin/index.js` hash |
| `ENTRYPOINT … main= composites= script-utils=` | then `.dcl-one/entrypoint.ts` verbatim, indented, absolute scene root replaced by `<SCENE>` |
| `  REQUIRE(scene): <key>` | registry keys the emitted scene chunk resolves; sorted and deduped, because this is a set |
| `ENTITY_NAMES <rel>` | present only when the build generated one; the enum follows, indented |
| `MAIN_CRDT main.crdt <bytes> bytes sha256= (native, N composites\|node data-layer)` | which generator ran is re-derived from `crdt_gen::generate`, never scraped from stdout |
| `DEPLOY entity= files= total=` + per-file `<cid> <rel> <bytes> bytes` | `deploy::collect_publishable_files` order, which the golden therefore also pins |
| `EVAL <main>` + `  READFILE:` / `  REQUIRE:` | the runtime tier's bring-up: files the sandbox served, then `~system/*` ids it handed out in first-request order |
| `CALL onStart()` / `CALL onUpdate(<dt>)` | upstream's phases; CRDT lines emitted during a phase sit under it |
| `  main.crdt: <TYPE> e=0x<hex> c=<dec> t=<lamport> data=<JSON\|null>` | state fed in through `crdtGetState` — upstream's `<bundle>.js-main.crdt` trick, except the file is the one this build just produced |
| `  Scene: …` | messages out through `crdtSendToRenderer`. Wire order, never sorted |
| `  CONSOLE(<level>): <text>` | anything the scene wrote to `console`, under the phase that wrote it (module-eval output sits in the `EVAL` block). Stack frames — lines matching `    at ` — are stripped, and the scene root becomes `<SCENE>`; neither an absolute path nor an eval offset may reach a golden |
| `HOSTCALLS readFile= crdtGetState= crdtSendToRenderer= sendBatch=` | boundary-traffic counters |
| `CRDT_TRAFFIC messages= bytes=` | total outbound over the run |

`REQUIRE:` is an assertion, not a log: the mock table *throws* on a module id it
does not carry, so a scene reaching a new host module fails the suite instead of
quietly working. (`~system/CommsApi` was found exactly this way while building
the smart-item fixture.)

`CONSOLE(...)` is an assertion for the same reason, and it closes the one hole
exit status cannot: the generated startup catches everything `main()` throws and
reports it with `console.error(e.stack)`. A scene that throws on startup still
runs its four frames, still emits the same CRDT traffic, and still exits zero —
so if console output only went to a failure dump, the single failure this tier
exists to catch would produce a byte-identical golden. Routing it into the text
makes a newly-throwing scene a stale golden, which is a test failure.

## What is deliberately not here

**No CPU metric.** Upstream's `OPCODES`, `MALLOC_COUNT`, `ALIVE_OBJS_DELTA` and
`MEMORY_USAGE_COUNT` all come from decentraland's archived fork of
quickjs-emscripten, published only as an npm package. This toolchain has no npm
step by design — the whole thing is a 2.4 MB zip compiled into the binary — and
node's `process.memoryUsage()` is GC-timing dependent, which would make a flaky
golden. The cost axis here is exact bytes, deploy total, message counts and
host-call counts: boundary-traffic proxies. **A pure-CPU regression that changes
no bytes and no messages will not be caught.** If someone wants the real thing
later, the honest route is an opt-in `--features quickjs-golden` tier with a
vendored WASM blob, never the default suite.

**No `Renderer:` prefix.** Upstream echoes updates back into the scene through a
second fake engine. Nothing this harness exists to catch (bundler, entrypoint
generator, crdt encoder, SDK chunk) needs it, so the format simply leaves the
prefix free; a two-way fixture later needs no format change.

**No `.test.`-infix tier.** The vendored blob carries no testing framework, and
vendoring one purely to snapshot it would grow the blob for a tier nothing else
uses. Scene-authored assertions stay in the coverage tour.

**No DEPLOY section in development mode.** A dev bundle's inline sourcemap
base64-embeds the absolute scene root, so `bin/scene.js`'s CIDv1 — and with it
the entity id and the total — would be a function of where the temp dir landed.
The same leak is why every size and hash above strips the map first. Production
artifacts *are* path-independent: the cube fixture built at two paths of
different length produces identical `bin/scene.js` and `bin/index.js` hashes.

**No numeric comparability with upstream.** Different bundler (rolldown 1.2.4 vs
esbuild ^0.18.17), a split three-chunk layout instead of one bundle, and a
prebuilt scene-independent SDK chunk copied out of the blob rather than
re-bundled. Comparability here is structural: identical line names so a human
can lay the two files side by side.

**No `.tsx` entry fixture.** `entrypoint::generate` resolves exactly one user
entry, `src/index.ts` (src/entrypoint.rs:33), so `src/index.tsx` fails to
resolve. The `ui` fixture calls `ReactEcs.createElement` from a `.ts` entry
instead. That `.tsx` entries are unsupported is worth its own issue.

**No auto-write of a missing golden.** Upstream writes one and passes. This
repo's CI backstop is `scripts/test-count-gate.sh`, which counts passing tests
and never checks for a dirty tree, so a fixture added without its golden would
auto-write on the CI machine and report green having asserted nothing. Missing
is a hard failure that prints the full rendered text, the path of a
`<CARGO_TARGET_TMPDIR>/golden/<name>.actual` dump and the exact
regeneration command. For
the same reason the gate refuses to run at all with `UPDATE_GOLDEN` set, exactly
as it already refuses `ALLOW_SKIPPED_INTEGRATION`.

## The fixtures

| fixture | mode(s) | what it pins |
|---|---|---|
| `cube` | production, development | the baseline scene, and dev-vs-prod as a flag rather than a directory |
| `composite` | production | `assets/scene/main.composite` (the crdt encoder's own opera reference, staged in so there is one source of truth) through the native generator |
| `dynamic-import` | production | `code_splitting: false` inlining a dynamic import — and that the import lands on the *second* frame |
| `ui` | production | react-ecs: UiTransform (1050) and UiText (1052) traffic |
| `smart-item` | production | the smart-item chunk, the node data-layer main.crdt fallback, `entity-names.ts` regeneration, and `data=null` for asset-packs component ids |
| `custom-entry` | production | `--customEntryPoint`: scene.json's main bundled verbatim, no generated startup system |
| `no-main` | production | a scene that runs at module scope with no exported `main` |

`ENTRYPOINT … composites=0` on the `composite` and `smart-item` goldens is not
a bug and is worth knowing: `entrypoint::write_all_composites` only inlines a
composite found at the scene ROOT (`main.composite`). A Creator Hub scene's
`assets/scene/main.composite` reaches the runtime through `main.crdt` instead,
which is exactly what the `MAIN_CRDT` line and the `main.crdt:` messages pin.

The `smart-item` fixture ships a deliberately **stale** `entity-names.ts`
(carrying an entry the composite no longer names). The build regenerates it, so
the golden proves the generator ran rather than echoing a checked-in file — and
`src/index.ts` imports the enum, so a regression fails the type check too.

## Adding a fixture

1. `mkdir testdata/golden/<name>` with `scene.json`, `package.json`,
   `tsconfig.json`, `src/index.ts` and whatever assets it needs. Keep it small:
   a golden suite that adds megabytes to the repo is a failed port.
2. Add a `Fixture::new("<name>")` test to `tests/golden.rs`.
3. `UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden` and **read the
   diff**.
4. Raise the `dcl-one-sdk golden` floor in `tests-manifest.tsv` (the upstream
   workspace root; like `update-goldens.sh` it is not part of the published
   tree, so this step is for contributors working in the source checkout).
