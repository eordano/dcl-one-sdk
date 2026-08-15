# dcl-one-sdk

An npm-free Rust toolchain for building, previewing, and deploying Decentraland
SDK7 scenes; an alternative to `@dcl/sdk-commands`.

Measured on the freshly scaffolded template scene (release build; absolute
times vary with hardware):

- one self-contained binary — 49 MB, the same from `cargo build`, `nix build`
  or a release, all of which embed the abgen asset-bundle server — and 283
  passing tests (`ALLOW_SKIPPED_INTEGRATION=1 cargo test`; three more need a
  live tunnel, a scene, or a node_modules tree, and fail loudly rather than
  skip silently when you point them at one). The upstream toolchain installs
  315 MB / 17,464 files of node_modules per scene and takes 31.5 s for an
  `npx` cold start
- `init` scaffolds a working scene fully offline in about 0.2 s — the vendored
  node_modules (422 files, ~14 MB unpacked) ships inside the binary as a 2.3 MB
  zip
- `build` bundles and type-checks in about half a second; `start` is serving the
  preview ~0.1 s after launch
- a production scene is a ~1 KB scene chunk and a 5.7 KB loader stub beside a
  shared, immutable 464 KB SDK-runtime chunk, vs upstream's ~938 KB single-file
  production bundle

## Install

```sh
nix run github:eordano/dcl-one-sdk -- --help
```

Build from source:

```sh
nix build                    # -> result/bin/dcl-one-sdk
cargo build -p dcl-one-sdk
```

## Usage

```sh
dcl-one-sdk init --dir my-scene --project scene -y
cd my-scene
dcl-one-sdk build
dcl-one-sdk start
dcl-one-sdk deploy --target peer.decentraland.org
```

## Commands

| command | description |
|---|---|
| `init` | scaffold a scene or smart-wearable project |
| `build` | bundle and type-check the scene |
| `start` | run a local preview server with live reload |
| `deploy` | hash, sign, and upload the scene to a catalyst or worlds server |
| `unpublish` | remove a published LAND scene from a dcl-one-style content server |
| `pack` | pack a smart wearable into `smart-wearable.zip` |
| `world` | manage worlds-server settings and permissions |
| `get-context-files` | fetch the SDK docs corpus into `dclcontext/` |

Run `dcl-one-sdk <command> --help` for options.

## Node.js

`build` and `start` bundle with rolldown compiled into the binary — no npm and
no per-scene JS toolchain in the bundle path. Node is used for the TypeScript
type check (the scene's own vendored `typescript` runs under node;
`--skip-type-check` builds without it) and for the visual editor and
`main.crdt` regeneration (`--data-layer` / composite scenes).

The scaffolded `package.json` declares `engines.node ">=24"` (and `npm ">=11"`,
which is what node 24 ships) — that is the version this toolchain is built and
tested against. The hard floor the vendored packages impose is lower, 20.19,
where node's `require(esm)` support became unflagged.

## Visual editor

The editor UI is not vendored. `start --data-layer` needs `@dcl/inspector` in
the scene (`npm install --save-dev @dcl/inspector`), or `DCL_ONE_INSPECTOR_DIR`
pointing at a package that contains a build; everything else — `build`,
`start`, `deploy` — works without it. Upstream ships an 18 MB pre-built browser
bundle that is ~60% a non-tree-shaken Babylon plus ~3,000 Font Awesome icons,
and nothing downstream can slim it, so carrying it in every binary for a path
most scenes never take was the wrong trade.

## Asset bundles (abgen)

`start` runs an [abgen](https://github.com/decentraland/abgen) asset-bundle
sidecar that serves optimized preview assets, and forwards its URL to the
Explorer as `optimized-assets-url`. There is nothing to install: **every**
dcl-one-sdk binary embeds abgen, whatever it was built with. On first run it
unpacks into a temp directory keyed by a content hash and is reused from then
on.

`--no-asset-bundles` turns the sidecar off — that is upstream `sdk-commands`
behaviour, which has no sidecar at all.

`--asset-bundles` matches upstream's flag of the same name and forwards
`local-ab=true`. That flag does not ask the Explorer to convert anything: per
`AppArgsFlags.LOCAL_AB` in unity-explorer it "carries no URL or port", so the
client appends `/optimized-assets` to the realm it already has and expects the
PREVIEW SERVER to serve it. This server does, proxying that path to the
sidecar — so the sidecar keeps running and only the addressing changes:
`local-ab=true` instead of `optimized-assets-url`. Everything then arrives over
one origin, which on a LAN join means no second port and no second firewall
approval.

`ABGEN_BIN` overrides the embedded copy at run time. It is for advanced use —
an abgen you built yourself, a bisect, a test — and nothing else consults the
environment, PATH, or the scene's `node_modules`.

**Where the bytes come from.** `abgen-release.lock` pins an upstream release
and the sha256 of its archive per target. `build.rs` downloads that archive
into a shared cache (`$CARGO_HOME/dcl-one-sdk-abgen`, or `ABGEN_EMBED_CACHE`),
verifies the hash, and embeds the executable deflate-compressed — 13 MB in the
binary for 36 MB on disk. `nix build` parses the same lock file and fetches the
same archive, so a nix-built binary and a `cargo build` one carry identical
bytes. Repin with `scripts/pin-abgen.sh <tag>`.

A release abgen is one self-contained file: the Unity templates and shader
bundles are compiled into it, which `/health` confirms with
`template_ok: true, templates_missing: []`.

`ABGEN_EMBED_BIN=<path>` embeds a different abgen instead of downloading — the
escape hatch for building offline, for a musl host (the pinned Linux archives
are glibc-linked), and for testing an abgen from source.

## Upstream parity notes

`start` serves the same preview surface as `@dcl/sdk-commands` 7.26.0, with two
deliberate differences, both about not baking in someone else's infrastructure:

- **`/feature-flags/{file}`** — upstream proxies to a hardcoded
  `https://feature-flags.decentraland.zone`. Here the host comes from
  `DCL_ONE_SDK_FEATURE_FLAGS`; unset, the route answers 501 and says so.
  Nothing else is affected.
- **`/world/{name}/about`** — same story, via `DCL_ONE_SDK_WORLD_BASE`.

`DCL_ONE_SDK_CATALYST` follows the same rule: it defaults to a **local** content
server (`http://127.0.0.1:5141`), not a public catalyst, so a preview never
silently sources its realm from production. With nothing listening there,
back-fill routes such as `/lambdas/profiles` return 502 until you name an
upstream — that is the intended posture, not a failure.

`/preview-wearables` is implemented for older explorer builds even though
upstream's own source marks it for removal in favour of
`/content/entities/active`, which this server also serves.

## License

AGPL-3.0. See [LICENSE](./LICENSE).

Not affiliated with the Decentraland Foundation.
