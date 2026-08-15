# @dcl/inspector: eight undeclared runtime dependencies, and an undeclared node floor

Draft for `decentraland/creator-hub` (`packages/inspector`). Not yet filed.

---

## Summary

`@dcl/inspector`'s published CommonJS entrypoint `dist/tooling-entrypoint.js` `require()`s eight
packages that appear in no `dependencies` chain reachable from the published manifest. Installing
`@dcl/inspector` on its own and `require()`ing it fails immediately, eight times in a row. It only
works today inside `@dcl/sdk-commands` because unrelated siblings happen to hoist all eight. In
7.37.0 the count is ten, and two of the new ones (`@babylonjs/core`, `@dcl/ecs-math`) are not
provided by the `@dcl/sdk-commands` tree either.

Secondary: the package declares no `engines`. Its real node floor depends on which `node-fetch`
major resolves — with `node-fetch` 3.x it is 20.19.0, because a CJS file `require()`s an ESM
package. The single `node-fetch` call site is `fetch(url).then(r => r.text())` behind an
almost-always-unset config flag, so deleting the import removes six packages and the floor problem
at once.

---

## Environment

| | |
|---|---|
| `@dcl/inspector` | 7.36.3 (`shasum 7c4f9b44…`, unpacked 125,062,573 B); latest 7.37.0 also checked |
| `@dcl/sdk-commands` | 7.25.0 (pins `"@dcl/inspector": "7.36.3"`) |
| node | v24.18.0 |
| package manager | `corepack pnpm@11.18.0` with `--config.node-linker=hoisted --ignore-scripts` (npm-equivalent flat layout; npm is blocked in our environment, see *Caveats*) |
| OS | macOS 25.5.0 arm64 |

`dist/tooling-entrypoint.js` in 7.36.3 is 2,316,341 bytes. All attributions below come from the
`dist/tooling-entrypoint.js.map` shipped alongside it.

---

# Defect 1 — undeclared runtime dependencies

## Reproduction

```sh
mkdir /tmp/repro && cd /tmp/repro
npm init -y
npm i @dcl/inspector@7.36.3
node -e 'require("@dcl/inspector")'
```

## Expected

Loads. `require("@dcl/inspector")` is the supported entrypoint — `main` points at
`dist/tooling-entrypoint.js`, and `@dcl/sdk-commands` itself does exactly this in
`dist/logic/composite.js:10`, `dist/commands/start/data-layer/rpc.js:37`,
`dist/commands/code-to-composite/composite-generator.js:37` and
`dist/commands/code-to-composite/scene-executor.js:12`.

## Actual

```
Error: Cannot find module 'long'
Require stack:
- /private/tmp/repro/node_modules/@dcl/inspector/dist/tooling-entrypoint.js
```

Installing each missing package and re-running walks the whole set. Verbatim sequence from our run
(each step: install the package named by the previous `MODULE_NOT_FOUND`, retry):

```
STEP 1: missing 'long'                                     -> install long
STEP 2: missing 'protobufjs/minimal'                       -> install protobufjs
STEP 3: missing 'ajv/dist/jtd'                             -> install ajv
STEP 4: missing '@protobufjs/utf8'                         -> install @protobufjs/utf8
STEP 5: missing 'ignore'                                   -> install ignore
STEP 6: missing 'node-fetch'                               -> install node-fetch
STEP 7: missing '@well-known-components/pushable-channel'  -> install @well-known-components/pushable-channel
STEP 8: missing 'fp-future'                                -> install fp-future
STEP 9: require OK
```

## Evidence

### Every literal `require()` in the published entrypoint

There are no computed/dynamic `require()` calls in the file — a scan for `require(` with a
non-string-literal argument returns nothing — so this census is complete:

