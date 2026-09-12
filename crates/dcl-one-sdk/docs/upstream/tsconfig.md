# PR draft: make `@dcl/sdk/types/tsconfig.ecs7.json` TypeScript 6/7 clean

Target repo: `decentraland/js-sdk-toolchain`
File: `packages/@dcl/sdk/types/tsconfig.ecs7.json`
Status: draft, not filed. Everything below was re-verified from scratch on 2026-08-02.

## Summary

`packages/@dcl/sdk/types/tsconfig.ecs7.json` sets three compiler options TypeScript 6 deprecates and
TypeScript 7 removes. Every SDK7 scene `extends` this file, so under TS 7 a stock scene cannot
type-check — and for `suppressExcessPropertyErrors` cannot work around it from its own
`tsconfig.json` (see *Why this has to change in the shipped file*). All three are inert at the
`target: es2020` the same file sets, so removing them is a no-op for behaviour and emit.

## Environment

| | |
| --- | --- |
| host | macOS 25.5.0, arm64 |
| node | v24.18.0 |
| `@dcl/sdk` | 7.25.0 (npm) |
| typescript | 5.9.3 (2025-09-30), 6.0.3 (2026-04-16), 7.0.2 (2026-07-08, current `latest`) |

The npm-published `@dcl/sdk@7.25.0` copy of `types/tsconfig.ecs7.json` is byte-identical to the
repo's `packages/@dcl/sdk/types/tsconfig.ecs7.json` at `2d718be5` ("feat: replace legacy web
explorer preview with Bevy Web", 2026-07-28) — sha256
`5e416d8c99b4ee713fb691c7643113a7f3671dc2de217403ffeb91e231ffab5b` for both. `tsconfig.ecs7.strict.json`
needs no change; it is `{"compilerOptions":{},"extends":"./tsconfig.ecs7.json"}`.

## Reproduction

```sh
mkdir repro && cd repro
npm init -y
npm i -D @dcl/sdk@7.25.0
mkdir src && echo 'export const answer: number = 42' > src/index.ts
cat > tsconfig.json <<'EOF'
{ "extends": "@dcl/sdk/types/tsconfig.ecs7.json", "include": ["src"] }
EOF

npx -p typescript@5.9.3 tsc -p . --noEmit --pretty false   # exit 0
npx -p typescript@6.0.3 tsc -p . --noEmit --pretty false   # exit 2
npx -p typescript@7.0.2 tsc -p . --noEmit --pretty false   # exit 1
```

Nothing here is scene-specific: the diagnostics come from parsing the inherited config, so any file
under `include` will do.

## Expected vs actual

Expected: a config shipped by the SDK parses cleanly on the current TypeScript release.

Actual:

```
# typescript 5.9.3 — exit 0, no diagnostics

# typescript 6.0.3 — exit 2
error TS5101: Option 'downlevelIteration' is deprecated and will stop functioning in TypeScript 7.0.
              Specify compilerOption '"ignoreDeprecations": "6.0"' to silence this error.
error TS5107: Option 'moduleResolution=node10' is deprecated and will stop functioning in TypeScript 7.0.
              Specify compilerOption '"ignoreDeprecations": "6.0"' to silence this error.

# typescript 7.0.2 — exit 1
error TS5102: Option 'downlevelIteration' has been removed. Please remove it from your configuration.
error TS5108: Option 'moduleResolution=node10' has been removed. Please remove it from your configuration.
node_modules/@dcl/sdk/types/tsconfig.ecs7.json(11,5): error TS5023: Unknown compiler option 'suppressExcessPropertyErrors'.
```

`suppressExcessPropertyErrors` is silent on 5.9.3 and 6.0.3, failing only on 7.0.2, where the option
no longer exists.

## Why this has to change in the shipped file

Every child-config escape hatch, against the same reproduction and unmodified `@dcl/sdk@7.25.0`:

