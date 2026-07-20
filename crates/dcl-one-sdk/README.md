# dcl-one-sdk

Binary-compatible Rust replacement for `@dcl/sdk-commands`: bundle (rolldown,
in-process), preview-serve, and deploy Decentraland SDK7 scenes. Parity target:
`@dcl/sdk-commands` 7.24.5 (npm latest; the vendored node_modules and init
scaffold pin the same line). Scenes still on 7.22.6 (project-realm-template /
editor-scene) remain covered — every 7.22.6→7.24.5 behavior change ported here
is backward-compatible with them.

## Commands

- `dcl-one-sdk init [--dir D] [--project scene|smart-wearable] [-y|--yes]` -
  embedded templates, no network; refuses a non-empty dir without `--yes`;
  prompts for project kind on a TTY, else defaults to `scene`. Scene
  scaffold: scene.json, package.json with exact-pinned `@dcl/sdk`
  devDependencies and npm scripts that call `dcl-one-sdk`, tsconfig extending
  `tsconfig.ecs7.json`, src/index.ts, .gitignore, .dclignore, README, navmap
  thumbnail; a vendored node_modules is extracted from the binary
  (`--node-modules-only` restores it into an existing project).
  Smart-wearable scaffold: `wearable.json` skeleton with generated UUID
  `id`, the full 10x10 portable-experience parcel grid, the `pack` npm
  script, a README naming the model.glb / thumbnail.png to supply.
- `dcl-one-sdk get-context-files [--dir D]` - recreates `dclcontext/` flat
  from the `decentraland/documentation` `ai-sdk-context` corpus via the
  GitHub contents API (network required); exits 0 with guidance outside a
  project; API base override: `DCL_ONE_SDK_CONTEXT_API`.
- `dcl-one-sdk build [--dir D] [-p|--production] [-w|--watch] [--ignoreComposite]
  [--customEntryPoint] [--skip-install] [--skip-type-check]` - at a
  `dcl-workspace.json` root builds every member in order with upstream's dim
  `[i/n] in <folder>:` header; `-w|--watch` runs one watch session per member.
- `dcl-one-sdk start [--dir D] [-p|--port N] [--skip-build] [--skip-install] [-w|--no-watch] [--ci] [--ignoreComposite] [--offline-comms] [-m|--mobile] [--data-layer] [--no-asset-bundles] [--tunnel WSS_URL|help] [--tunnel-token TOKEN] [--tunnel-token-file PATH]` -
  accepts and ignores `--no-browser`, `--mini-comms`, `--skip-install` for
  drop-in compat with supervisor-managed preview service invocations that
  pass the upstream flags. Comms is ON by default;
  `--data-layer` enables the visual editor (Creator Hub section). An abgen
  asset-bundle sidecar runs by default; the binary resolves in order from
  `ABGEN_BIN`, the abgen embedded at compile time (`src/abgen_embed.rs`:
  release builds set `ABGEN_EMBED_BIN` to an unpacked abgen release archive
  — server binary with `template/` + `shader/` siblings, the exe-dir
  fallback needs no env; it extracts under
  `<temp>/dcl-abgen/bin/<content-tag>/` and is reused byte-verified across
  runs; unset at compile time = empty embed, dev builds stay fast), the
  scene's `@dcl/abgen` npm platform package, then PATH. A missing or
  crashing abgen prints a one-line hint and preview starts immediately, and
  `--no-asset-bundles` turns the sidecar off. Trap: the abgen RELEASE
  archives' `abgen` is the JIT server (`catalyrst-abgen` bin renamed by the
  abgen export assembler); the in-tree `--bin abgen` is the one-shot
  converter CLI and exits on boot if embedded. After bind
  it prints the JOIN BLOCK: per-interface realm URLs classified via
  `src/netinfo.rs` (loopback / LAN / overlay-VPN / virtual bridge,
  link-local skipped), each address self-probed for reachability; web +
  desktop-deep-link + second-instance (`&multi-instance=true`) + native
  rows; the second-identity note (same address = kicked by the relay); a
  mixed-content warning with the loopback `ssh -L` workaround; warnings for
  loopback-only and NAT-VM guest (`10.0.2.15`). Every URL handed out
  (`/about` fixedAdapter, `content.publicUrl`, `scenesUrn` baseUrl) is
  Host-header-derived, so whatever address a client dials is the address
  comms + content flow through. `--mobile` adds a terminal QR
  of the LAN mobile deep link. Web-row explorer base:
  `DCL_ONE_SDK_WEB_EXPLORER`. At a `dcl-workspace.json` root all members are
  served in ONE realm. `--tunnel <wss-url>` / `--tunnel-token` / `--tunnel
  help`: see the tunnel section.