| specifier | count | declared where | resolves from a bare install? |
|---|---:|---|---|
| `protobufjs/minimal` | 68 | nowhere | no |
| `mitt` | 3 | transitively, via `@dcl/asset-packs` -> `mitt ^3.0.1` | yes (by luck) |
| `long` | 2 | nowhere | no |
| `@protobufjs/utf8` | 2 | nowhere | no |
| `ts-deepmerge` | 1 | `dependencies` | yes |
| `node-fetch` | 1 | `devDependencies` (`^2.7.0`) | no |
| `ignore` | 1 | `devDependencies` (`^7.0.5`) | no |
| `fp-future` | 1 | `devDependencies` (`^1.0.1`) | no |
| `ajv/dist/jtd` | 1 | nowhere (not even a devDependency) | no |
| `@well-known-components/pushable-channel` | 1 | `devDependencies` (`^1.0.3`) | no |

`dependencies` in 7.36.3 is exactly `{"@babel/parser": "7.28.5", "@dcl/asset-packs": "^2.17.0",
"ts-deepmerge": "^7.0.0"}` — one of the ten. (`@babel/parser` is not required by
`tooling-entrypoint.js` at all; we assume it is there for `public/bundle.js` and did not chase it.)

### Where each one enters the bundle

Decoded from `tooling-entrypoint.js.map` by mapping the generated offset of each `require()` back
to its original source:

| specifier | origin |
|---|---|
| `ajv/dist/jtd` | `src/lib/logic/preferences/io.ts:2` |
| `ignore` | `src/lib/data-layer/host/fs-utils.ts:1` |
| `node-fetch` | `src/lib/data-layer/host/utils/install-bin.ts:1` |
| `@well-known-components/pushable-channel` | `src/lib/data-layer/host/stream.ts:1` |
| `long`, `protobufjs/minimal` | `src/lib/data-layer/proto/gen/data-layer.gen.ts:2-3` (ts-proto output) |
| `long`, `protobufjs/minimal`, `@protobufjs/utf8` | inlined `@dcl/ecs/dist/**` (~70 `*.gen.js`, `serialization/ByteBuffer/index.js`, `components/component-number.js`) |
| `fp-future`, `mitt` | inlined `@dcl/mini-rpc/src/{rpc,transport}.ts` |

So there are two layers to the same mistake. Four specifiers are the inspector's own source
(`ajv`, `ignore`, `node-fetch`, `pushable-channel`), plus `long`/`protobufjs` from your own
generated proto. The rest arrive because the bundler inlines `@dcl/ecs` and `@dcl/mini-rpc`
*source* while leaving *their* bare specifiers external — the externalised set is a property of the
build, and nothing reconciles it against `dependencies`.

We could not read `build.js` (it is not in `files`), so the exact `external:` configuration is an
inference; the effect is not.

### Why this has never broken CI

Under a real `@dcl/sdk-commands` 7.25.0 install every one of the eight resolves — from packages
that have nothing to do with the inspector:

| specifier | who actually declares it in that tree | version resolved |
|---|---|---|
| `long` | `protobufjs (^5.0.0)`, `ts-proto-descriptors` | 5.3.2 |
| `protobufjs` | `@dcl/protocol (7.2.4)`, `@dcl/ts-proto`, `ipfs-unixfs`, `ipld-dag-pb`, `ts-proto` | 7.2.4 |
| `ajv` | `@dcl/schemas (^8.11.0)`, `ajv-errors`, `ajv-keywords` | 8.20.0 |
| `@protobufjs/utf8` | `protobufjs (^1.1.0)` | 1.1.2 |
| `ignore` | `@dcl/sdk-commands (^5.2.4)` | 5.3.2 |
| `node-fetch` | `@well-known-components/http-server (^2.6.9)`, `rabin-wasm` | **2.7.0** |
| `@well-known-components/pushable-channel` | `@dcl/mini-comms (^1.0.3)` | 1.0.3 |
| `fp-future` | `@dcl/sdk-commands (^1.0.1)`, `@well-known-components/http-server` | 1.0.1 |

Verified: `cd /tmp/realflow && npm i @dcl/sdk-commands@7.25.0 && node -e 'require("@dcl/inspector")'`
exits 0.

