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
asset-bundle sidecar upstream has no equivalent of, a preview content hash
carries a digest of the file's bytes rather than being derived from its path
alone, and scene runtime errors are pulled out of the running client and
printed in your terminal. Each of those is called out below where it matters.

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
                  [--no-asset-bundles] [--no-mcp] [--mcp-port N]
                  [--tunnel WSS_URL] [-- EXPLORER_PARAMS...]
```

Builds the scene, serves it as a local realm on port 8000 (or the next free
port), and reloads it in the running client when you save. Comms is on by
default. `--skip-install` and `--no-browser` are accepted and ignored, so
supervisors that pass upstream's flags keep working.

The banner prints the ways in: a `decentraland://` deep link for the desktop
client, LAN addresses for another device on the network, a web-explorer URL,
and with `-m` a QR code for a phone. Open `http://127.0.0.1:8000` in a browser
and you get one launch button with a column of knobs beside it, plus the
scene's parcels and spawn points, its permissions, and the log of the requests
this server has answered.

The knobs are: **where** it opens (this machine, another device, the web
explorer, a phone), whether the link asks the client to open its **MCP port**,
which **spawn point** to land on, and checkboxes for the deep-link params worth
having in a preview loop — `multi-instance` (a second client beside the first),
`skip-auth-screen`, `landscape-terrain-enabled`, `hub`, `force-open-backpack`.
There is no free-text field. The page builds the link only out of controls it
drew: a query key it does not own, or a value it never offered, is dropped
before the link exists, so the page cannot be made to produce a param the
client would refuse anyway.

That checkbox list is short on purpose. The client declares around ninety
launch flags but accepts only sixteen from a deep link (`DeepLinkAllowlist.cs`):
eight for any realm, and eight more only when the target realm is
`Uri.IsLoopback`. A key in neither set is dropped for every realm, so a checkbox
for one would promise a change the client silently discards. Of the sixteen,
seven are set for you (`realm`, `position`, `local-scene`, `dclenv`, `local-ab`
while the abgen sidecar runs, and the `mcp`/`mcp-port` pair the MCP knob turns
off), six are the knobs above (the five checkboxes plus `spawnpoint`), and the
last three — `community`, `signin`, `authRequestId` — are login and
notification intents with no place in a preview.

Loopback is not a given, because one of the targets is not: the LAN address
another device dials is routable, so that client keeps only
`force-open-backpack` and `spawnpoint` and drops the whole loopback tier —
including `local-scene` and `local-ab`. The web and phone links read no
deep-link params at all. The page
shows this instead of hiding it: pick a target and the knobs its client would
throw away go grey with the reason beside them, and they are left out of the
link rather than sent and discarded.

The page renders one launch card — the selected target — and one `GET` form
back to itself holding every knob, so a change costs one request and no
JavaScript. A knob the selected target cannot use keeps its value in a hidden
field, so switching back finds it still set.

**Asset bundles.** An abgen sidecar runs by default on 5147, converting models
on demand so the client renders optimised assets instead of raw GLBs. Every
deep link carries `local-ab=true`, which makes the client fetch
`{realm}/optimized-assets` — proxied by this server to the sidecar. That is one
port and one firewall approval instead of two, and a LAN or tunnel guest needs
no second reachable address. What that guest does *not* get is the flag: like
`local-scene`, `local-ab` is in the client's loopback-only tier, so a client
launched at the LAN or tunnel realm drops it and renders raw GLBs however the
sidecar is configured; the optimisation is real for the machine running the
preview. `--no-asset-bundles` turns the sidecar off and stops forwarding the
flag; the two move together, because forwarding it with no sidecar points the
client at a route that answers 503. `--asset-bundles` is
accepted for upstream parity and does nothing, since it now describes the
default.

**Scene errors in your terminal.** On by default — a scene that throws prints
here instead of vanishing into the client's log:

```
  ✘ scene error in src/index.ts:41
    TypeError: Cannot read properties of undefined (reading 'gallery')
   41 │   return layout.gallery.shelf
                        ^
    at src/index.ts:45:17
   45 │   const shelf = buildShelf()
```

