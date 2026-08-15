# PR draft: make `@dcl/sdk/types/tsconfig.ecs7.json` TypeScript 6/7 clean

Target repo: `decentraland/js-sdk-toolchain`
File: `packages/@dcl/sdk/types/tsconfig.ecs7.json`
Status: draft, not filed. Everything below was re-verified from scratch on 2026-08-02.

## Summary

`packages/@dcl/sdk/types/tsconfig.ecs7.json` sets three compiler options that TypeScript 6
deprecates and TypeScript 7 removes. Every SDK7 scene `extends` this file, so under TS 7 a stock
scene cannot type-check and — for `suppressExcessPropertyErrors` specifically — cannot work around
it from its own `tsconfig.json`, because a child config can neither delete an inherited key nor
silence a *removed* option. All three options are inert at the `target: es2020` that the same file
sets, so removing them is a no-op for behaviour and for emit.

## Environment

| | |
| --- | --- |
| host | macOS 25.5.0, arm64 |
| node | v24.18.0 |
| `@dcl/sdk` | 7.25.0 (npm) |
| typescript | 5.9.3 (2025-09-30), 6.0.3 (2026-04-16), 7.0.2 (2026-07-08, current `latest`) |

The npm-published `@dcl/sdk@7.25.0` copy of `types/tsconfig.ecs7.json` is byte-identical to
`packages/@dcl/sdk/types/tsconfig.ecs7.json` in the repo at `2d718be5` ("feat: replace legacy web
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

Nothing in the reproduction is scene-specific: the diagnostics come from parsing the inherited
config, so any file under `include` will do.

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

Note that `suppressExcessPropertyErrors` is silent on 5.9.3 and 6.0.3 and only fails on 7.0.2, where
the option no longer exists at all.

## Why this has to change in the shipped file

We tried every child-config escape hatch against the same reproduction. Verified matrix, all runs
against unmodified `@dcl/sdk@7.25.0`:

| child `compilerOptions` | 5.9.3 | 6.0.3 | 7.0.2 |
| --- | --- | --- | --- |
| *(none)* | clean | TS5101 + TS5107 | TS5102 + TS5108 + TS5023 |
| `"ignoreDeprecations": "6.0"` | **TS5103** invalid value | clean | TS5102 + TS5108 + TS5023 |
| `"downlevelIteration": false` | clean | TS5101 + TS5107 (re-reported at the child's own line) | TS5102 + TS5108 + TS5023 |
| `"moduleResolution": "bundler"` | clean | TS5101 | TS5102 + TS5023 |
| `"downlevelIteration": null, "moduleResolution": "bundler"` | clean | clean | **TS5023** |
| add `"suppressExcessPropertyErrors": null` to the above | clean | clean | **TS5023** |

Two things fall out of that table:

* On TS 6 there *is* a workaround (`ignoreDeprecations: "6.0"`, or a `null` override plus
  `moduleResolution`), but `ignoreDeprecations: "6.0"` is not portable: TypeScript 5.9.3 rejects the
  value outright with `TS5103: Invalid value for '--ignoreDeprecations'`, so a scene that adds it
  stops building for everyone still on 5.x.
* On TS 7 there is no workaround for `suppressExcessPropertyErrors`. `TS5023` is reported at
  `tsconfig.ecs7.json(11,5)` — the inherited file — and a child config cannot delete an inherited
  key. Setting it to `null` in the child does not help. The only user-side escape is to stop
  extending the SDK config and inline a copy of it.

## Evidence that the three edits are behaviour-preserving

### `downlevelIteration` is dead at `target: es2020`

Three independent checks.

1. **Compiler source.** In `typescript@6.0.3` `lib/_tsc.js`, every read of
   `compilerOptions.downlevelIteration` reachable from the checker is gated on the language version
   being below ES2015 (lines 73940, 73974, 79669, 80460, 83459, 83831). The one ungated read, line
   83920, is `const downlevelIteration = !uplevelIteration && compilerOptions.downlevelIteration`,
   where line 83919 is `const uplevelIteration = languageVersion >= 2 /* ES2015 */ && iterableExists`
   — so at `es2020` with `lib: ["ES2020"]` it is forced to `false` and only ever selects the wording
   of a diagnostic. All remaining reads (93269, 93365, 93534, 106571, 107863, 107892) are in emit
   transformers.

2. **Emit probe with a sensitivity control.** A file exercising generators, `Map`/`Set` spread,
   array-rest destructuring, `Math.max(...set)`, `for...of` over an `Iterable`, destructuring
   `for...of` over a `Map`, and an async generator, compiled by tsc 6.0.3 with
   `--downlevelIteration true` vs `false`:

   * `--target es2020`: **identical**, sha256 `1aea3621e7085044fdada02b892f39d0cfab4f2548bf3186094d1dfc13254183` both ways.
   * `--target es5`: **different** (`f2ef2d24…` vs `56d660fa…`), which is the control proving the
     probe is actually sensitive to the flag.

3. **The SDK package's own tsc build.** `packages/@dcl/sdk/package.json` has
   `"build": "tsc -p tsconfig.json"`, and that config extends `types/tsconfig.ecs7.strict.json` →
   `tsconfig.ecs7.json`, with `declaration: true` and `outDir: "."`. So this is a real tsc emit path
   that the change touches. Compiling `packages/@dcl/sdk/src` at `2d718be5` against published
   `@dcl/ecs`/`@dcl/react-ecs`/`@dcl/js-runtime` 7.25.0, before and after removing
   `downlevelIteration` **and** `suppressExcessPropertyErrors`: **all 56 emitted files (`.js` and
   `.d.ts`) byte-identical**, zero type errors in both arms.

4. **Scene bundles.** Across the 60 scenes of `decentraland/sdk7-test-scenes`, built through our own
   Rust toolchain (rolldown, `target: es2020`), `bin/scene.js`, `bin/sdk-runtime.js` and
   `bin/index.js` are byte-identical with and without `downlevelIteration` — 60/60, including the 5
   scenes that fail type-check for unrelated missing dependencies. (This arm is evidence from our
   toolchain, not yours; item 3 above is the one that covers `sdk-commands`' own tsc invocation.)

Independently of all of that, `downlevelIteration` cannot reach a byte that `sdk-commands` ships:
`packages/@dcl/sdk-commands/src/logic/bundle.ts:349` runs tsc with `--noEmit` (or
`--emitDeclarationOnly`), and the bundler is esbuild pinned to `target: 'es2020'` at
`bundle.ts:203`. esbuild does not read `downlevelIteration` at all.

### `suppressExcessPropertyErrors: false` is the compiler default

`typescript@6.0.3` `lib/_tsc.js:37826-37832` declares the option with
`defaultValueDescription: false`. Behaviourally, assigning `{ x: 1, y: 2, z: 3 }` to an
`interface Point { x: number; y: number }` produces the same `TS2353` with the key present and with
it absent, on 5.9.3, 6.0.3 and 7.0.2. Removing the key changes nothing; it is only reachable as a
deprecation/unknown-option check.

Do not "fix" this one by flipping it to `true` — that is the removed-option path (`TS5102` on TS 7),
and it would actually change checking.

### `moduleResolution: "node"` → `"bundler"`

This is the only edit of the three with any observable delta, so it gets more detail.

`tsc --showConfig` diff for a scene extending the config, node10 vs bundler — exactly three defaults
flip, nothing else:

```
-        "moduleResolution": "node10",
+        "moduleResolution": "bundler",
+        "resolvePackageJsonExports": true,
+        "resolvePackageJsonImports": true,
+        "resolveJsonModule": true,
```

* No package in a scene's type graph is affected by the `exports`/`imports` flip: `@dcl/sdk` declares
  neither field, and across a full scene install the only `@dcl/*` package that declares `exports` is
  `@dcl/gltf-validator-ts`, a transitive `sdk-commands` dependency that no scene's types reach.
* Across all 60 `sdk7-test-scenes`, `tsc --noEmit --listFiles --pretty false` under TypeScript 5.9.3
  (which accepts both values without deprecation noise) produced **byte-identical output** — the full
  resolution graph *and* the full diagnostic text, 13,578 lines total, 0 files differing. The 5
  scenes with pre-existing errors reported exactly the same errors in both arms.
* **One real difference, in the SDK's own declaration emit.** Rebuilding `packages/@dcl/sdk` as in
  item 3 above, adding the `moduleResolution` change on top of the two removals changes exactly one
  of the 56 output files, `ethereum-provider/index.d.ts`:

  ```diff
  -    send(message: import("../internal/provider").RPCSendableMessage, ...): void;
  -    sendAsync(message: import("../internal/provider").RPCSendableMessage, ...): void;
  +    send(message: import(".").RPCSendableMessage, ...): void;
  +    sendAsync(message: import(".").RPCSendableMessage, ...): void;
  ```

  The `import(".")` form is self-referential — the same file ends with
  `export { RPCSendableMessage } from '../internal/provider'` — but it resolves. A consumer that
  calls `createEthereumProvider()` and uses `Parameters<typeof p.send>[0]` type-checks clean against
  both emitted `.d.ts` variants, under both `moduleResolution: node` and `moduleResolution: bundler`.
  We are flagging it because it is a change in a published artifact, not because we found it to
  break anything.
* **It raises the TypeScript floor to 5.0.** `bundler` did not exist before TS 5.0; TypeScript 4.9.5
  rejects it with `TS6046: Argument for '--moduleResolution' option must be: 'node', 'classic',
  'node16', 'nodenext'`. `@dcl/sdk-commands` already depends on `typescript: ^5.0.2`, so the shipped
  toolchain is unaffected, but a scene that pins its own `typescript@4.x` would break.

`node16`/`nodenext` are not alternatives here: `TS5110` requires `module` to be `Node16`/`NodeNext`,
and this config sets `module: "esnext"`. `bundler` is also the honest description of the pipeline,
since esbuild does the real resolving.

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

After the patch, the reproduction above exits 0 on 5.9.3, 6.0.3 *and* 7.0.2.

If you would rather keep the blast radius minimal, the two deletions are provably inert (identical
tsc emit, identical diagnostics, identical bundles) and can land on their own; that alone fixes the
one error users cannot work around (`TS5023`) and one of the two TS 6 deprecations. The
`moduleResolution` line is the one with a `.d.ts` delta and a TypeScript-5.0 floor, and is also the
one a scene *can* override for itself, so it is reasonable to split it into a separate PR.

## Out of scope, but the same class of problem

Not verified beyond reading the files, except where noted:

* `packages/@dcl/ecs/tsconfig.json` and `packages/@dcl/react-ecs/tsconfig.json` both set
  `downlevelIteration: true` and `moduleResolution: "node"` at `target: es2020`. These are
  repo-internal build configs, not shipped to users, but they will stop the repo's own `npm run
  build` once the root `typescript: ^5.0.2` range is widened.
* `decentraland/sdk7-scene-template`'s `tsconfig.json` (different repo) sets `baseUrl: "."` plus a
  non-relative `paths` target. Verified against TS 7.0.2 with an already-patched ecs7 config: it
  still fails with `TS5102: Option 'baseUrl' has been removed` and `TS5090: Non-relative paths are
  not allowed`. Fixing ecs7 is necessary but not sufficient for a scaffolded scene on TS 7.
* Scenes already on disk keep whatever `tsconfig.json` they were scaffolded with; nothing rewrites
  it. The `baseUrl` problem above therefore needs a migration step, not just a template change.

## What we verified vs what we are inferring

Verified by running it, this session:

* every diagnostic and exit code in the "Expected vs actual" and workaround-matrix tables, on
  5.9.3 / 6.0.3 / 7.0.2 against unmodified `@dcl/sdk@7.25.0`;
* the sha256 equality between the npm-published config and the repo file at `2d718be5`;
* the `downlevelIteration` emit probe (es2020 identical, es5 different);
* the 56-file byte-identical rebuild of `packages/@dcl/sdk`, and the single `.d.ts` delta introduced
  by the `moduleResolution` line, plus the consumer type-check of both `.d.ts` variants;
* the 60-scene `--listFiles` + diagnostics equality between node10 and bundler under 5.9.3;
* the 60-scene byte-identical bundle comparison with and without `downlevelIteration`;
* the `--showConfig` default-flip diff; the `suppressExcessPropertyErrors` no-op probe; the
  TS 4.9.5 `TS6046` rejection of `bundler`; the `@dcl/sdk`/`@dcl/*` `exports`/`imports` census.

Inferred, not executed:

* that `sdk-commands`' esbuild path is unaffected — read from `bundle.ts:203` and `:349` plus
  esbuild's documented option set, not from a diffed `sdk-commands` build;
* the claims about `@dcl/ecs` and `@dcl/react-ecs` build configs above (read, not built);
* that no downstream repo depends on the exact `import("../internal/provider")` spelling in the
  emitted `ethereum-provider/index.d.ts`.

Our own overlay of this patch lives in `scripts/build-base-blob.py` (`patch_ecs7_tsconfig`) with the
rationale in `docs/ts7-migration.md`; it exists only until this lands upstream.
