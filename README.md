# dcl-one-sdk

An npm-free Rust toolchain for building, previewing, and deploying Decentraland
SDK7 scenes; an alternative to `@dcl/sdk-commands`.

Measured on the freshly scaffolded template scene (release build; absolute
times vary with hardware):

- one self-contained binary (48 MB; 80 MB as released with the abgen
  asset-bundle server embedded), 311 tests; the upstream toolchain installs
  315 MB / 17,464 files of node_modules per scene and takes 31.5 s for an
  `npx` cold start
- `init` scaffolds a working scene fully offline in 0.2 s — the vendored
  node_modules (27 MB, 2,970 files) ships inside the binary
- `build` bundles and type-checks in under a second; `start` is serving the
  preview ~0.1 s after launch
- a production scene is a ~1 KB scene chunk beside a shared, immutable
  SDK-runtime chunk, vs upstream's ~938 KB single-file production bundle

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

## Asset bundles (abgen)

`start` also runs an [abgen](https://github.com/eordano/abgen) asset-bundle
sidecar that serves optimized preview assets. Release binaries embed abgen:
on first use `start` extracts it (reused across runs while unchanged) and
serves asset bundles with zero extra installs. The sidecar binary resolves
in order:

1. `ABGEN_BIN` — explicit path, always wins
2. the copy embedded in the dcl-one-sdk binary (release downloads)
3. the scene's `@dcl/abgen` npm platform package
   (`node_modules/@dcl/abgen-<platform>-<arch>`)
4. `abgen` on PATH

When none resolves, preview continues immediately with a one-line hint, and
`--no-asset-bundles` turns the sidecar off.

Source builds embed nothing by default (`cargo build` / `nix build` stay
fast, and the binary is ~32 MB smaller). To reproduce a release binary,
point `ABGEN_EMBED_BIN` at the `abgen` server binary inside an unpacked
release archive (`abgen-v<ver>-<target>.tar.gz` from
<https://github.com/eordano/abgen/releases>; its `template/` and `shader/`
directories must sit next to the binary) and build. The release workflow
(`.github/workflows/release.yml`) does exactly that per target.

## License

AGPL-3.0. See [LICENSE](./LICENSE).

Not affiliated with the Decentraland Foundation.
