# Multiplayer server support in dcl-one-sdk -- design sketch

Decided 2026-08-30 (user interview, hexabricks session): dcl-one-sdk should
grow the `@dcl/sdk@auth-server` surface so scenes written against the
official multiplayer server APIs run under this toolchain -- today they
feature-detect and fall back to single-player. The concrete consumer is
`hexabricks/src/persistence.ts`, which exercises every API below and is the
acceptance test for this work.

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

Two halves, mirroring how the rest of this crate splits toolchain from
runtime:

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
(`src/comms.rs`); the host rides the same listener so `start` +
multiplayer is one process and one port. `dcl-one-sdk start --host` runs
both roles for local testing; a bare `host` serves headless.

## Milestones

- M1: room messages end to end -- registerMessages/send/onMessage with
  verified senders; hexabricks lay/break/notice work, no persistence.
- M2: syncEntity LWW + late-join snapshot -- BrickData/Builders/LayLog
  mirror; the builders panel goes live.
- M3: Storage -- snapshots survive restarts; visit stamps work.
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
- Augmentation point: scene chunks treat `@dcl/sdk/*` as externals wired by
  the split loader, and the generated entrypoint already injects
  before-scene modules (`sdk-boot.js` precedent). The MP runtime is one
  more injected module that patches the loaded network namespace and
  registers a synthetic `@dcl/sdk/server` in the loader table. It must
  activate only when a host is present, or every preview flips scenes into
  MP mode with nobody serving.
- The host-side scene sandbox exists: `scripts/golden-runtime.mjs` runs
  scene bundles under node behind a `~system/*` mock table. The host
  harness is that table with real implementations (live frame loop,
  CommunicationsController bridged to the room, `isServer() == true`,
  Storage on disk).

**Landed:** the room's host side-door, `GET /mini-comms/{room}/host`
(`src/comms.rs`). The host joins as a real peer through a JSON websocket --
loopback-gated, occupying the zero-address slot no wallet can mint -- and
the relay transcodes protobuf<->JSON both ways, stamping every inbound
update with the sender address the signed handshake verified. Targeted
sends (`to: [addresses]`) come for free for `room.send(..., { to })`.

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

Remaining: in-world verification with a real explorer client (the one
step a headless harness cannot take), M3 restart-survival exercises, and
the hardening list below. Client-side inbound sender identity is not
verifiable (the platform hands scenes bytes, not senders) -- state
authority lives server-side where the relay stamps addresses.

## Non-goals for now

- Scaling past one room per scene process.
- The upstream hosting service's deployment story; this host is for the
  self-hosted/local-realm lane.
- Server-side physics or any authority beyond what scene code implements.
