# TypeScript 6 / 7 migration

Status: the scaffold half is done. Two options still have to be fixed in
`@dcl/sdk`'s own `tsconfig.ecs7.json`, which we ship inside
`src/vendor/node_modules.zip` but do not author.

Everything below is a genuine option fix. `"ignoreDeprecations": "6.0"` is
explicitly rejected as a solution — see "Why not ignoreDeprecations".

## The four options

| option | where it is set | status | fixed by |
| --- | --- | --- | --- |
| `baseUrl` | our scaffold `tsconfig.json` | fixed | deleted |
| `moduleResolution=node10` | upstream `tsconfig.ecs7.json` | fixed for scaffolded scenes (overridden), still pending upstream for scenes that resolve `@dcl/sdk` from npm | scaffold sets `"moduleResolution": "bundler"` |
| `downlevelIteration` | upstream `tsconfig.ecs7.json` | pending upstream — nothing a child tsconfig can do | deletion from ecs7 |
| `suppressExcessPropertyErrors` | upstream `tsconfig.ecs7.json` | pending upstream (TS 6 accepts it, TS 7 rejects it as unknown) | deletion from ecs7 |

## What we changed (done)

`src/templates/init/scene/tsconfig.json` — one template, written for both
`ProjectKind::Scene` and `ProjectKind::SmartWearable` (`src/init.rs`
`SCENE_TSCONFIG`, used at both write sites), so there is no second scaffold
tsconfig to update:

```diff
     "strict": true,
-    "baseUrl": "."
+    "moduleResolution": "bundler"
```

* `baseUrl` deleted, not replaced. It only adds a "resolve this
  non-relative specifier against the project root" step. No template and no
  real scene uses root-relative imports, and we set no `paths`, so removing it
  changes nothing: across the 60-scene `sdk7-test-scenes` corpus, `tsc
  --listFiles` and every diagnostic were identical with and without it, and the
  emitted `bin/*.js` / `main.crdt` were byte-identical.
  `@dcl/asset-packs` needs no `paths` mapping here — unlike the upstream
  `sdk7-scene-template`, `inspector.zip` installs it at top-level
  `node_modules/@dcl/asset-packs`, so plain node resolution finds it (verified:
  `import { ComponentName } from '@dcl/asset-packs/dist/definitions'` type-checks
  clean with no `baseUrl` and no `paths`).
* `moduleResolution: "bundler"` added. A child `compilerOptions` overrides
  `extends`, so this replaces the inherited `node10` without touching the blob.
  `node16`/`nodenext` are not usable: TS5110 requires `module` to be
  `Node16`/`NodeNext`, and `module` is `esnext` from the SDK config. `bundler`
  is also what the build actually does — rolldown/oxc resolves the bundle. The
  only defaults that flip are `resolvePackageJsonExports`,
  `resolvePackageJsonImports` and `resolveJsonModule`; none of the packages a
  scene imports declares `exports`/`imports`, and the scaffold already sets
  `resolveJsonModule: true`.
* Keep this line even after the ecs7 fix below lands: it is what protects a
  scene that resolves `@dcl/sdk` from npm rather than from the blob.

If a scene ever does need `paths` (e.g. the upstream asset-packs mapping), the
targets must be relative once `baseUrl` is gone — `"@dcl/asset-packs/*":
["./node_modules/.../@dcl/asset-packs/*"]`. A non-relative target without
`baseUrl` is a hard `TS5090` on every TypeScript version.

## What upstream must change (pending)

File: `node_modules/@dcl/sdk/types/tsconfig.ecs7.json`, shipped inside
`src/vendor/node_modules.zip` (@dcl/sdk 7.26.0). Upstream home:
`decentraland/js-sdk-toolchain`, `packages/@dcl/sdk/types/tsconfig.ecs7.json`.
`tsconfig.ecs7.strict.json` needs nothing — it is
`{"compilerOptions":{},"extends":"./tsconfig.ecs7.json"}`.

```diff
--- a/packages/@dcl/sdk/types/tsconfig.ecs7.json
+++ b/packages/@dcl/sdk/types/tsconfig.ecs7.json
@@
     "module": "esnext",
-    "moduleResolution": "node",
+    "moduleResolution": "bundler",
     "pretty": true,
     "forceConsistentCasingInFileNames": true,
     "allowSyntheticDefaultImports": true,
     "experimentalDecorators": true,
-    "downlevelIteration": true,
-    "suppressExcessPropertyErrors": false,
     "exactOptionalPropertyTypes": false,
```

Why each line is safe:

* `downlevelIteration` is emit-only and only has an effect when
  `target < ES2015`. The same file sets `"target": "es2020"`, and
  `src/rolldown_backend.rs` transforms at a hardcoded `es2020` regardless of the
  tsconfig, so the flag cannot reach a shipped byte. On top of that
  `src/build.rs` runs `tsc -p tsconfig.json --noEmit`, so tsc never emits at
  all. Removing it left every corpus bundle byte-identical and produced no new
  diagnostic anywhere. It cannot be neutralised from a child tsconfig: TS
  reports the deprecation on *presence*, so `"downlevelIteration": false` still
  errors (it just moves the error to our line).