Nothing is injected into your bundle. The client already collects its scene
log and exposes it over an MCP server, which the deep link asks it to start
(`mcp=true&mcp-port=…`, the Explorer's own default port 8123); this polls that.
`--no-mcp` turns both halves off together — the link stops asking for the port
and nothing here reads it — and `--mcp-port N` moves it, within `1024-65535`.
That range is the client's (`McpServerPlugin`): a port outside it makes the
client fall back to 8123 while this side keeps polling the port you named, so
it is refused at parse time rather than turned into silence. `mcp` is in the
loopback-only tier, so this reaches the "this machine" link and not the LAN
one. One more clash is refused rather than run: `--mcp-port` equal to the port
this preview bound would aim the poller at this server, so the poller is
skipped and the collision is printed with the three ways out of it.
Frames resolve through the bundle's inline source map, so the quoted line comes
out of the bundle rather than your source tree. Frames in generated code are
dropped and library frames stay dim. `--error-source-lines-context N` (and
`-before` / `-after`) widen the quote; the default is 0, just the line that
threw.

**Live reload.** Saving a `.ts` rebuilds the scene chunk in a few milliseconds
and tells the client to reload. Saving a `.glb` sends a message naming that one
file. Content hashes include a digest of the file's bytes, so an edited asset
gets a new hash and the client refetches that one asset instead of dropping
its whole cache.

Only that half is content addressing, and the asymmetry is worth knowing
before you build anything on these hashes. The digest names a version; it does
not pin one. `/content/contents/{hash}` resolves on the path part of the hash
and never compares the digest, so a URL carrying a superseded digest is
answered `200` with the file's **current** bytes — not `404`, and not the bytes
that digest named. The preview keeps no old versions, so there are no other
bytes to serve, and failing the request would break a fetch that was already in
flight when the watcher rewrote the file. If you need the exact bytes a hash
was minted for, this server cannot give them to you.

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
browser, on the preview server itself: deploy brings up the same server
`start` runs — the scene about to go up is walkable from the same origin while
you decide — and the landing page leads with the signing panel. The panel is
server-rendered down to the entity id, which is minted at render time and is
exactly the id the wallet signs; the browser's only job is the wallet
hand-off, and the server goes down once the signature comes back.
`DCL_PRIVATE_KEY` signs headlessly instead, for CI, and
`DCL_ONE_SDK_LINKER_HOST=0.0.0.0` opens the signing gate to another device.

While a preview is running, `/deploy` — the button in the page header — answers
what this command would do before you run it: which target it would pick (your
world's content server, your `DCL_ONE_SDK_DEFAULT_TARGET`, or with neither the
public Genesis City rotation it would walk — the rotation, not a resolved
address, because health is only known at deploy time), how many files and how
many megabytes would go up, the largest of them, and which files `.dclignore` is
keeping out (the ones from directories that are published, not the contents of
`node_modules`). It also previews the three refusals `deploy` itself raises
*after* the wallet has signed: a scene not built yet, a file over the 50 MB
per-file limit, and two names a content server would read as one. It reads no
file bytes — only the directory entries' sizes, memoised for half a second — so
it costs milliseconds on a scene of any size, and it is a page rather than a
button because a publish route on an unauthenticated port would be a publish
button for anyone who can reach it. The entity id is not shown: it hashes a
timestamp minted at deploy time, so any value here would be a different one.
`--dry-run` prints an id, but it mints its own timestamp too — only
`--timestamp` makes that id the one a real deploy would produce.

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

**Running the tests.** `cargo test -p dcl-one-sdk` passes with nothing installed
but a toolchain and `node`. node is not `#[ignore]`d away: the golden suite's
runtime tier needs it, and so does `build`'s own type check, so a machine
without it cannot use this tool at all and a red test is the honest answer. The
suites that need a resource this repo cannot ship — an installed scene
`node_modules`, a provisioned scene checkout, a live tunnel — are
`#[ignore]`d with a reason, so the harness names what it did not run instead
of counting it as a pass. `docs/testing.md` lists every variable that turns one
on (`DCL_ONE_SDK_TEST_NODE_MODULES`, `DCL_ONE_SDK_TEST_SCENE`,
`DCL1_TUNNEL_PUBLIC_URL`) and what `ALLOW_SKIPPED_INTEGRATION` costs you.

**Golden snapshots.** Seven fixture scenes under `testdata/golden/` are built
and snapshotted into eight goldens (`cube` twice, development and production):
artifact sizes and hashes, the generated entrypoint, decoded `main.crdt`
messages, deploy CIDs, and a runtime trace of per-frame CRDT traffic. Any
change to the bundler, the entrypoint generator or the crdt encoder shows up as
a reviewable diff. Regenerate with
`UPDATE_GOLDEN=1 cargo test -p dcl-one-sdk --test golden`.

**Security posture of the preview server.** It binds a local port and serves
unauthenticated routes, so it is meant for your machine and your LAN, not the
public internet without a tunnel you control. The landing page carries no
JavaScript, and its one form is a `GET` back to itself: it cannot be made to
mutate server state by a page you happen to have open. That form is
allowlist-only in both directions — a query key the page does not own, and a
value it never drew, are both ignored — so what it accepts reaches only the deep
links the page draws, never the realm they point at: a core param (`realm`,
`position`, …) keeps the value this server chose, and nothing a visitor sends is
reflected back into the page at all. `/content/contents/{hash}` serves only
files the scene actually publishes, so a `.dclignored` file cannot be read back
out. That check
is on the path the hash names, not on its digest: a hash grants access to a
file, not to one version of it, and it keeps working until that file stops
being published.
