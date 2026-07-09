# dcl-one-sdk

An npm-free Rust toolchain for building, previewing, and deploying Decentraland
SDK7 scenes; an alternative to `@dcl/sdk-commands`.

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
| `skills` | install the `decentraland/sdk-skills` pack |
| `get-context-files` | fetch the SDK docs corpus into `dclcontext/` |

Run `dcl-one-sdk <command> --help` for options.

## Backends

`build` and `start` bundle with a built-in Rust backend by default; no Node.js is
required. `--use-esbuild` uses an embedded esbuild binary instead (macOS arm64 only).

## License

AGPL-3.0. See [LICENSE](./LICENSE).

Not affiliated with the Decentraland Foundation.