* `suppressExcessPropertyErrors` is set to the compiler's own default
  (`false`) and has been inert in the checker since TS 5.5 — the only code in
  tsc 6.0.3 that reads it is the deprecation check. Excess-property checking is
  demonstrably still on with the key removed. Do **not** "fix" it by setting it
  to `true`: that is TS5102 (removed option), which `ignoreDeprecations` cannot
  silence.
* `moduleResolution` — same rationale as the scaffold override; this arm is
  what fixes scenes that already exist and never get a new tsconfig.

Because the blob is a pure registry install, this must land either upstream (so
a re-vendor picks it up) or as an explicit overlay step in
`scripts/build-base-blob.py`, applied next to the other post-extract rewrites,
with a guard so a regen fails loudly if upstream has already fixed it:

```python
ECS7 = 'node_modules/@dcl/sdk/types/tsconfig.ecs7.json'

def patch_ecs7_tsconfig(files: dict[str, bytes]) -> None:
    """Drop the two options TS 7 removes and modernise moduleResolution.

    downlevelIteration is emit-only and dead at target es2020 (set two lines
    above it) and under our `tsc --noEmit`; suppressExcessPropertyErrors is
    the compiler default and inert since TS 5.5. TS 6 errors TS5101/TS5107 on
    them, TS 7 removes them outright (TS5102/TS5023/TS5108) and
    `ignoreDeprecations` cannot silence a removed option.
    """
    raw = files[ECS7].decode('utf-8')
    out = (raw
           .replace('    "downlevelIteration": true,\n', '', 1)
           .replace('    "suppressExcessPropertyErrors": false,\n', '', 1)
           .replace('"moduleResolution": "node"', '"moduleResolution": "bundler"', 1))
    assert out != raw, 'ecs7 tsconfig already patched upstream — drop this overlay'
    json.loads(out)
    files[ECS7] = out.encode('utf-8')
```

This makes the blob no longer "a pure registry install — no overlays"; that
sentence in `src/vendor/README.md` has to change when the overlay lands.

## Why not `ignoreDeprecations`

* It is a mute, not a fix: under TypeScript 7.0.2 all four options are
  **removed**, not deprecated (`TS5102` for `downlevelIteration` and `baseUrl`,
  `TS5108` for `moduleResolution=node10`, `TS5023` for
  `suppressExcessPropertyErrors`), and `ignoreDeprecations` — which is still a
  valid option in TS 7 — cannot silence any of them.
* It breaks the version we are migrating *from*: TypeScript 5.9.3 rejects
  `"ignoreDeprecations": "6.0"` with `TS5103: Invalid value for
  '--ignoreDeprecations'`, so a scaffold carrying it stops building for anyone
  still on the 5.9.3 pin.

## TypeScript 7 is still not reachable from this toolchain

Independent of the tsconfig: TS 7 is the Go port. `lib/tsc.js` is an ESM shim
that resolves a per-platform native package (`@typescript/typescript-darwin-arm64`
and friends, ~23 MB each) which also carries all `lib.*.d.ts`. Vendoring it
would end the one-zip-serves-every-OS property of `node_modules.zip`
(`scripts/build-base-blob.py` says as much: 6.x is the ceiling until that is
addressed). So the pin stays at 6.0.3; this document is about being *correct*
for 7.0, not about shipping it.

Two further gaps, for whoever picks up TS 7 later:

* A scene that runs a real `npm install` gets the unpatched `@dcl/sdk` and will
  still hit `TS5101`/`TS5102` for `downlevelIteration` and `TS5023` for
  `suppressExcessPropertyErrors`. Only the upstream change closes that.
* Scenes already scaffolded by an older `dcl-one-sdk` keep the `baseUrl` on
  disk — `init` only writes `tsconfig.json` once and nothing rewrites it. Under
  TS 7 they get `TS5102 ... Use '"paths": {"*": ["./*"]}' instead.` If we ever
  move the pin to 7, they need a migration step (delete `baseUrl`, and relativise
  any `paths` targets).

## Verification

Fresh `dcl-one-sdk init --project scene`, `node_modules` from the current
`src/vendor/node_modules.zip` (@dcl/sdk 7.26.0), driven through
`dcl-one-sdk build --dir <scene> --production`. No `ignoreDeprecations`
anywhere.

| arm | scaffold tsconfig | ecs7 | result |
| --- | --- | --- | --- |
| TS 6.0.3 control | previous (`baseUrl`) | upstream | `type check failed — 3 errors`: TS5101 `downlevelIteration`, TS5107 `moduleResolution=node10`, TS5101 `baseUrl` |
| TS 6.0.3 | committed | upstream | `type check failed — 1 error`: TS5101 `downlevelIteration` (the pending upstream fix) |
| TS 6.0.3 | committed | patched per this doc | `[5/5] Type check passed` |
| TS 5.9.3 | committed | upstream | `[5/5] Type check passed` |
| TS 7.0.2 | committed | patched per this doc | `tsc --noEmit` exit 0, zero diagnostics |
| TS 7.0.2 | committed | upstream | TS5023 `suppressExcessPropertyErrors`, TS5102 `downlevelIteration` (no `baseUrl`/`node10` error) |

Wider evidence behind the two scaffold changes: the 60-scene
`decentraland/sdk7-test-scenes` corpus under both 5.9.3 and 6.0.3, comparing
`--listFiles` resolution graphs, full diagnostic text, and sha256 of every
emitted artifact — no scene gained or lost a diagnostic and no bundle byte
changed.
