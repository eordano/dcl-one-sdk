# dcl-one-sdk

An npm-free Rust toolchain for building, previewing, and deploying Decentraland
SDK7 scenes; an alternative to `@dcl/sdk-commands`.

Measured on the freshly scaffolded template scene (release build; absolute
times vary with hardware):

- one self-contained binary — 34 MB from `cargo build`, ~32 MB larger from
  `nix build` or a release, which embed the abgen asset-bundle server — and 280
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

`start` also runs an [abgen](https://github.com/decentraland/abgen)
asset-bundle sidecar that serves optimized preview assets. On first use `start`
extracts the embedded copy (reused across runs while unchanged) and serves
asset bundles with no extra installs. The sidecar binary resolves in order:

1. `ABGEN_BIN` — explicit path, always wins
2. the copy embedded in the dcl-one-sdk binary
3. the scene's `@dcl/abgen` npm platform package
   (`node_modules/@dcl/abgen-<platform>-<arch>`)
4. `abgen` on PATH

When none resolves, preview continues immediately with a one-line hint, and
`--no-asset-bundles` turns the sidecar off.

**Which builds embed it.** `nix build` embeds abgen — the flake takes it as an
input and assembles the layout the embed needs, so a nix-built binary and a
released one behave the same. `cargo build` embeds nothing, which keeps a
source build fast and about 32 MB smaller; that binary still runs the sidecar
if any of steps 1, 3 or 4 above resolves.

To embed by hand, point `ABGEN_EMBED_BIN` at the `abgen` server binary inside
an unpacked release archive (`abgen-v<ver>-<target>.tar.gz` from
<https://github.com/decentraland/abgen/releases>) and build. Its `template/`
and `shader/` directories must sit next to the binary — the build script
checks all three and fails loudly rather than embedding a half-archive.

## License

AGPL-3.0. See [LICENSE](./LICENSE).

Not affiliated with the Decentraland Foundation.