- `dcl-one-sdk deploy [--dir D] [-t|--target CATALYST] [--target-content URL]
  [--sign-key PATH] [--skip-build] [--dry-run] [--timestamp MS] [--entity-out PATH]
  [--multi-scene] [-y|--yes] [-b|--no-browser] [--ci] [-p|--port N]`
  - signing: `--sign-key` (wins) or `DCL_PRIVATE_KEY` (env use announced on
    stderr) sign headlessly; otherwise a
    local signing page is served (`/api/info` re-mints a fresh entity per
    page load, `personal_sign(entityId)`, POST `/api/sign` uploads) and its
    URL is ALWAYS printed (gate `g13`; upstream's `--no-browser` silent hang
    cannot recur). Browser opens unless `--no-browser`/`--ci`; wait times
    out after 10 minutes (`DCL_ONE_SDK_LINKER_TIMEOUT_SECS` overrides);
    `--port` pins the page's port (default: any free port).
  - target resolution (upstream `getCatalyst` semantics): `--target-content
    <url>` verbatim; `--target <catalyst>` gets `https://` prepended when
    scheme-less, is probed via `GET /about`, deploys to `content.publicUrl`;
    both together rejected. With neither: `DCL_ONE_SDK_DEFAULT_TARGET`
    (probed as catalyst first, else content URL) wins if set; a
    `worldConfiguration` scene demands an explicit server; a headless key
    demands an explicit target (key-signed deploys never pick a server
    implicitly); the browser flow walks the upstream mainnet catalyst
    snapshot and uses the first `/about`-healthy one.
  - worlds: without `--multi-scene`, existing scenes on other parcels get a
    `Continue? (y/N)` prompt (`--yes` skips; non-TTY/`--ci` refuses); the
    armed removal signs the upstream `delete:/entities/<world>:<ts>:{}`
    payload (second browser signature or the same key) and sends
    `DELETE <tc>/entities/<world>` with `x-identity-*` headers before
    uploading. `--multi-scene` is purely additive.
  - `--timestamp`/`--entity-out` make entity construction reproducible for
    oracle A/B (browser flow re-mints at signing time unless `--timestamp`
    pins it).
- `dcl-one-sdk world settings get|set NAME [--target-content URL]` and
  `dcl-one-sdk world permissions list|grant|revoke NAME [PERMISSION ADDRESS] [--target-content URL]` -
  worlds-server management with the ADR signed-fetch auth chain
  (`method:path:timestamp:metadata` lowercased, EIP-191 2-link), signed with
  `--sign-key`/`DCL_PRIVATE_KEY` (flag wins). `settings set` multipart fields: `--title
  --description --content-rating --spawn-coordinates --skybox-time
  --single-player --show-in-places --category (repeatable) --thumbnail
  <png>`. `permissions grant|revoke` maps to
  `PUT|DELETE /world/{name}/permissions/{deployment|streaming|access}/{address}`;
  `list`/`settings get` are unsigned reads.
- `dcl-one-sdk pack [--dir D] [--skip-build]` (alias `pack-smart-wearable`) -
  flat `smart-wearable.zip` of the publishable file set (same `.dclignore`
  semantics and glob-9 ordering as deploy; entries resolved against the
  project dir, fixing the upstream cwd defect). Validates wearable.json
  first (rarity/category schema enums, representations complete, referenced
  files present); the 2 MiB overrun is a warning (as upstream); plain scenes
  get an explicit "not a smart wearable" error, not upstream's silent exit 0.

## Build parity

`@dcl/sdk-commands` bundles via the esbuild JS API + two JS plugins;
dcl-one-sdk reproduces that pipeline with rolldown compiled into the binary
(`src/rolldown_backend.rs`, crates pinned `=1.2.0` - their Rust API is
internal surface, bump in lockstep), so no JS toolchain runs in the bundle
path. The three virtual inputs upstream feeds esbuild through plugins are
pre-generated as real files under `<project>/.dcl-one/`:

