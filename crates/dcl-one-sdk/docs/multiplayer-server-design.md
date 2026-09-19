# Multiplayer server support in dcl-one-sdk -- design sketch

Decided 2026-08-30 (user interview, hexabricks session): dcl-one-sdk should grow
the `@dcl/sdk@auth-server` surface so scenes written against the official
multiplayer server APIs run under this toolchain -- today they feature-detect
and fall back to single-player. The concrete consumer is
`hexabricks/src/persistence.ts`, which exercises every API below and is the
acceptance test.

## Surface a scene needs

From `@dcl/sdk/network` (module augmentation; scenes feature-detect):

- `registerMessages(schemas) -> room` with `room.send(type, payload,
  { to })` and `room.onMessage(type, cb)` where `ctx.from` is a VERIFIED
  wallet address, never client-claimed.
- `isServer()`, `isStateSyncronized()`.
- `syncEntity(entity, componentIds, enumId?)` -- LWW component replication
  to every client, late joiners included.
- `validateBeforeChange` on synced components (server-only writes).

From `@dcl/sdk/server` (dynamic import; absence keeps scenes portable):

- `Storage.get<T>(key)` / `Storage.set(key, value)` -- durable string KV.

Plus `players.onEnterScene/onLeaveScene/getPlayer` answering for the
server's view of the room.

## Shape

Two halves, mirroring how the rest of this crate splits toolchain from runtime:

1. **Vendored SDK half.** The prebuilt sdk-runtime chunk gains the
   `network` + `server` entry points, compiled from the same pinned
   upstream line as the rest of the vendored toolchain. Type stripping
   keeps `persistence.ts`-style feature detection working unchanged.

2. **Host half: `dcl-one-sdk host`.** Runs the scene bundle headless under
   node with `isServer() == true`, owning:
   - a websocket room on a registered port; clients join through the same
     signed-identity handshake the comms island already validates, so
     `ctx.from` costs nothing new to trust;
   - message relay with per-type schema validation (reject, not crash, on
     malformed frames);
   - LWW component sync: server-written components fan out to clients;
     client CRDT writes to server-validated components are dropped at the
     door (`validateBeforeChange` server-only is the degenerate case);
   - `Storage` backed by a JSON-per-key directory under the scene's
     `.dcl-one/storage/` (the play-lane shape: no daemon, inspectable,
     trivially backed up).

The preview server already runs a comms websocket island per scene
(`src/comms.rs`); the host rides the same listener, so `start` + multiplayer is
one process and one port. `dcl-one-sdk start --host` runs both roles for local
testing; a bare `host` serves headless.

## Milestones

- M1: room messages end to end -- registerMessages/send/onMessage with
  verified senders; hexabricks lay/break/notice work, no persistence.
- M2: syncEntity LWW + late-join snapshot -- BrickData/Builders/LayLog
  mirror; the builders panel goes live.
- M3: Storage -- snapshots survive restarts; visit stamps work. (landed 2026-09-15, below)
- M4: `start --host` integration + reconnect behaviour; document in the
  README beside the preview section.

## Findings + first landing (2026-08-30, later)

Exploration collapsed the estimate considerably:

- The vendored 7.26 chunk ALREADY ships `@dcl/sdk/network` with
  `syncEntity`; only the auth-server additions are missing
  (`registerMessages`, `isServer`, `isStateSyncronized`, `@dcl/sdk/server`).
- Scene-level messaging needs no new client transport: it rides
  `~system/CommunicationsController.sendBinary`, which the explorer relays
  through the preview's existing mini-comms room (signed-challenge
  handshake, verified addresses).
- Augmentation point: scene chunks treat `@dcl/sdk/*` as externals wired by the
  split loader, and the generated entrypoint already injects before-scene
  modules (`sdk-boot.js` precedent). The MP runtime is one more injected module
  that patches the loaded network namespace and registers a synthetic
  `@dcl/sdk/server` in the loader table. It must activate only when a host is
  present, or every preview flips scenes into MP mode with nobody serving.
- The host-side scene sandbox exists: `scripts/golden-runtime.mjs` runs
  scene bundles under node behind a `~system/*` mock table. The host
  harness is that table with real implementations (live frame loop,
  CommunicationsController bridged to the room, `isServer() == true`,
  Storage on disk).

