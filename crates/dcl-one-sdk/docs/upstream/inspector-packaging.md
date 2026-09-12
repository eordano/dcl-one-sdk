# @dcl/inspector: eight undeclared runtime dependencies, and an undeclared node floor

Draft for `decentraland/creator-hub` (`packages/inspector`). Not yet filed.

## Summary

`dist/tooling-entrypoint.js`, the published CommonJS entrypoint, `require()`s eight packages no
`dependencies` chain in the manifest reaches: a standalone install + `require()` fails eight times
running. It works only inside `@dcl/sdk-commands`, where unrelated siblings hoist all eight. 7.37.0
makes it ten; two new ones (`@babylonjs/core`, `@dcl/ecs-math`) are absent from that tree too.

Secondary: no `engines`. The node floor tracks which `node-fetch` major resolves — 20.19.0 under
`node-fetch` 3.x, since a CJS file `require()`s an ESM package. Its single call site is
`fetch(url).then(r => r.text())` behind an almost-always-unset config flag, so deleting the import
drops six packages and the floor problem together.

## Environment

| | |
|---|---|
| `@dcl/inspector` | 7.36.3 (`shasum 7c4f9b44…`, unpacked 125,062,573 B); latest 7.37.0 also checked |
| `@dcl/sdk-commands` | 7.25.0 (pins `"@dcl/inspector": "7.36.3"`) |
| node | v24.18.0 |
| package manager | `corepack pnpm@11.18.0` `--config.node-linker=hoisted --ignore-scripts` (npm-equivalent flat layout; npm blocked here, see *Caveats*) |
| OS | macOS 25.5.0 arm64 |

`dist/tooling-entrypoint.js` in 7.36.3 is 2,316,341 bytes. All attributions below come from the
`dist/tooling-entrypoint.js.map` shipped with it.

# Defect 1 — undeclared runtime dependencies

## Reproduction

```sh
mkdir /tmp/repro && cd /tmp/repro
npm init -y
npm i @dcl/inspector@7.36.3
node -e 'require("@dcl/inspector")'
```

## Expected

Loads. `require("@dcl/inspector")` is the supported entrypoint (`main` -> `dist/tooling-entrypoint.js`),
and `@dcl/sdk-commands` does exactly this at `dist/logic/composite.js:10`,
`dist/commands/start/data-layer/rpc.js:37`, `dist/commands/code-to-composite/composite-generator.js:37`,
`dist/commands/code-to-composite/scene-executor.js:12`.

## Actual

```
Error: Cannot find module 'long'
Require stack:
- /private/tmp/repro/node_modules/@dcl/inspector/dist/tooling-entrypoint.js
```

Installing each missing package and retrying walks the whole set (verbatim run; each step installs
what the previous `MODULE_NOT_FOUND` named):

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

No dynamic `require()` in the file — a scan for `require(` with a non-literal argument returns
nothing — so the census is complete:

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