- `.dcl-one/entrypoint.ts` - port of `getEntrypointCode()` (incl. the
  literal `false` statement upstream emits for non-editor scenes), the
  `~sdk/all-composites` / `~sdk/script-utils` imports rewritten to relative
  paths; `is_editor_scene()` parses `assets/scene/main.composite` and
  requires an `asset-packs::` component other than the build-time-only
  `asset-packs::Script` (7.24.5 `isEditorScene`, upstream #1381; malformed
  or missing composite = not an editor scene)
- `.dcl-one/all-composites.js` - `export const compositeFromLoader = {...}`;
  since 7.24.5 only a ROOT-LEVEL `main.composite` is inlined (secondary
  composites stay on disk and lazy-load at runtime through the sdk composite
  provider via `~system/Runtime.readFile`; editor scenes keep their state in
  `main.crdt`), each through the Rust port of upstream's
  `Composite.fromJson` -> `Composite.toJson` normalization
  (`src/composite_norm.rs`, schema table `docs/composite-component-schemas.json`
  regenerated from the released `@dcl/ecs` 7.24.5; edge cases in
  `docs/composite-tojson-edge-cases.json`); byte-identical to upstream.
  Composite files over 16 MiB are refused (upstream cap), and the scan also
  yields `maxCompositeEntity` = max(`entityId & 0xffff`) across every
  parseable composite
- `.dcl-one/script-utils.js` - sdk-commands' `dist/logic/runtime-script.js`
  from the scene's node_modules, same CJS-strip transforms, incl.
  `prepareRuntimeCode`'s `@dcl/inspector/node_modules/@dcl/asset-packs` ->
  `@dcl/asset-packs` rewrite (without it the inlined runtime-script keeps an
  `--external:@dcl/inspector/*` require that throws at eval); wrapped with
  `_initializeScripts` (scripts array support not implemented)

