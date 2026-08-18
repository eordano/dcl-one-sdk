# dcl-one-sdk

One binary that builds, previews and publishes Decentraland SDK7 scenes.

It replaces `@dcl/sdk-commands` — the npm CLI a scene normally depends on — with
a single Rust executable. There is nothing to `npm install`: the bundler is
rolldown compiled in-process, the TypeScript toolchain a scene needs is
vendored inside the binary and extracted on demand, and the abgen asset-bundle
converter is embedded too. A machine with this binary and nothing else can
scaffold a scene, build it, serve a live preview to the desktop client, and
deploy it to a catalyst.

**Compared with upstream.** The parity target is `@dcl/sdk-commands` 7.26.0,
which is npm `latest`; the vendored toolchain and the `init` scaffold pin the
same line, and scenes still on 7.22.6 keep working because every behaviour
change ported here is backward-compatible with them. Commands, flags and output
match upstream closely enough to drop into a supervisor that invokes the npm
CLI, including flags this binary accepts and ignores. Where it deliberately
differs: builds are in-process rather than shelling out to node, `main.crdt` is
generated natively instead of through `@dcl/inspector`, the preview runs an
asset-bundle sidecar upstream has no equivalent of, content hashes are
content-addressed rather than path-derived, and scene runtime errors are pulled
out of the running client and printed in your terminal. Each of those is called
out below where it matters.

---

## Start a scene

```
dcl-one-sdk init [--dir D] [--project scene|smart-wearable] [-y|--yes]
```

Writes a scene from templates embedded in the binary — no network. You get
`scene.json`, a `package.json` whose scripts call `dcl-one-sdk`, a `tsconfig`
extending `tsconfig.ecs7.json`, `src/index.ts`, `.gitignore`, `.dclignore`, a
README and a navmap thumbnail. It refuses a non-empty directory unless you pass
`--yes`, and on a terminal it asks which kind of project you want.

The vendored `node_modules` is extracted from the binary at the same time.
`--node-modules-only` restores it into a project that already exists.

`--project smart-wearable` scaffolds a wearable instead: a `wearable.json`
skeleton with a generated UUID, the full 10×10 portable-experience parcel grid,
and a `pack` script.

One gap worth knowing: the vendored blob carries a stand-in for
`@dcl/inspector` that covers the crdt dump and a minimal data-layer host, but
not the editor's browser UI. `/inspector/*` needs a real
`npm install --save-dev @dcl/inspector`, or `DCL_ONE_INSPECTOR_DIR` pointing at
one.

### AI context files

```
dcl-one-sdk get-context-files [--dir D] [--offline]
```

Installs the context a scene hands to a coding agent, in two halves. The
embedded half writes `.claude/skills/migrate-smart-items-to-code/` out of the
binary — that path is where Claude Code discovers project skills, so an agent
picks it up without being told. The downloaded half fetches Decentraland's
`ai-sdk-context` corpus into `dclcontext/`. An unreachable GitHub is a note,
not a failure: the skill is still installed and the command still exits 0.
`--offline` skips the request; `DCL_ONE_SDK_CONTEXT_API` overrides the API base.

---

## Build it

```
dcl-one-sdk build [--dir D] [-p|--production] [-w|--watch]
                  [--ignoreComposite] [--customEntryPoint]
                  [--skip-install] [--skip-type-check]
```

Bundles the scene into `bin/index.js` and type-checks it. At a
`dcl-workspace.json` root it builds every member in order.

Two things happen here that upstream does differently.

**Type checking runs beside the build, not in front of it.** tsc keeps its
state in `.dcl-cache/tsbuildinfo`, so it stops re-checking all of `@dcl/sdk`'s
declarations on every run — roughly 790 ms down to 390 ms on a real scene. A
missing or stale info file costs exactly one full check. `--skip-type-check`
turns it off; the bundle is written either way.

**`main.crdt` is generated in Rust.** Composites are parsed and encoded
natively, including components that carry their own `jsonSchema` — which is
every Creator Hub scene, since `core-schema::Network-Entity` alone triggers it.
That used to fall back to a node round trip costing about 355 ms of a 900 ms
build. The node path still exists and still runs for the cases the native
encoder does not cover: nested composites, binary composite entries, and
component names it does not recognise. `--ignoreComposite` skips regeneration
entirely. Set `DCL_ONE_CRDT_VERIFY=1` to run the node data-layer alongside the
native encoder and log whether the bytes match.

`--customEntryPoint` bundles `scene.json`'s `main` verbatim instead of
generating the loader stub.

---

## Preview it

```
dcl-one-sdk start [--dir D] [-p|--port N] [--skip-build] [--no-watch]
                  [-m|--mobile] [--data-layer] [--offline-comms]
                  [--no-asset-bundles] [--mcp --mcp-port N]
                  [--tunnel WSS_URL] [-- EXPLORER_PARAMS...]
```

Builds the scene, serves it as a local realm on port 8000 (or the next free
port), and reloads it in the running client when you save. Comms is on by
default. `--skip-install` and `--no-browser` are accepted and ignored, so
supervisors that pass upstream's flags keep working.

The banner prints the ways in: a `decentraland://` deep link for the desktop
client, LAN addresses for another device on the network, a web-explorer URL,
and with `-m` a QR code for a phone. Open `http://127.0.0.1:8000` in a browser
and you get a page with all of them, the scene's parcels and spawn points, its
permissions, a request log, the other routes this server exposes, and the
`deploy` command for this scene.