7.36.3's `dependencies` is exactly `{"@babel/parser": "7.28.5", "@dcl/asset-packs": "^2.17.0",
"ts-deepmerge": "^7.0.0"}` — one of the ten. (`@babel/parser` is not required by
`tooling-entrypoint.js` at all; presumably for `public/bundle.js`, not chased.)

### Where each one enters the bundle

From `tooling-entrypoint.js.map`, each `require()`'s generated offset mapped to its original source:

| specifier | origin |
|---|---|
| `ajv/dist/jtd` | `src/lib/logic/preferences/io.ts:2` |
| `ignore` | `src/lib/data-layer/host/fs-utils.ts:1` |
| `node-fetch` | `src/lib/data-layer/host/utils/install-bin.ts:1` |
| `@well-known-components/pushable-channel` | `src/lib/data-layer/host/stream.ts:1` |
| `long`, `protobufjs/minimal` | `src/lib/data-layer/proto/gen/data-layer.gen.ts:2-3` (ts-proto output) |
| `long`, `protobufjs/minimal`, `@protobufjs/utf8` | inlined `@dcl/ecs/dist/**` (~70 `*.gen.js`, `serialization/ByteBuffer/index.js`, `components/component-number.js`) |
| `fp-future`, `mitt` | inlined `@dcl/mini-rpc/src/{rpc,transport}.ts` |

Two layers to one mistake: `ajv`, `ignore`, `node-fetch`, `pushable-channel` are the inspector's own
source, `long`/`protobufjs` your generated proto; the rest arrive because the bundler inlines
`@dcl/ecs` and `@dcl/mini-rpc` *source* while leaving *their* bare specifiers external — the
externalised set is a property of the build, and nothing reconciles it against `dependencies`.
`build.js` is not in `files`, so the exact `external:` config is an inference; the effect is not.

### Why this has never broken CI

Under a real `@dcl/sdk-commands` 7.25.0 install all eight resolve, from unrelated packages:

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

Each is a version the inspector never chose and does not pin. `ignore`: devDependency `^7.0.5`, tree
gives 5.3.2. `protobufjs`: 7.2.4 here, 8.7.1 bare.

### 7.37.0 makes it worse

Same census on 7.37.0 adds two, both top-level eager requires (`var qo=require("@babylonjs/core")`,
`var Da=require("@dcl/ecs-math")`):

```
      1 require("@dcl/ecs-math")        # devDependency 2.1.0, not a dependency
      1 require("@babylonjs/core")      # devDependency 8.7.0, not a dependency
```

`@dcl/sdk-commands` 7.25.0's tree provides neither, so 7.37.0 fails at `require("@dcl/inspector")`
where 7.36.3 succeeds — also why we pin 7.36.3 downstream: satisfying `@babylonjs/core` means a 3D
engine in a node-side host process, apparently for `Vector3`/`Quaternion` in browser-side snap
helpers.

Separately (own issue, flagged because we tripped over it): with `@dcl/ecs-math` 2.1.0 present,
`require("@dcl/inspector")@7.37.0` on node 24 then fails with

```
Error [ERR_MODULE_NOT_FOUND]: Cannot find module '.../@dcl/ecs-math/dist/Quaternion'
imported from .../@dcl/ecs-math/dist/index.js
```

— extensionless relative imports in an ESM build, a `@dcl/ecs-math` defect.

# Defect 2 — no `engines`, and a node floor that depends on transitive luck

7.36.3 and 7.37.0 publish **no `engines` field at all** and no `"type"`, so
`dist/tooling-entrypoint.js` is CJS.

"The floor is node 20.19" holds **only when `node-fetch` 3.x resolves**. Both directions verified.

**With `node-fetch` 3.3.2** (`"type": "module"`) — what a bare `npm i node-fetch` gives today, i.e.
what anyone hand-fixing Defect 1 lands on:

```sh
node --no-experimental-require-module -e 'require("@dcl/inspector")'
```

```
Error [ERR_REQUIRE_ESM]: require() of ES Module
/private/tmp/repro/node_modules/node-fetch/src/index.js from
/private/tmp/repro/node_modules/@dcl/inspector/dist/tooling-entrypoint.js not supported.
```

Exit 1. `require(esm)` is unflagged from node 20.19.0 / 22.12.0, so that is the floor there.

**With `node-fetch` 2.7.0** (CJS) — what the `@dcl/sdk-commands` tree hoists — the same command
exits 0, printing `REQUIRE OK`. `node-fetch` is thus the *only* ESM-require blocker in the closure;
`long` 5.3.2 is `"type": "module"` but ships a `require` export condition (`./umd/index.js`), so it
is fine.

So **on the default `@dcl/sdk-commands` install `ERR_REQUIRE_ESM` does not reproduce today.** It is
latent — it fires the moment anything in the tree resolves `node-fetch` to 3.x, exactly what a user
or downstream tool adding `node-fetch` to satisfy Defect 1 does. Declaring `"node-fetch": "^2.7.0"`
fixes both; `"^3"` would make Defect 2 real for everyone.

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

The only `node-fetch` call site in the published entrypoint, called once from
`src/lib/data-layer/host/rpc-methods.ts:129` (`await installBin(fs)`) inside `createDataLayerHost`,
returning immediately unless `binIndexJsUrl` is set — de-minified, `getConfig()` gives
`binIndexJsUrl: searchParams.get('binIndexJsUrl') || globalThis.InspectorConfig?.binIndexJsUrl ||
null`, so it defaults to `null` and nothing in the package sets it. `fetch(url).then(r => r.text())`
is signature-identical on global `fetch`, unflagged in node since 18.0.0.

Dropping the import removes six packages plus `@types/node-fetch`: `node-fetch`, `fetch-blob`,
`formdata-polyfill`, `data-uri-to-buffer`, `node-domexception`, `web-streams-polyfill`
(`node-fetch@3.3.2` -> `fetch-blob@3.2.0` -> `node-domexception` + `web-streams-polyfill`;
`formdata-polyfill` -> `fetch-blob`).

# Proposed fix

(1) is the bug fix; (2) and (3) make it stay fixed.

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

We only have the *published* manifest, whose keys npm normalised; the repo file's key order differs,
so apply as five discrete edits, not a patch.

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

Range choices, flagged rather than asserted:

- `protobufjs ^7.2.4` not `^8`: 7.2.4 is what `@dcl/protocol` pins and every `@dcl/sdk-commands`
  install resolves today; a bare `npm i protobufjs` gives 8.7.1, and `^8` would silently change the
  wire codec under existing scenes. Untested against protobufjs 8 beyond confirming it loads.
- `ignore ^7.0.5` matches the existing devDependency, though the tree runs 5.3.2 via
  `@dcl/sdk-commands`. Whichever you pick, picking one is the point.
- `ajv ^8.12.0`: the bundle uses `ajv/dist/jtd` only (JTD, RFC 8927); resolved today is 8.20.0.
- `engines: ">=20.19.0"`: with (1) the *technical* floor drops to 18.0.0 (global `fetch`), but node
  18 is EOL and 20.19.0 is the general `require(esm)` floor, which this package is one dependency
  bump from needing again. To keep `node-fetch`, declare `"node-fetch": "^2.7.0"` in `dependencies`;
  same `engines` value.

### 3. A publish-time smoke test so this cannot regress

This defect class is invisible to `npm test` and to `packages/inspector`'s own build (the monorepo's
`node_modules` has everything), visible only from outside the repo. One CI step catches all ten
cases including the 7.37.0 `@babylonjs/core` regression:

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
ESM-only — the thing `engines` tracks.

## What we verified vs. what we are inferring

**Verified by direct execution or by reading the published artifact:**

- The `require()` census for 7.36.3 and 7.37.0; no `require()` takes a non-literal argument.
- 7.36.3's published `dependencies`, absent `type`, absent `engines` (`registry.npmjs.org` metadata
  + the installed package).
- The eight-step bare-install failure sequence, in order.
- `require("@dcl/inspector")` succeeding under a real `@dcl/sdk-commands@7.25.0` install, and which
  sibling declares each of the eight.
- `ERR_REQUIRE_ESM` under `node-fetch@3.3.2`, gone under `2.7.0`, with
  `--no-experimental-require-module` on node 24.18.0.
- `install-bin.ts` + its single call site `rpc-methods.ts:129`, from the published sourcemap.
- `node-fetch@3.3.2`'s six-package closure, from registry manifests.
- 7.37.0's `@babylonjs/core` and `@dcl/ecs-math` requires being top-level and eager.

**Inferred, not verified:**

- Node 18 behaviour — not run; `--no-experimental-require-module` on node 24 is the documented
  emulation of pre-20.19 `require()` semantics, treated as equivalent.
- That the cause is an esbuild `external:` list unreconciled against `dependencies` — `build.js` is
  unpublished and we did not read the repo, so this is reasoning from the artifact.
- That `@babel/parser` and `@dcl/asset-packs` are `dependencies` for `public/bundle.js`; neither is
  required by `tooling-entrypoint.js`.
- Whether upstream intends `protobufjs` 7 or 8, `ignore` 5 or 7 — we picked what today's trees
  resolve; only you can say which is correct.
- Whether anything outside `tooling-entrypoint.js`'s reach relies on `node-fetch`'s v2-specific API
  surface; we checked only the published node entrypoint.

## Caveats on our environment

`npm` is blocked machine-wide here, so every install above was `corepack pnpm add --ignore-scripts
--config.node-linker=hoisted <pkg>`, producing the flat `node_modules` layout npm does. These are
resolution failures against that layout, so they should reproduce identically under npm; unconfirmed
by us. `--ignore-scripts` is not load-bearing — none of the missing packages have install scripts
that place files.

## Where this bit us

`dcl-one-sdk` vendors `@dcl/inspector` into a self-contained binary, so it has no sibling tree to
hoist from and sees bare-install behaviour directly. Our build script carries an explicit install
list for exactly these packages plus a resolver check failing the build on any unresolvable import
in a kept file — `crates/dcl-one-sdk/scripts/build-inspector-blob.py`,
`crates/dcl-one-sdk/src/vendor/README.md`. A workaround, not a position on how you should package it.