Options mirrored onto `rolldown::BundlerOptions`: cjs/browser/es2020,
externals (`~system/*`, `@dcl/inspector*`; `*` globs compiled to anchored
regexes), aliases (react, `@dcl/sdk`, `@dcl/ecs`, `@dcl/asset-packs` -
upstream resolution order, which since 7.24.5 checks the scene's OWN
`node_modules/@dcl/asset-packs` by direct path, never a walk-up resolve;
`Project::node_module` has always behaved that way), define block
(document/window/DEBUG/NODE_ENV), minify in production, `sourcemap: inline`
dev / NONE in production (7.24.5 dropped prod maps; `*.map` also joined the
`.dclignore` defaults). Upstream's `DCL_MAX_COMPOSITE_ENTITY` esbuild define
is delivered differently in the split layout: the loader stub sets
`globalThis.DCL_MAX_COMPOSITE_ENTITY` before the sdk chunk evals - the
`typeof`-guarded reader in `@dcl/ecs` `createEntityContainer` sees the same
value, and the cached sdk-runtime chunk bytes stay independent of composite
content (watch rewrites the stub when the value changes).
`INVALID_ANNOTATION`/`IMPORT_IS_UNDEFINED` warnings are filtered
caller-side (rolldown core ignores `ChecksOptions` for emission). Type checking
shells the scene's own `node_modules/typescript/lib/tsc.js --noEmit` under
node (upstream's forked-tsc behavior).

## Start parity

The preview-server surface bevy-explorer consumes:

- `GET /about` - sdk-commands `setupRealmAndComms` shape
  (`localSceneParcels`, `bff.publicUrl = host`); `scenesUrn` embeds a
  `?=&baseUrl=http://<host>/content/contents/` modifier pinning content
  fetches to the preview server (bevy's `ipfs_path.rs::to_url` checks the
  embedded base BEFORE the realm about; without it `main.crdt` raced against
  the default catalyst and the scene never loaded)
- `GET /mini-comms/{roomId}` - RFC-5 ws-room relay (comms section)
- `GET|HEAD /content/contents/{b64-hash}` - `b64-` +
  base64(`<absPath>-<machineId>`) addressing (sdk-commands'
  `b64HashingFunction`), restricted to paths under the project root; the
  project-root hash returns the scene entity JSON (upstream `serveFolders`);
  responses carry `ETag` (= `hash_bytes_v1` content CID) +
  `Cache-Control: no-cache` and answer `If-None-Match` with 304 - the b64
  hashes are path-derived and never change on edit, so revalidation is what
  makes the sdk-runtime chunk cache; every request hits the access log
- `POST /content/entities/active`, `GET /content/entities/scene` -
  synthetic scene entity with b64 content hashes
- `GET /scenes` - `{"scenes":[],"total":0}`
- `GET /mobile-preview` - `{ok,data:{url,qr}}` with the mobile deep link and
  the QR as an `image/svg+xml` data URL (upstream ships PNG; both embed in
  `<img src>`); 404 `{ok:false,error:"No LAN IP address found"}` without a
  shareable interface
- `GET /` - WebSocket; each reload pushes TWO frames in guaranteed order:
  the legacy `{"type":"SCENE_UPDATE","payload":{"sceneId"}}` text frame
  first (the only frame bevy's `comms/src/preview.rs` parses), then a binary
  protobuf `WsSceneMessage` (`updateScene`; `updateModel` with
  upstream-bug-compatible `src`/`hash` fields for `.glb`/`.gltf` edits,
  which notify without a rebuild) for foundation explorers
- watcher: notify, 100 ms debounce, filters ts/tsx/js/jsx/composite; ignores
  `.dcl-one`/`bin`/`node_modules`/`.git` matched as path COMPONENTS (not
  string prefixes - `bindings.ts` or a `binary/` folder are
  still watched); drops inotify Access events (the rebuild's own reads
  otherwise self-trigger an infinite loop); composite changes regenerate
  `.dcl-one/all-composites.js` strictly before the rebuild
- workspaces (`dcl-workspace.json`, upstream `logic/workspace-validations.js`
  semantics; no file -> single-folder workspace on the fly): `/about` unions
  `localSceneParcels`, one
  `scenesUrn` per member; the entities endpoints answer over the union
  filtered by requested pointers (upstream `pointerRequestHandler`; no
  pointers -> all entities where upstream returns `[]` - deliberate, keeping
  the single-scene bevy flow byte-identical); `/content/contents/{b64}`
  resolves the owning member by longest-root match; per-member watch
  session, reload frames carry that member's sceneId so only that scene
  reloads. Single-scene projects take the exact pre-workspace code path.

## Creator Hub / visual editor (`start --data-layer`)

Node-bridge approach (a pure-Rust DataService was rejected):

- `start --data-layer` writes `.dcl-one/data-layer-host.mjs` (embedded
  template) and spawns it under node: the driver `createRequire`s the
  SCENE'S OWN `@dcl/inspector`, `@dcl/rpc` and `ws` (no version skew), boots
  `createDataLayerHost(fs)` over a ported sdk-commands fs adapter
  (cwd-sensitive; the driver chdirs to the project), and serves the
  22-procedure `DataService` on a loopback-only ephemeral port reported as
  `{"ready":true,"port":N}` on stdout. Restart with 1s->30s backoff; the
  driver exits when stdin closes so no node process outlives `start`
  (tokio's `Child::wait()` closes stdin - the supervisor holds the handle).
- `GET /data-layer` raw-proxies binary WS frames to the driver.
  `GET /inspector/` serves the scene's own
  `node_modules/@dcl/inspector/public` with the `$CONFIG` injection
  (`dataLayerRpcWsUrl` derived per-request from Host/X-Forwarded-*). Gotcha:
  only the `const config = '$CONFIG'` assignment may be rewritten - the file
  also compares `config !== '$CONFIG'` as a sentinel; replacing both makes
  the UI silently fall back to its in-memory fake data layer (unit-pinned).
- The join block prints `editor: http://<ip>:<port>/inspector/` rows; scenes
  without a vendored inspector can point `DCL_ONE_INSPECTOR_DIR` at an
  external `@dcl/inspector` package.
- Save loop: inspector save -> `assets/scene/main.composite` -> watcher ->
  regeneration -> incremental rebuild -> SCENE_UPDATE push -> hot reload.
- `main.crdt`: `build` and the watcher run the driver's one-shot `dump-crdt`
  mode, calling the scene's own sdk-commands `getAllComposites` with a real
  `writeFile` (inspector-API fallback when sdk-commands is missing); `build`
  skips when there are no composites, degrades to a warn when node is
  unavailable. Byte-parity vs sdk-commands 7.22.6: 3/3 layouts cmp-identical.
- Tests: `tests/data_layer_rpc.rs` (gated on
  `DCL_ONE_SDK_TEST_NODE_MODULES`) drives the @dcl/rpc handshake through the
  axum proxy.

## Comms: RFC-5 ws-room relay (`src/comms.rs`)

Rust port of `@dcl/mini-comms` (`dist/logic/handle-linear-protocol.js` +
`dist/adapters/rooms.js`), the relay sdk-commands runs in preview:

- `/about` advertises `comms.fixedAdapter = "ws-room:ws://<host>/mini-comms/room-1"`
  (host from the `Host` header; nginx `sub_filter` handles external
  rewriting). `--offline-comms` restores `offline:offline`; `--mini-comms`
  is accepted (hidden, ignored) for back-compat with existing unit files.
- `GET /mini-comms/{roomId}` upgrades with subprotocols `rfc5` OR `rfc4`
  echoed back (bevy sends `Sec-WebSocket-Protocol: rfc5`); frames are binary
  protobuf `WsPacket` (vendored
  `proto/decentraland/kernel/comms/rfc5/ws_comms.proto`, compiled by the
  same `build.rs` prost pass as the reload proto).
- Linear protocol and room semantics are upstream-exact: address regex
  `^0x[0-9a-fA-F]{40}$` (lowercased), `dcl-`-prefixed challenge (address map
  global across rooms), 1000 ms handshake timeouts, global monotonic alias
  counter, `from_alias` overwritten server-side on rebroadcast, unknown
  packet types tolerated, empty rooms dropped, second login with the same
  address kicks the old session (`WsKicked{"Already logged in"}`).
- Signature validation:
  `catalyrst_crypto::sign::verify_signed_message(chain, challenge, ident.address)`
  runs the upstream chain validation (personal-sign ECDSA links, ephemeral
  links incl. expiry, low-s enforcement) AND binds `chain[0]` (the owner) to
  the identified address. This deliberately EXCEEDS upstream, whose
  `Authenticator.validateSignature` registers the peer under the *claimed*
  address as long as ANY wallet validly signs the challenge - spoofable via
  an attacker-owned chain or a lone `[{SIGNER, payload:<challenge>}]` echo.
  Tests: `tests/comms_ws.rs` (two tokio-tungstenite clients through the bevy
  wire sequence). EIP-1654 (contract wallet) links are the one true
  deviation: rejected (`Eip1654NotImplemented`) - no ethereum RPC in preview.
- Not ported (metrics/config-only): the `WS_MAX_BUFFERED_AMOUNT`
  unreliable-drop heuristic (default 0 upstream; we always forward -
  unbounded per-peer queues) and the prometheus counters.

## Internet reach: `--tunnel` + catalyrst-preview-tunnel

`catalyrst-preview-tunnel` (sibling crate in this workspace) is a small
self-hostable tunnel service:

```
# on a public host
catalyrst-preview-tunnel        # PUBLIC_BASE_URL=https://<tunnel-host>, optional TUNNEL_TOKEN

# on the creator machine
dcl-one-sdk start --tunnel wss://<tunnel-host> [--tunnel-token <token> | --tunnel-token-file <path>]
```

Token sources, highest wins: `--tunnel-token`, then `--tunnel-token-file`,
then the `DCL_ONE_SDK_TUNNEL_TOKEN` env var (file/env keep the token out of
argv and `ps` output).

The agent (`src/tunnel.rs`) dials out over one websocket trunk, is assigned
a `/t/<id>` prefix, and the CLI prints an INTERNET join block (public realm
+ web/desktop/native/mobile rows) alongside the local one. HTTP + websocket
traffic (incl. mini-comms and live-reload) is multiplexed over the trunk;
`/about` and the root redirect honor `X-Forwarded-Proto`/`Host`/`Prefix`, so
the realm descriptor is correct behind the tunnel or any reverse proxy.
Reconnects: 1s->30s jittered backoff, SAME public URL resumed across blips
(keyed resume); an unreachable tunnel warns ONCE with a clean UserError
while serving continues locally. `--tunnel help` prints the zero-infra
`ssh -R`/`ssh -L` fallback recipes. In-engine bevy through the tunnel was
not driven (protocol-level proof only).

## Error contract

Every user-reachable failure names its cause and at least one `-> try:`
next step; raw error chains (`caused by:`, os errors) render only under
`--verbose`. `tests/error_contract.rs` enforces the contract (G1-G32),
including: the tunnel-unreachable warn renders the concise first cause
(`g24`); tsc gets `--pretty false` on non-TTY stderr and the error count
survives; broken workspace members name the `dcl-workspace.json` folders
entry instead of misdirecting to `--dir`, with the absolute path trimmed to
satisfy gate g20.

## Deploy

Upstream-exact pipeline: sdk-commands' `.dclignore` defaults + user lines
with CASE-INSENSITIVE gitignore semantics (npm `ignore` 5.3.2 defaults
`ignoreCase: true`, so `Readme.MD` is excluded by `*.md`; our matcher sets
`GitignoreBuilder::case_insensitive(true)`), the `getPublishableFiles` file
set in glob-9 order, `hash_bytes_v1` CIDv1 hashing via catalyrst-hashing, v3
entity JSON without `id` byte-identical to dcl-catalyst-client's
(`src/jsjson.rs` reproduces JS `JSON.stringify` number/string formatting),
EIP-191 simple 2-link auth chain, multipart POST `/entities`. Entity-id
parity is pinned against the sdk-commands oracle. World deploys with an
explicit `--target-content` pre-check deployment permission for the signing
address before anything is deleted or uploaded (7.24.5 semantics: owner,
`unrestricted`, wallet with world-wide grant, else the per-parcel allowlist);
denial is a hard error naming the denied parcels, while an unreachable
permissions endpoint only warns and continues. Planned: query the server
for existing hashes and upload only the delta.

## Split bundle layout

Every build emits three files (the split layout is the only build mode):

- `bin/index.js` - loader stub (`src/templates/split-loader.js`): sets
  `globalThis.DCL_MAX_COMPOSITE_ENTITY` (upstream's esbuild define, see
  Build parity), reads both chunks via `~system/Runtime.readFile`, evals
  them, wires a `require` shim (`~system/*` passthrough, registry lookup,
  loud error otherwise), delegates `onStart`/`onUpdate`; TextDecoder
  feature-detect with a chunked ASCII fallback
- `bin/sdk-runtime.js` - lazy+memoized registry of the SDK modules (24
  static keys + conditional `@dcl/asset-packs{,/dist/scene-entrypoint}` and
  `react/jsx-runtime`); byte-identical across scene-code edits (same
  ETag/CID), so it caches indefinitely
- `bin/scene.js` - the scene entrypoint with the SDK externalized
  (composites stay scene-side, filling the sdk chunk's slot via
  `Object.assign`)

Production sizes on the freshly scaffolded template scene: scene.js 998 B,
loader stub 3,702 B, sdk-runtime.js 463,288 B - the per-scene marginal cost
is the ~1 KB scene chunk, vs upstream's single ~938 KB production bundle.

Watch/start wiring: the scene chunk re-bundles on every edit; the
sdk-runtime chunk re-bundles only when the registry key list changes
(`split::registry_keys` re-checked per batch; node_modules is unwatched, so
in practice once per session), with `SCENE_UPDATE` pushed per scene-chunk
rebuild. Composite edits regenerate `all-composites.js` (and `main.crdt`)
strictly before the scene rebuild.

## Security posture (`start` preview server)

The preview server binds `0.0.0.0` (LAN reach is the point) with
`CorsLayer::permissive()`. Hardened:

- `/data-layer` (WRITE-capable inspector RPC) enforces an Origin allowlist
  on the WS upgrade: same-origin (`Host` / `X-Forwarded-Host`-aware, so
  nginx fronting keeps working), native clients (no Origin / `null`), and
  origins in `DCL_ONE_SDK_ALLOWED_ORIGINS` (comma-separated) are accepted;
  other cross-origin pages (incl. DNS-rebind) are rejected `403`.
- `/content/contents/{hash}` only serves files the entity listing includes
  (what `.dclignore`/`collect_publishable_files` publish); `.env`, `.git`,
  `package.json` and other ignored files under the scene root `404`.
- `/inspector/{*path}` canonicalizes and confines reads to `public_dir`
  (absolute-path and `..`/`..\` escapes `404`).
- The linker signing server binds loopback (`127.0.0.1`) by default so LAN
  hosts cannot scrape `/api/info` or race a bogus `/api/sign`; set
  `DCL_ONE_SDK_LINKER_HOST=0.0.0.0` to sign from another device.

Known gaps (deferred; do not break the proven bevy/nginx preview): the `/`
live-reload WS and `/mini-comms/*` upgrades enforce no Origin check
(mini-comms already requires a wallet-signed challenge bound to the
identified address; the reload socket is read-only push);
`CorsLayer::permissive()` stays - a full cross-origin lockdown risks the
decentraland.org-origin bevy web client. mini-comms keeps unbounded
rooms/peers and per-peer queues, and the alias counter is a `u32` (wraps
after 4.2B joins) - DoS/wrap surface only, acceptable for a local preview
relay.