Every one of these is a version the inspector never chose and does not pin. Note `ignore`: the
inspector's devDependency says `^7.0.5`, the tree hands it 5.3.2. `protobufjs`: 7.2.4 here, 8.7.1
in a bare install.

### 7.37.0 makes it worse

Same census against `@dcl/inspector@7.37.0` adds two entries, both top-level eager requires
(`var qo=require("@babylonjs/core")`, `var Da=require("@dcl/ecs-math")`):

```
      1 require("@dcl/ecs-math")        # devDependency 2.1.0, not a dependency
      1 require("@babylonjs/core")      # devDependency 8.7.0, not a dependency
```

`@dcl/sdk-commands` 7.25.0's tree provides neither, so 7.37.0 would fail at
`require("@dcl/inspector")` where 7.36.3 succeeds. This is also why we are pinned to 7.36.3
downstream: satisfying `@babylonjs/core` means installing a 3D engine into a node-side host
process, for what appears to be `Vector3`/`Quaternion` use inside browser-side snap helpers.

Separately (not part of this issue, flagging it because we tripped over it): with `@dcl/ecs-math`
2.1.0 present, `require("@dcl/inspector")@7.37.0` on node 24 then fails with

```
Error [ERR_MODULE_NOT_FOUND]: Cannot find module '.../@dcl/ecs-math/dist/Quaternion'
imported from .../@dcl/ecs-math/dist/index.js
```

— extensionless relative imports in an ESM build. That is a `@dcl/ecs-math` defect and belongs in
its own issue.

---

# Defect 2 — no `engines`, and a node floor that depends on transitive luck

`@dcl/inspector` 7.36.3 and 7.37.0 publish **no `engines` field at all**, and no `"type"` field, so
`dist/tooling-entrypoint.js` is CJS.

The naive claim "the floor is node 20.19" is **only true when `node-fetch` 3.x is what resolves**.
We verified both directions:

**With `node-fetch` 3.3.2** (`"type": "module"`) — what you get from a bare `npm i node-fetch`
today, i.e. what anyone fixing Defect 1 by hand will land on:

```sh
node --no-experimental-require-module -e 'require("@dcl/inspector")'
```

```
Error [ERR_REQUIRE_ESM]: require() of ES Module
/private/tmp/repro/node_modules/node-fetch/src/index.js from
/private/tmp/repro/node_modules/@dcl/inspector/dist/tooling-entrypoint.js not supported.
```

Exit 1. `require(esm)` is unflagged from node 20.19.0 / 22.12.0, so this is the floor in that
configuration.

**With `node-fetch` 2.7.0** (CJS) — what the `@dcl/sdk-commands` tree happens to hoist — the same
command exits 0 and prints `REQUIRE OK`. So `node-fetch` is the *only* ESM-require blocker in the
closure. `long` 5.3.2 is `"type": "module"` but ships a `require` export condition
(`./umd/index.js`), so it is fine.

Conclusion: **today, on the default `@dcl/sdk-commands` install, `ERR_REQUIRE_ESM` does not
reproduce.** It is latent — it fires the moment anything in the tree resolves `node-fetch` to 3.x,
which is precisely what happens if a user or a downstream tool adds `node-fetch` to satisfy Defect
1. Fixing Defect 1 by declaring `"node-fetch": "^2.7.0"` fixes Defect 2 as a side effect; declaring
`"node-fetch": "^3"` would make Defect 2 real for everyone.

## The `node-fetch` import is removable

`src/lib/data-layer/host/utils/install-bin.ts` in full (recovered from `sourcesContent`):

```ts
import fetch from 'node-fetch';
import type { FileSystemInterface } from '../../types';
import { getConfig } from '../../../logic/config';

export async function installBin(fs: FileSystemInterface) {
  const config = getConfig();
  if (!config.binIndexJsUrl) {
    return;
  }

  console.log('Installing binaries');
  const bin = await fetch(config.binIndexJsUrl).then(resp => resp.text());
  await fs.writeFile('bin/index.js', Buffer.from(bin));
}
```