**Landed:** the room's host side-door, `GET /mini-comms/{room}/host`
(`src/comms.rs`). The host joins as a real peer through a JSON websocket --
loopback-gated, occupying the zero-address slot no wallet can mint -- and the
relay transcodes protobuf<->JSON both ways, stamping every inbound update with
the sender address the signed handshake verified. Targeted sends
(`to: [addresses]`) come for free for `room.send(..., { to })`.

**Next in M1:** the host harness (`host-runtime.mjs` from the golden
table), the injected mp-runtime module + loader entry, the host-presence
activation signal, and a smoke test driving hexabricks lay/notice through
a headless host and one fake client.

## Upstream parity (official authoritative-servers docs, read 2026-08-31)

The platform documentation pins several things this design must match:

- **Activation** is `"authoritativeMultiplayer": true` in scene.json -- not
  a CLI flag or deep-link param. `start`/`host` should key off exactly
  that, so a scene ports between toolchains without edits. (`hexabricks`
  now carries the flag; it is inert until the host exists.)
- **Local dev parity:** upstream's preview AUTO-STARTS the server role
  beside the client preview, with storage in a single JSON file under the
  toolchain's runtime dir. `start` should do the same when the scene.json
  flag is set -- `--host` as an override, not a requirement.
- **API surface** beyond what this doc already lists:
  `registerMessages(...)` RETURNS the room (call once at module load);
  `Storage.player.get/set(address, key, value)` is a per-player namespace
  beside the world-level `Storage.get/set`; `EnvVar.get(name)` reads
  server-side env; `validateBeforeChange(entity, cb)` is PER-ENTITY and
  the callback sees `senderAddress` with an `AUTH_SERVER_PEER_ID`
  constant marking server writes (the host's zero-address peer should be
  surfaced through that constant, not leaked as an address).
- **Limits to respect in the harness:** 256 MB isolate, 10 s synchronous
  turn, 60 s async settle, ~300 messages/s/peer, ~13 KB per message
  (silently dropped above), 128 KB inbound packet, ~30 KB scene-to-comms,
  40 in-flight host calls, 32 concurrent signedFetch (15 s timeout,
  10 MB cap). Synced components ride the comms path, so anything
  log-shaped must stay under the 30 KB packet ceiling (hexabricks caps
  its lay log accordingly).
- **Verified positions:** the server reads `PlayerIdentityData` +
  `Transform` as server-verified state -- the harness must surface player
  entities, not just message senders.
- **Ops surface** (later): `sdk-commands storage scene|player|env` CLI
  equivalents, and log access gated by scene.json `logsPermissions`.

## Landed (2026-08-31): M1, M2 and M4

The loop is closed end to end, verified headlessly:

- **Host isolate** (`dcl-one-sdk host`, M1): the golden runtime grown live
  under node -- auth-server surface grafted onto the served sdk chunk,
  Storage/EnvVar on disk, DCLR room envelopes with relay-verified senders,
  stdin lifeline (dies with its parent, SIGKILL-proven), kicked hosts
  concede, reconnects survive preview restarts with a cleared peer map.
- **rfc4 interop** (M2, host side): a hand-rolled varint codec wraps and
  unwraps the explorer's Packet.scene envelope (field numbers from the
  rfc4 comms.proto, protocol_version 100), the scene id learned from the
  preview's /about scenesUrn and adopted from the first client packet.
  Non-scene comms are ignored; raw test-peer frames still pass.
- **Client shim** (M2, client side): with scene.json's flag the split
  loader arms a CommunicationsController wrap (DCLR envelopes folded out
  of the sync stream into an inbox; outbound rides the transport's next
  flush) and the entrypoint imports the generated mp-client.js before the
  scene: registerMessages/isServer land as NEW keys on the network
  namespace, so feature detection finds the room exactly as upstream's.
  Without the flag, zero bundle bytes change.
- **Auto-host** (M4): `start` attaches the isolate when the scene carries
  the flag, --no-host opts out, spawn failure degrades rather than kills,
  and the lifeline ties the isolate to the preview.
- The acceptance scene pauses building with a notice when the host goes
  silent (heartbeat grace covers joining) instead of dropping lays.