**Asset bundles.** An abgen sidecar runs by default on 5147, converting models
on demand so the client renders optimised assets instead of raw GLBs. Every
deep link carries `local-ab=true`, which makes the client fetch
`{realm}/optimized-assets` — proxied by this server to the sidecar. That is one
port and one firewall approval instead of two, and a LAN or tunnel guest needs
no second reachable address. `--no-asset-bundles` turns the sidecar off and
stops forwarding the flag; the two move together, because forwarding it with no
sidecar points the client at a route that answers 503. `--asset-bundles` is
accepted for upstream parity and does nothing, since it now describes the
default.

**Scene errors in your terminal.** Run with `--mcp --mcp-port N` — both are
required — and a scene that throws prints here instead of vanishing into the
client's log:

```
  ✘ scene error in src/index.ts:41
    TypeError: Cannot read properties of undefined (reading 'gallery')
   41 │   return layout.gallery.shelf
                        ^
    at src/index.ts:45:17
   45 │   const shelf = buildShelf()
```

Nothing is injected into your bundle. The client already collects its scene log
and exposes it over the MCP server it runs when `--mcp` is set; this polls that.
Frames resolve through the bundle's inline source map, so the quoted line comes
out of the bundle rather than your source tree. Frames in generated code are
dropped and library frames stay dim. `--error-source-lines-context N` (and
`-before` / `-after`) widen the quote; the default is 0, just the line that
threw.

**Live reload.** Saving a `.ts` rebuilds the scene chunk in a few milliseconds
and tells the client to reload. Saving a `.glb` sends a message naming that one
file. Content hashes include a digest of the file's bytes, so an edited asset
gets a new hash and no cache — ours or the client's — can serve the old one.

---

## Edit it visually

```
dcl-one-sdk start --data-layer
```

Serves the Creator Hub data-layer protocol at `/data-layer`, so the visual
editor can drive the scene while it runs. With `@dcl/inspector` installed the
editor UI is served at `/inspector/` as well; without it, the protocol still
works and the UI is simply absent.

---

## Share it

Anyone on your network can join through the LAN address in the banner. For
someone who is not, `--tunnel` exposes the preview through a relay:

```
# on a public host
catalyrst-preview-tunnel --listen 0.0.0.0:9000 --token SECRET

# on your machine
dcl-one-sdk start --tunnel wss://tunnel.example:9000 --tunnel-token SECRET
```

Prefer `--tunnel-token-file` or `DCL_ONE_SDK_TUNNEL_TOKEN` over the flag — a
token passed as an argument is visible in `ps` and in shell history.

---

## Publish it

```
dcl-one-sdk deploy [--dir D] [-t|--target CATALYST] [--target-content URL]
```

Builds, packages and signs the scene, then uploads it. Signing happens in a
browser: deploy starts a small page on a throwaway port, waits for your wallet,
and shuts it down once the signature comes back. `DCL_PRIVATE_KEY` signs
headlessly instead, for CI.

Without a target it walks a rotation of public catalysts until one is healthy.
A world scene needs an explicit `--target-content`, because publishing a world
to a random Genesis City catalyst is not what you meant.
`DCL_ONE_SDK_DEFAULT_TARGET` sets your own default.

Related: `unpublish` takes a scene down, `pack` builds the `.zip` a smart
wearable is submitted as, and `world` reads and writes a world's settings and
permissions on a worlds content server.

---

## Reference

**Ports.** 8000 preview server (or next free), 5147 abgen sidecar (or random),
5141 a local catalyrst if you run one.

**Files written into the scene.** `bin/` holds the built chunks.
`.dcl-one/` holds the generated entrypoint and composite index. `.dcl-cache/`
holds the tsc info file and fetched upstream content. `.dcl-optimized-assets/`
holds abgen's output and JIT cache. All are watcher-ignored, and none are
deployed.

**Environment.** `DCL_ONE_SDK_CATALYST` (falling back to upstream's
`DCL_CATALYST`) picks where profiles, wearables and avatars come from; it
defaults to `https://interconnected.online`, and without a reachable catalyst
the client shows no avatars. `DCL_ONE_SDK_CATALYST_ROTATION` overrides the
deploy rotation. `DCL_ONE_SDK_WORLD_BASE` names a worlds host for the
`/world/…` mirror. `DCL_ONE_SDK_FEATURE_FLAGS` names a feature-flag host.
`DCL_ONE_SDK_CONTENT_CACHE_MAX` bounds the fetched-content LRU.
`DCL_ONE_SDK_WEB_EXPLORER` overrides the web explorer URL.
`DCL_ONE_SDK_ALLOWED_ORIGINS` widens CORS. `ABGEN_BIN` runs a different
sidecar; every other `ABGEN_*` variable this binary sets is env-wins, so
exporting one overrides it.

**Error output.** Failures print what went wrong, why, and what to try, and the
set of them is pinned by tests — an error that loses its guidance fails the
build rather than shipping.

**Golden snapshots.** Eight fixture scenes are built and snapshotted into
`testdata/golden/`: artifact sizes and hashes, the generated entrypoint, decoded
`main.crdt` messages, deploy CIDs, and a runtime trace of per-frame CRDT
traffic. Any change to the bundler, the entrypoint generator or the crdt
encoder shows up as a reviewable diff. Regenerate with
`scripts/update-goldens.sh`.

**Security posture of the preview server.** It binds a local port and serves
unauthenticated routes, so it is meant for your machine and your LAN, not the
public internet without a tunnel you control. The landing page carries no
JavaScript and no form: it cannot be made to mutate server state by a page you
happen to have open. `/content/contents/{hash}` serves only files the scene
actually publishes, so a `.dclignored` file cannot be read back out.