| child `compilerOptions` | 5.9.3 | 6.0.3 | 7.0.2 |
| --- | --- | --- | --- |
| *(none)* | clean | TS5101 + TS5107 | TS5102 + TS5108 + TS5023 |
| `"ignoreDeprecations": "6.0"` | **TS5103** invalid value | clean | TS5102 + TS5108 + TS5023 |
| `"downlevelIteration": false` | clean | TS5101 + TS5107 (re-reported at the child's own line) | TS5102 + TS5108 + TS5023 |
| `"moduleResolution": "bundler"` | clean | TS5101 | TS5102 + TS5023 |
| `"downlevelIteration": null, "moduleResolution": "bundler"` | clean | clean | **TS5023** |
| add `"suppressExcessPropertyErrors": null` to the above | clean | clean | **TS5023** |

Two consequences:

* TS 6 has a workaround (`ignoreDeprecations: "6.0"`, or a `null` override plus `moduleResolution`),
  but `ignoreDeprecations: "6.0"` is not portable: TypeScript 5.9.3 rejects the value with
  `TS5103: Invalid value for '--ignoreDeprecations'`, so a scene adding it stops building for
  everyone still on 5.x.
* TS 7 has none for `suppressExcessPropertyErrors`. `TS5023` is reported at
  `tsconfig.ecs7.json(11,5)` — the inherited file — and a child config cannot delete an inherited
  key; `null` there does not help. The only user-side escape is to stop extending the SDK config and
  inline a copy.

## Evidence that the three edits are behaviour-preserving

### `downlevelIteration` is dead at `target: es2020`

Three independent checks.

1. **Compiler source.** In `typescript@6.0.3` `lib/_tsc.js`, every checker-reachable read of
   `compilerOptions.downlevelIteration` is gated on language version below ES2015 (lines 73940,
   73974, 79669, 80460, 83459, 83831). The one ungated read, line 83920, is
   `const downlevelIteration = !uplevelIteration && compilerOptions.downlevelIteration`, with line
   83919 `const uplevelIteration = languageVersion >= 2 /* ES2015 */ && iterableExists` — at `es2020`
   with `lib: ["ES2020"]` it is forced `false` and only selects diagnostic wording. Remaining reads
   (93269, 93365, 93534, 106571, 107863, 107892) are in emit transformers.

2. **Emit probe with a sensitivity control.** A file exercising generators, `Map`/`Set` spread,
   array-rest destructuring, `Math.max(...set)`, `for...of` over an `Iterable`, destructuring
   `for...of` over a `Map`, and an async generator, compiled by tsc 6.0.3 with
   `--downlevelIteration true` vs `false`:

   * `--target es2020`: **identical**, sha256 `1aea3621e7085044fdada02b892f39d0cfab4f2548bf3186094d1dfc13254183` both ways.
   * `--target es5`: **different** (`f2ef2d24…` vs `56d660fa…`) — the control proving the probe is
     sensitive to the flag.

3. **The SDK package's own tsc build.** `packages/@dcl/sdk/package.json` has
   `"build": "tsc -p tsconfig.json"`; that config extends `types/tsconfig.ecs7.strict.json` →
   `tsconfig.ecs7.json` with `declaration: true`, `outDir: "."` — a real tsc emit path the change
   touches. Compiling `packages/@dcl/sdk/src` at `2d718be5` against published
   `@dcl/ecs`/`@dcl/react-ecs`/`@dcl/js-runtime` 7.25.0, before and after removing
   `downlevelIteration` **and** `suppressExcessPropertyErrors`: **all 56 emitted files (`.js` and
   `.d.ts`) byte-identical**, zero type errors both arms.

4. **Scene bundles.** Across the 60 scenes of `decentraland/sdk7-test-scenes` built through our own
   Rust toolchain (rolldown, `target: es2020`), `bin/scene.js`, `bin/sdk-runtime.js` and
   `bin/index.js` are byte-identical with and without `downlevelIteration` — 60/60, including the 5
   scenes failing type-check for unrelated missing dependencies. (Our toolchain, not yours; item 3
   covers `sdk-commands`' own tsc invocation.)

Independently, `downlevelIteration` cannot reach a byte `sdk-commands` ships:
`packages/@dcl/sdk-commands/src/logic/bundle.ts:349` runs tsc with `--noEmit` (or
`--emitDeclarationOnly`), and the bundler is esbuild pinned to `target: 'es2020'` at
`bundle.ts:203`. esbuild does not read `downlevelIteration` at all.

### `suppressExcessPropertyErrors: false` is the compiler default

`typescript@6.0.3` `lib/_tsc.js:37826-37832` declares the option with
`defaultValueDescription: false`. Assigning `{ x: 1, y: 2, z: 3 }` to
`interface Point { x: number; y: number }` gives the same `TS2353` with the key present and absent,
on 5.9.3, 6.0.3 and 7.0.2. Removing it changes nothing; it is reachable only as a
deprecation/unknown-option check.

Do not "fix" it by flipping to `true` — that is the removed-option path (`TS5102` on TS 7) and would
actually change checking.

### `moduleResolution: "node"` → `"bundler"`

The only edit of the three with an observable delta, hence the detail.

`tsc --showConfig` diff for a scene extending the config, node10 vs bundler — exactly three defaults
flip, nothing else:

```
-        "moduleResolution": "node10",
+        "moduleResolution": "bundler",
+        "resolvePackageJsonExports": true,
+        "resolvePackageJsonImports": true,
+        "resolveJsonModule": true,
```

* Nothing in a scene's type graph is affected by the `exports`/`imports` flip: `@dcl/sdk` declares
  neither field, and across a full scene install the only `@dcl/*` package declaring `exports` is
  `@dcl/gltf-validator-ts`, a transitive `sdk-commands` dependency no scene's types reach.
* Across all 60 `sdk7-test-scenes`, `tsc --noEmit --listFiles --pretty false` under TypeScript 5.9.3
  (which accepts both values without deprecation noise) produced **byte-identical output** — full
  resolution graph *and* full diagnostic text, 13,578 lines, 0 files differing. The 5 scenes with
  pre-existing errors reported the same errors in both arms.
* **One real difference, in the SDK's own declaration emit.** Rebuilding `packages/@dcl/sdk` as in
  item 3, adding the `moduleResolution` change on top of the two removals changes exactly one of the
  56 output files, `ethereum-provider/index.d.ts`:

  ```diff
  -    send(message: import("../internal/provider").RPCSendableMessage, ...): void;
  -    sendAsync(message: import("../internal/provider").RPCSendableMessage, ...): void;
  +    send(message: import(".").RPCSendableMessage, ...): void;
  +    sendAsync(message: import(".").RPCSendableMessage, ...): void;
  ```

  The `import(".")` form is self-referential — the same file ends with
  `export { RPCSendableMessage } from '../internal/provider'` — but it resolves. A consumer calling
  `createEthereumProvider()` and using `Parameters<typeof p.send>[0]` type-checks clean against both
  emitted `.d.ts` variants, under both `moduleResolution: node` and `bundler`. Flagged as a change in
  a published artifact, not because it broke anything.
* **Raises the TypeScript floor to 5.0.** `bundler` did not exist before TS 5.0; TypeScript 4.9.5
  rejects it with `TS6046: Argument for '--moduleResolution' option must be: 'node', 'classic',
  'node16', 'nodenext'`. `@dcl/sdk-commands` already depends on `typescript: ^5.0.2`, so the shipped
  toolchain is unaffected, but a scene pinning its own `typescript@4.x` breaks.

`node16`/`nodenext` are not alternatives: `TS5110` requires `module` to be `Node16`/`NodeNext`, and
this config sets `module: "esnext"`. `bundler` also honestly describes the pipeline, since esbuild
does the real resolving.

## Proposed patch

```diff
--- a/packages/@dcl/sdk/types/tsconfig.ecs7.json
+++ b/packages/@dcl/sdk/types/tsconfig.ecs7.json
@@ -2,13 +2,11 @@
   "compilerOptions": {
     "target": "es2020",
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
     "inlineSourceMap": true,
     "sourceMap": false,
```

After the patch the reproduction exits 0 on 5.9.3, 6.0.3 *and* 7.0.2.

For a minimal blast radius the two deletions are provably inert (identical tsc emit, diagnostics,
bundles) and can land alone; that fixes the one error users cannot work around (`TS5023`) and one of
the two TS 6 deprecations. The `moduleResolution` line carries the `.d.ts` delta and the
TypeScript-5.0 floor, and is the one a scene *can* override itself, so a separate PR is reasonable.

## Out of scope, but the same class of problem

Not verified beyond reading the files, except where noted:

* `packages/@dcl/ecs/tsconfig.json` and `packages/@dcl/react-ecs/tsconfig.json` both set
  `downlevelIteration: true` and `moduleResolution: "node"` at `target: es2020`. Repo-internal build
  configs, not shipped, but they stop the repo's own `npm run build` once the root
  `typescript: ^5.0.2` range widens.
* `decentraland/sdk7-scene-template`'s `tsconfig.json` (different repo) sets `baseUrl: "."` plus a
  non-relative `paths` target. Verified against TS 7.0.2 with an already-patched ecs7 config: still
  fails with `TS5102: Option 'baseUrl' has been removed` and `TS5090: Non-relative paths are not
  allowed`. Fixing ecs7 is necessary but not sufficient for a scaffolded scene on TS 7.
* Scenes on disk keep whatever `tsconfig.json` they were scaffolded with; nothing rewrites it, so
  the `baseUrl` problem needs a migration step, not just a template change.

## What we verified vs what we are inferring

Verified by running it, this session:

* every diagnostic and exit code in the "Expected vs actual" and workaround-matrix tables, on
  5.9.3 / 6.0.3 / 7.0.2 against unmodified `@dcl/sdk@7.25.0`;
* sha256 equality between the npm-published config and the repo file at `2d718be5`;
* the `downlevelIteration` emit probe (es2020 identical, es5 different);
* the 56-file byte-identical `packages/@dcl/sdk` rebuild, the single `.d.ts` delta the
  `moduleResolution` line introduces, and the consumer type-check of both `.d.ts` variants;
* the 60-scene `--listFiles` + diagnostics equality, node10 vs bundler, under 5.9.3;
* the 60-scene byte-identical bundle comparison with and without `downlevelIteration`;
* the `--showConfig` default-flip diff; the `suppressExcessPropertyErrors` no-op probe; TS 4.9.5's
  `TS6046` rejection of `bundler`; the `@dcl/sdk`/`@dcl/*` `exports`/`imports` census.

Inferred, not executed:

* that `sdk-commands`' esbuild path is unaffected — read from `bundle.ts:203` and `:349` plus
  esbuild's documented option set, not a diffed `sdk-commands` build;
* the `@dcl/ecs` / `@dcl/react-ecs` build-config claims above (read, not built);
* that no downstream repo depends on the exact `import("../internal/provider")` spelling in the
  emitted `ethereum-provider/index.d.ts`.

Our overlay lives in `scripts/blob_overlays.py` (`patch_ecs7_tsconfig()`, run by
`scripts/build-base-blob.py`), rationale in `docs/ts7-migration.md`; it exists only until this lands
upstream. Each of its three edits must match the vendored `tsconfig.ecs7.json` exactly once, so the
blob build fails naming any edit upstream already shipped, with the instruction to delete the
overlay once none of the three finds anything to change.