This is the only `node-fetch` call site in the published entrypoint. It is called once, from
`src/lib/data-layer/host/rpc-methods.ts:129` (`await installBin(fs)`) inside
`createDataLayerHost`, and returns immediately unless `binIndexJsUrl` is set — de-minifying
`getConfig()` in the shipped bundle gives `binIndexJsUrl: searchParams.get('binIndexJsUrl') ||
globalThis.InspectorConfig?.binIndexJsUrl || null`, i.e. it defaults to `null` and nothing in the
package sets it. `fetch(url).then(r => r.text())` is signature-identical on
global `fetch`, which node has had unflagged since 18.0.0.

Dropping the import removes six packages from the install: `node-fetch`, `fetch-blob`,
`formdata-polyfill`, `data-uri-to-buffer`, `node-domexception`, `web-streams-polyfill`
(`node-fetch@3.3.2` -> `fetch-blob@3.2.0` -> `node-domexception` + `web-streams-polyfill`;
`formdata-polyfill` -> `fetch-blob`). It also lets `@types/node-fetch` go.

---

# Proposed fix

Three changes. (1) is the actual bug fix; (2) and (3) are what make it stay fixed.

### 1. `packages/inspector/src/lib/data-layer/host/utils/install-bin.ts` — use global `fetch`

```diff
--- a/packages/inspector/src/lib/data-layer/host/utils/install-bin.ts
+++ b/packages/inspector/src/lib/data-layer/host/utils/install-bin.ts
@@ -1,3 +1,2 @@
-import fetch from 'node-fetch';
 import type { FileSystemInterface } from '../../types';
 import { getConfig } from '../../../logic/config';
```

No other line changes; `fetch(config.binIndexJsUrl).then(resp => resp.text())` is unchanged.

### 2. `packages/inspector/package.json` — declare what the bundle requires

We only have the *published* manifest, whose keys npm has normalised; the repo file's key order
will differ, so apply this as five discrete edits rather than as a patch:

**`dependencies` — add seven** (`ajv` is new; the other six move from `devDependencies`):

```diff
   "dependencies": {
     "@babel/parser": "7.28.5",
     "@dcl/asset-packs": "^2.17.0",
+    "@protobufjs/utf8": "^1.1.0",
+    "@well-known-components/pushable-channel": "^1.0.3",
+    "ajv": "^8.12.0",
+    "fp-future": "^1.0.1",
+    "ignore": "^7.0.5",
+    "long": "^5.2.3",
+    "protobufjs": "^7.2.4",
     "ts-deepmerge": "^7.0.0"
   },
```

**`devDependencies` — delete five entries** (four moved above, one now unused):

```diff
-    "@types/node-fetch": "^2.6.4",
-    "@well-known-components/pushable-channel": "^1.0.3",
-    "fp-future": "^1.0.1",
-    "ignore": "^7.0.5",
-    "node-fetch": "^2.7.0",
```

**Add an `engines` block** (there is none today):

```diff
+  "engines": {
+    "node": ">=20.19.0"
+  },
```

Notes on the ranges, all of which are judgement calls we are flagging rather than asserting:

- `protobufjs ^7.2.4` rather than `^8`: 7.2.4 is what `@dcl/protocol` pins and therefore what every
  `@dcl/sdk-commands` install resolves today. A bare `npm i protobufjs` gives 8.7.1. Declaring `^8`
  would silently change the wire codec under existing scenes. We did not test the bundle against
  protobufjs 8 beyond confirming it loads.
- `ignore ^7.0.5` matches the existing devDependency, but note the tree currently runs 5.3.2 via
  `@dcl/sdk-commands`. Whichever you pick, picking one is the point.
- `ajv ^8.12.0`: the bundle uses `ajv/dist/jtd` only (JTD, RFC 8927). Resolved version today is
  8.20.0.