## Revised (2026-09-15): upstream's Room replaces the DCLR stand-in

The vendored `@dcl/sdk` moved from mainline 7.27.0 to the `auth-server`
line (`7.29.1-34986384248.commit-bb45080`), which ships the surface the
stand-in imitated: `isServer()`, `registerMessages`/`getRoom` (a `Room`
over `binaryMessageBus`, CUSTOM_EVENT frames with schema-encoded
payloads), `@dcl/sdk/server` (`Storage`/`EnvVar`, off-server calls throw
"only available on server-side scenes") and `@dcl/ecs`'s
`AUTHORITATIVE_PUT_COMPONENT` + `CreatedBy`. Both ends now run the same
chunk, so the JSON envelope, `mp-client.js`, the loader's inbox/outbox
and the host's grafted network facade are gone. What remains is glue:

- **Host** (`host-runtime.mjs`): answers `EngineApi.isServer` with true,
  stamps every inbound door frame `[senderLen][sender][payload]` (the
  engine's own framing, which the sdk's bus decodes), overrides the
  chunk's `@dcl/sdk/server` key with `host-storage.mjs`, and feeds the
  scene a `RealmInfo` PUT through `crdtSendToRenderer`'s answer whose
  `isConnectedSceneRoom` follows the door -- the sync transport's
  room-ready state hangs off `RealmInfo.onChange`, and ecs `onChange`
  fires only for CRDT arriving over a transport.
- **Loader** (`split-loader.js`, flag-armed): the sdk's client trusts CRDT,
  state responses and room events only from the sender
  `authoritative-server`; the engine names peers by hex address and the
  preview host is mini-comms' zero address, so the wrap re-labels frames
  from that address. Production comms already present the scene-state
  server under that name, and no real peer owns the zero address.
- **Blob overlay** (`patch_sdk_peer_trust`, `src/vendor/README.md`): the
  same sender check is gated on `globalThis.__dclOneAuthoritative`, set
  by the loader from the flag, so a scene without a server keeps
  mainline's peer trust and serverless-multiplayer scenes keep syncing.

## Landed (2026-09-15): M3 storage

Storage moved from a JSON file the isolate owned to a service the preview
runs, which is what upstream's hosted server sees:

- **One authority.** `src/storage.rs` keeps each scene's values in
  `.dcl-one/storage.sqlite` (table `kv(scope, address, key, value,
  updated_at, source)` plus `settings` and a 200-row `activity` log; WAL,
  5 s busy timeout, a connection per request). A legacy `storage.json` is
  imported once on first open.
- **The production wire shape.** The preview serves `/values[/{key}]`,
  `/players[/{address}/values[/{key}]]`, `/env[/{key}]` and `/usage/*` as
  `world-storage-service` does (audited 2026-09-15 against its handlers,
  OpenAPI and integration tests): `{ error, message }` error bodies, `Value
  not found` 404s, `Invalid player address` for anything but `0x` + 40 hex,
  `Key must be between 1 and 255 characters`, per-scope value and total
  ceilings in its words, `limit` clamped to 1–100 (default 100) and echoed
  in `pagination`, `GET /players` and `GET /env` as arrays of names, and the
  `X-Confirm-Delete-All` guard (`src/start/storage_page.rs`). `Page::to_json`
  emits only `key`/`value`, so a scene never sees the bookkeeping columns.
  Local extras ride on separate routes (`/storage/*`) or headers
  (`x-dcl-one-storage-source`), never on the service's shapes.
- **Upstream's client.** The isolate's `@dcl/sdk/server` is
  `templates/host-storage.mjs`, a port of the auth-server branch's client:
  512-entry/60 s read cache with negative entries, coalesced GETs, per-key
  write queue capped at two requests, `skipIfUnchanged`, `fresh`, list pages
  that seed the cache, `getValues` queries built from truthy options only,
  a 404 that is an outcome rather than an error line (upstream's
  `fix/storage-404-not-an-error`), async methods that reject off-server, and
  boolean results that never throw. `scripts/host-storage.test.mjs` pins it.
