# Vendored node_modules

`node_modules.zip` is extracted by `dcl-one-sdk init` so a scaffolded scene builds
and previews with no npm and no network. It contains the scene toolchain the Rust
CLI actually uses: `@dcl/sdk` (with `@dcl/ecs`, `@dcl/react-ecs`, `@dcl/js-runtime`,
`@dcl/ecs-math` and their runtime deps) plus `typescript` for the type check. It is
pure JS — no platform binaries — so one blob serves linux, macOS and Windows.

The current blob is a pure registry install of the released 7.24.5 toolchain — no
overlays. #1450 (single tree-shakeable ecs) shipped upstream, and the released sdk
now provides the `@dcl/sdk/text-codec` module that published `@dcl/asset-packs`
imports, so offline and npm-flow scenes finally see the same packages. The one
regression vs the previous overlaid blob (7.24.4 + PRs #1450/#1451/#1452): the
549 KB `text-encoding` polyfill is back in the tree until #1452 (built-in utf-8
codec) merges upstream — the overlay recipe in step 3 below is the escape hatch
if that wait gets long.

## Regenerating

1. Empty dir with a `package.json` whose `devDependencies` are copied verbatim from
   `src/templates/init/scene/package.json`, plus `ethers` (imported by
   `@dcl/sdk/ethereum-provider` but undeclared in its manifest).
2. `npm install --ignore-scripts --no-audit --no-fund`
3. Optionally overlay toolchain packages: `npm pack` each of
   `packages/@dcl/{sdk,ecs,react-ecs,js-runtime}` in the toolchain worktree,
   extract each tarball over `node_modules/@dcl/<name>`, then rewrite the four
   manifests: `version` to the scaffold pin and any `file:` dep specs to that same
   version.
4. Prune by manifest reachability instead of a fixed list: BFS the `dependencies`
   + `optionalDependencies` graph from `@dcl/sdk`, `@dcl/js-runtime`,
   `typescript`, `protobufjs`, `@protobufjs/utf8`, `ethers` (the last three are
   real imports of `@dcl/ecs`/`@dcl/sdk` dist files that their manifests do not
   declare), skipping `@dcl/sdk-commands` and `@dcl/explorer` (npm-toolchain-only,
   ~160 MB); delete every top-level package not reached. After pruning, scan the
   kept dist files for bare-specifier imports that do not resolve inside the tree —
   the scan must come back empty.
5. Delete symlinks (`find node_modules -type l -delete`), source maps
   (`find node_modules -name '*.map' -delete`), `node_modules/.bin` and
   `node_modules/.package-lock.json` — symlinks break Windows extraction, and a
   stale lockfile misleads later `npm install`s.
6. Zip deterministically (sorted paths, fixed timestamps, deflate per entry —
   python `ZipInfo` defaults to STORED) as `node_modules/...` entries at the
   archive root.
7. Prove it before committing: `dcl-one-sdk init` in an empty dir, then
   `build --production` (rolldown + type check) with a scene importing
   `@dcl/sdk/players` and `@dcl/sdk/network`, then `start` and probe `/about`.

When bumping the `@dcl/sdk` pin, update the pin in both
`src/templates/init/*/package.json` first — the vendored set must match the
scaffold manifest so a later `npm install` converges instead of fighting it.

Smart items (`@dcl/asset-packs`) and the visual editor are not in the default
path; scenes that want them run `npm install` after adding the dependency.