- `engines: ">=20.19.0"`: with change (1) applied the *technical* floor drops to 18.0.0 (global
  `fetch`), but node 18 is EOL and 20.19.0 is the floor for `require(esm)` generally, which this
  package is one dependency bump away from needing again. If you would rather keep `node-fetch`,
  declare `"node-fetch": "^2.7.0"` in `dependencies` and the same `engines` value still holds.

### 3. A publish-time smoke test so this cannot regress

The whole class of defect is invisible to `npm test` and to `packages/inspector`'s own build,
because the monorepo's `node_modules` has everything. It is only visible from outside the repo. One
CI step catches all ten cases including the 7.37.0 `@babylonjs/core` regression:

```yaml
- name: entrypoint loads from a clean install
  run: |
    TARBALL=$(cd packages/inspector && npm pack --silent)
    mkdir -p /tmp/pkgtest && cd /tmp/pkgtest && npm init -y
    npm i "$GITHUB_WORKSPACE/packages/inspector/$TARBALL"
    node -e 'require("@dcl/inspector")'
    node --no-experimental-require-module -e 'require("@dcl/inspector")'
```

The second `node` line pins the CJS/ESM boundary: it fails if any newly-externalised dependency is
ESM-only, which is the thing `engines` is supposed to be tracking.

---

## What we verified vs. what we are inferring

**Verified by direct execution or by reading the published artifact:**

- The full `require()` census of `dist/tooling-entrypoint.js` for 7.36.3 and 7.37.0, and that no
  `require()` in the file takes a non-literal argument.
- 7.36.3's published `dependencies`, absence of `type`, absence of `engines` (read from
  `registry.npmjs.org` metadata and from the installed package).
- The eight-step failure sequence from a bare install, in the order shown.
- That `require("@dcl/inspector")` succeeds under a real `@dcl/sdk-commands@7.25.0` install, and
  which sibling declares each of the eight.
- The `ERR_REQUIRE_ESM` failure under `node-fetch@3.3.2`, and that it disappears under
  `node-fetch@2.7.0`, with `--no-experimental-require-module` on node 24.18.0.
- `install-bin.ts` and its single call site at `rpc-methods.ts:129`, recovered from the published
  sourcemap.
- `node-fetch@3.3.2`'s six-package closure, from registry manifests.
- That 7.37.0's `@babylonjs/core` and `@dcl/ecs-math` requires are top-level and eager.

**Inferred, not verified:**

- Node 18 behaviour. We did not run node 18; `--no-experimental-require-module` on node 24 is the
  documented emulation of pre-20.19 `require()` semantics, and we are treating it as equivalent.
- That the cause is an esbuild `external:` list that is not reconciled against `dependencies`.
  `build.js` is not published and we did not read the repo, so this is reasoning from the artifact.
- That `@babel/parser` and `@dcl/asset-packs` are `dependencies` for `public/bundle.js`'s sake.
  Neither is required by `tooling-entrypoint.js`.
- Whether upstream intends `protobufjs` 7 or 8, and `ignore` 5 or 7. We picked what today's trees
  resolve; only you can say which is correct.
- Whether anything else in the codebase (not reachable from `tooling-entrypoint.js`) also relies on
  `node-fetch`'s v2-specific API surface. We checked only the published node entrypoint.

## Caveats on our environment

`npm` is blocked machine-wide here, so every install above was `corepack pnpm add --ignore-scripts
--config.node-linker=hoisted <pkg>`, which produces the same flat `node_modules` layout npm does.
The failures are resolution failures against that flat layout, so they should reproduce identically
under npm; we have not confirmed that ourselves. `--ignore-scripts` is not load-bearing — none of
the missing packages have install scripts that would place files.

## Where this bit us

`dcl-one-sdk` vendors `@dcl/inspector` into a self-contained binary, so it has no sibling tree to
hoist from and sees the bare-install behaviour directly. Our build script carries an explicit
install list for exactly these packages and a resolver check that fails the build if any kept file
has an unresolvable import — see `crates/dcl-one-sdk/scripts/build-inspector-blob.py` and
`crates/dcl-one-sdk/src/vendor/README.md`. That is a workaround, not a position on how you should
package it.