- **A service behind the switch.** The page's "Use upstream storage" and
  `storage target` point the same routes at `storage.decentraland.org`,
  `.zone` or a custom URL; `src/storage_remote.rs` forwards with ADR-44
  signed-fetch headers (simple chain from `DCL_PRIVATE_KEY`, or the Deploy
  page's delegated identity) and `{ realm, realmName, parcel }` metadata.
  Reads and writes bound for a service are gated to this machine.
- **Ops surface.** `dcl-one-sdk storage scene|player|env get|set|delete|
  list|clear`, `target`, `export`, `import` (`src/storage_cli.rs`) cover
  upstream's `sdk-commands storage` (text values unless `--json`, `player
  clear` for everyone, `localhost` as local development, the `World:` /
  `Genesis City scene at parcel` line, its ignored dApp flags) and the local
  extras. The one intentional divergence: the default target is the
  project's remembered one, not `storage.decentraland.org`.

Remaining: in-world verification with a real explorer client (the one step a
headless harness cannot take), signing a remote CLI request with a browser
wallet (today: a private key, or the preview's proxy with the Deploy page's
session), and the hardening list below. Client-side inbound sender identity is not verifiable (the platform
hands scenes bytes, not senders) -- state authority lives server-side where the
relay stamps addresses.

The headless half of that verification is `tests/host_room.rs`: a flagged
scene built the way `start` builds it, the real host isolate joined through
an in-process mini-comms door, and a fake explorer peer that must receive
the host's unicast `RES_CRDT_STATE` for its `REQ_CRDT_STATE` and a `pong`
custom event whose `from` is the peer's own address, proving the sender
stamp, the `to` list and upstream's `Room` end to end over the wire.

## Landed (2026-09-18): the mode works with default flags

Voice (0.25.0) moved a preview's comms onto the embedded livekit-server while
the host kept joining the ws-room, so a flagged scene started with no flags had
its clients in a LiveKit scene room and its server in mini-comms; the release
notes asked for a manual `--no-livekit`. `start` now decides `hosts_scene`
(flag set, no `--no-host`) before comms: such a preview stays on the ws-room
and says `voice off for this preview`, and an explicit `--livekit-url` beside
a host is called out instead of silently splitting the room
(`tests/livekit_embedded.rs::a_scene_with_a_server_keeps_comms_on_the_ws_room`).
`--no-server`, upstream's spelling (auth-server `e2bbcc9a`), is a visible alias
of `--no-host`. The README gained the section M4 asked for. `tests/host_room.rs`
had stopped compiling when `BuildOptions` grew `built_in`; it builds and passes
again. Still open: a LiveKit transport for the host, which would give flagged
scenes voice back.

## Landed (2026-09-18): the flag is a switch, and the preview follows it

The `/scene` page's layout card has a **Multiplayer** tab: one switch
(`POST /scene-json` `{ "authoritativeMultiplayer": bool }`, the editors'
allowlist and gates; off removes the key) and a status line for the four
states (`off`, `running`, `skipped` under `--no-host`, `down`). What made a
switch worth having is that the flag no longer needs a restart, from the page
or from a hand edit:

- **Server.** The isolate moved from a local in `start` to `AppState.host`
  (`src/start/host_slot.rs`); `follow_host` runs at startup and after every
  scene rebuild
  (`notify_reload`), attaching, dropping, or swapping the isolate for a fresh
  one. The swap also fixes a gap the mode always had: the isolate runs the
  bundle it loaded, so server code used to go stale on the first save.
- **Client.** The loader stub bakes the flag in (`__dclOneMp`), and the
  built-in watch session staged it once; `follow_mp_flag` re-reads it on a
  scene.json batch and rewrites the stub before the rebuild.
- **Comms.** `/about` and `/get-scene-adapter` read `AppState::voice()`, which
  is `None` while the scene has a server, so a preview that started with a
  livekit-server sends clients to the ws-room as soon as the flag appears and
  back to LiveKit when it goes. Clients already connected keep their room
  until they rejoin; the terminal and the page's toast say so.

`tests/host_toggle.rs` drives the cycle against a real `start`: scaffold, on,
host in the room with the loader armed and `/scene` reporting `running`, off,
key gone with the host detached and the loader disarmed.

## Non-goals for now

- Scaling past one room per scene process.
- The upstream hosting service's deployment story; this host is for the
  self-hosted/local-realm lane.
- Server-side physics or any authority beyond what scene code implements.
