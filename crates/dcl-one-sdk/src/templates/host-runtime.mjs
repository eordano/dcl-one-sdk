// dcl-one-sdk host runtime -- the authoritative-server isolate (design doc
// M1). Runs a built scene under node with isServer() == true, bridged to the
// preview's mini-comms room through the JSON host door. Pure node built-ins:
// the only modules loaded are the scene's own bundle and chunks.
//
//   node host-runtime.mjs <sceneRoot> <doorWsUrl>
//
// The ~system table is the golden harness's grown live: readFile serves the
// real files (and points the sdk chunk's '@dcl/sdk/server' at host-storage.mjs
// -- see PATCH below), EngineApi answers isServer with true and feeds the
// scene a RealmInfo whose isConnectedSceneRoom follows the door,
// CommunicationsController bridges the room, and Storage/EnvVar ride the
// preview's storage routes. The sync transport, registerMessages and getRoom
// are upstream's own (@dcl/sdk auth-server line): room messages are
// binaryMessageBus frames, and every inbound frame is stamped
// [senderLen][sender][payload] with the relay-verified sender, exactly as the
// engine stamps them for a client.
'use strict'

import fs from 'node:fs'
import path from 'node:path'
import { createServerModule } from './host-storage.mjs'

const root = path.resolve(process.argv[2] ?? '.')
const doorUrl = process.argv[3]
if (!doorUrl) {
  console.error('usage: host-runtime.mjs <sceneRoot> <doorWsUrl>')
  process.exit(2)
}
const sceneJson = JSON.parse(fs.readFileSync(path.join(root, 'scene.json'), 'utf8'))
const mainFile = sceneJson.main
// Mark harness lifecycle events for Rust to render with the SDK's timestamp
// and continuation gutter. Scene console output remains untouched.
const log = (...a) => console.log('DCL_ONE_MULTIPLAYER:status:' + a.join(' '))
const tech = (...a) => console.log('DCL_ONE_MULTIPLAYER:detail:' + a.join(' '))
// one headline no matter which side of the boot/welcome race prints first
let announced = false
const announce = () => {
  if (announced) return
  announced = true
  log('host joined the room')
}

// ---------------------------------------------------------------------------
// rfc4 comms envelope: explorer clients wrap every scene binary message as
// Packet{ scene: Scene{ scene_id, data }, protocol_version } before it hits
// the ws-room, so the host speaks the same. Hand-rolled varint codec for the
// three fields involved (rfc4/comms.proto: Packet.scene = 6,
// Scene.scene_id = 1, Scene.data = 2, protocol_version = 11 -- explorers
// stamp version 100).
// ---------------------------------------------------------------------------
function varint(n) {
  const out = []
  while (n > 127) {
    out.push((n & 127) | 128)
    n >>>= 7
  }
  out.push(n)
  return Buffer.from(out)
}
function lenDelim(tag, payload) {
  return Buffer.concat([Buffer.from([(tag << 3) | 2]), varint(payload.length), payload])
}
function rfc4Wrap(sceneId, data) {
  const scene = Buffer.concat([
    lenDelim(1, Buffer.from(sceneId, 'utf8')),
    lenDelim(2, Buffer.from(data))
  ])
  return Buffer.concat([lenDelim(6, scene), Buffer.from([(11 << 3) | 0, 100])])
}
function readVarint(buf, at) {
  let n = 0
  let shift = 0
  while (at < buf.length) {
    const b = buf[at++]
    n |= (b & 127) << shift
    if ((b & 128) === 0) return [n >>> 0, at]
    shift += 7
  }
  return [n >>> 0, at]
}
/* the scene message inside an rfc4 packet, or null for anything else
   (positions, profiles, chat -- the room carries them all) */
function rfc4Unwrap(buf) {
  let at = 0
  while (at < buf.length) {
    const tag = buf[at]
    const field = tag >> 3
    const wire = tag & 7
    at += 1
    if (wire === 0) {
      ;[, at] = readVarint(buf, at)
    } else if (wire === 2) {
      let len
      ;[len, at] = readVarint(buf, at)
      const body = buf.subarray(at, at + len)
      at += len
      if (field === 6) {
        // Scene { scene_id = 1, data = 2 }
        let sAt = 0
        let sceneId = ''
        let data = null
        while (sAt < body.length) {
          const sTag = body[sAt]
          const sField = sTag >> 3
          const sWire = sTag & 7
          sAt += 1
          if (sWire === 2) {
            let sLen
            ;[sLen, sAt] = readVarint(body, sAt)
            const sBody = body.subarray(sAt, sAt + sLen)
            sAt += sLen
            if (sField === 1) sceneId = sBody.toString('utf8')
            else if (sField === 2) data = sBody
          } else if (sWire === 0) {
            ;[, sAt] = readVarint(body, sAt)
          } else return null
        }
        return data ? { sceneId, data } : null
      }
    } else if (wire === 5) at += 4
    else if (wire === 1) at += 8
    else return null
  }
  return null
}

// the id explorers stamp on this scene's messages: fetched from the preview,
// and adopted from the first inbound scene packet either way
let sceneId = ''
async function learnSceneId() {
  try {
    const base = doorUrl.replace(/^ws/, 'http').replace(/\/mini-comms\/.*$/, '')
    const about = await (await fetch(base + '/about')).json()
    const urn = (about.configurations?.scenesUrn ?? [])[0]
    if (urn) {
      // urn:decentraland:entity:<id>?=&baseUrl=... -- the entity id is what
      // explorers stamp as Scene.scene_id (adoption from the first client
      // packet corrects this if the guess is ever wrong)
      sceneId = String(urn).replace(/^urn:decentraland:entity:/, '').split('?')[0]
      if (welcomed) tech('scene: ' + sceneId + ' (updated)')
    }
  } catch {}
}
learnSceneId()

// ---------------------------------------------------------------------------
// room door: JSON websocket, relay-verified senders
// ---------------------------------------------------------------------------
let ws = null
let welcomed = false
let conceded = false
// alias -> address, from welcome/join/leave frames
const peers = new Map()
// inbound scene binary, sender-stamped, drained by sendBinary
let inboundSync = []
let reconnectDelay = 1000

// the engine's inbound framing (bevy crates/dcl/src/js/comms.rs): one byte
// of sender length, the sender, then the scene bytes. The sdk's
// binaryMessageBus decodes exactly this.
function stampSender(from, body) {
  const sender = Buffer.from(from, 'utf8')
  if (sender.length > 255) return null
  return new Uint8Array(Buffer.concat([Buffer.from([sender.length]), sender, body]))
}

function connect() {
  ws = new WebSocket(doorUrl)
  ws.addEventListener('open', () => {
    reconnectDelay = 1000
    // the welcome line carries the details; a bare connect is noise
  })
  ws.addEventListener('message', (ev) => {
    let frame
    try {
      frame = JSON.parse(String(ev.data))
    } catch {
      return
    }
    if (frame.type === 'welcome') {
      const rejoin = welcomed
      welcomed = true
      // a fresh welcome is a fresh room (the preview restarts freely and
      // drops it); stale peers from the previous connection must not linger
      peers.clear()
      for (const [alias, address] of Object.entries(frame.peers ?? {}))
        peers.set(Number(alias), String(address).toLowerCase())
      if (rejoin) log('rejoined the multiplayer room')
      else announce()
      tech('scene: ' + (sceneId || '(pending)') + ' (alias ' + frame.alias + ')')
      tech('room: ' + doorUrl + (peers.size ? ' (' + peers.size + ' player(s) here)' : ''))
      setRealmConnected(true)
      onPresence()
    } else if (frame.type === 'join') {
      peers.set(Number(frame.alias), String(frame.address).toLowerCase())
      log('player joined:', frame.address)
      onPresence()
    } else if (frame.type === 'leave') {
      peers.delete(Number(frame.alias))
      if (frame.address) log('player left:', frame.address)
      onPresence()
    } else if (frame.type === 'update') {
      const raw = Buffer.from(String(frame.body), 'base64')
      const from = String(frame.fromAddress ?? '').toLowerCase()
      // explorer peers wrap scene traffic in rfc4; fake/test peers may speak
      // raw scene bytes -- accept both, ignore non-scene comms (positions,
      // profiles, chat)
      const unwrapped = rfc4Unwrap(raw)
      let body
      if (unwrapped) {
        if (!sceneId && unwrapped.sceneId) {
          sceneId = unwrapped.sceneId
          tech('scene: ' + sceneId + ' (from client)')
        }
        if (sceneId && unwrapped.sceneId && unwrapped.sceneId !== sceneId) return
        body = unwrapped.data
      } else if (raw.length) {
        body = raw
      } else {
        return
      }
      const stamped = from && stampSender(from, body)
      if (stamped) inboundSync.push(stamped)
    } else if (frame.type === 'kicked') {
      // another host took the slot: concede instead of reconnecting, or two
      // hosts ping-pong kicking each other forever
      log('another server took over this room -- exiting:', frame.reason)
      conceded = true
      process.exit(0)
    }
  })
  const retry = () => {
    if (conceded) return
    welcomed = false
    setRealmConnected(false)
    setTimeout(connect, reconnectDelay)
    reconnectDelay = Math.min(reconnectDelay * 2, 15000)
  }
  ws.addEventListener('close', retry)
  ws.addEventListener('error', () => {
    try {
      ws.close()
    } catch {}
  })
}
connect()

function doorSend(bytes, to) {
  if (!ws || ws.readyState !== 1) return
  // wrap for explorer peers when the scene id is known; raw until then
  // (before the first client, only test peers can be listening anyway)
  const wire = sceneId ? rfc4Wrap(sceneId, bytes) : Buffer.from(bytes)
  const frame = { type: 'update', body: wire.toString('base64') }
  if (to && to.length) frame.to = to
  ws.send(JSON.stringify(frame))
}

// storage: the preview server owns the file and serves upstream's routes;
// this side is upstream's client semantics (host-storage.mjs), bound to the
// preview the door URL names (its path prefix kept, the door suffix dropped)
const previewBase = doorUrl.replace(/^ws/, 'http').replace(/\/mini-comms\/[^/]+\/host$/, '')
const { Storage, EnvVar } = createServerModule({ baseUrl: previewBase })

// presence -> the sdk players lib, best effort: once the chunks are loaded the
// registry hook exposes the bundle's engine, and joins/leaves become
// PlayerIdentityData entities the lib's onEnterScene watches
let presenceDirty = false
function onPresence() {
  presenceDirty = true
}
const presenceEntities = new Map()

// RealmInfo -> the sdk sync transport. Its room-ready logic hangs off
// RealmInfo.onChange(RootEntity).isConnectedSceneRoom, and ecs onChange fires
// only for CRDT arriving over a transport, never for local writes, so the
// host plays renderer: the next crdtSendToRenderer answers with a
// PUT_COMPONENT for the root entity (reserved, which the renderer transport
// alone may touch). Wire layout, @dcl/ecs PutComponentOperation.write:
// u32 LE length, type=1, entity, componentId, timestamp, dataLength, data.
let realmDirty = true
let realmConnected = false
let realmTimestamp = 0
const realmPending = []
function setRealmConnected(v) {
  if (realmConnected !== v) realmDirty = true
  realmConnected = v
}
function pushRealmInfo() {
  const reg = fakeGlobal.__dclOneHostRegistry && fakeGlobal.__dclOneHostRegistry()
  if (!reg) return
  let engine, RealmInfo, ReadWriteByteBuffer
  try {
    const ecs = reg['@dcl/sdk/ecs']
    engine = ecs.engine
    RealmInfo = ecs.RealmInfo || ecs.components.RealmInfo(engine)
    ReadWriteByteBuffer = reg['@dcl/ecs/dist/serialization/ByteBuffer'].ReadWriteByteBuffer
  } catch {
    return
  }
  if (!engine || !RealmInfo || !RealmInfo.schema || !ReadWriteByteBuffer) return
  realmDirty = false
  const value = {
    baseUrl: previewBase,
    realmName: 'dcl-one-host',
    networkId: 0,
    commsAdapter: 'host',
    isPreview: true,
    isConnectedSceneRoom: realmConnected
  }
  const data = new ReadWriteByteBuffer()
  RealmInfo.schema.serialize(value, data)
  const body = data.toBinary()
  const msg = Buffer.alloc(24 + body.length)
  msg.writeUInt32LE(msg.length, 0)
  msg.writeUInt32LE(1, 4) // CrdtMessageType.PUT_COMPONENT
  msg.writeUInt32LE(engine.RootEntity, 8)
  msg.writeUInt32LE(RealmInfo.componentId, 12)
  msg.writeUInt32LE(++realmTimestamp, 16)
  msg.writeUInt32LE(body.length, 20)
  msg.set(body, 24)
  realmPending.push(new Uint8Array(msg))
}

function reconcilePresence() {
  presenceDirty = false
  const reg = fakeGlobal.__dclOneHostRegistry && fakeGlobal.__dclOneHostRegistry()
  if (!reg) return
  let engine, PlayerIdentityData
  try {
    const ecs = reg['@dcl/sdk/ecs']
    engine = ecs.engine
    PlayerIdentityData = ecs.PlayerIdentityData || ecs.components.PlayerIdentityData(engine)
  } catch {
    return
  }
  if (!engine || !PlayerIdentityData || typeof PlayerIdentityData.create !== 'function') return
  const want = new Set(peers.values())
  for (const [addr, ent] of presenceEntities)
    if (!want.has(addr)) {
      engine.removeEntity(ent)
      presenceEntities.delete(addr)
    }
  for (const addr of want)
    if (!presenceEntities.has(addr)) {
      const ent = engine.addEntity()
      PlayerIdentityData.create(ent, { address: addr, isGuest: false })
      presenceEntities.set(addr, ent)
    }
}

// ---------------------------------------------------------------------------
// the ~system table (live where multiplayer needs it, golden-stub elsewhere)
// ---------------------------------------------------------------------------
const HOST_ADDRESS = '0x0000000000000000000000000000000000000000'

// PATCH: the sdk chunk's module.exports IS the split-loader registry. The
// suffix appended here runs inside the chunk wrapper, so it can rebind one
// key before the scene chunk evaluates: '@dcl/sdk/server' becomes
// host-storage.mjs (upstream's module resolves its storage URL from the
// realm; ours is bound to the preview's routes and tested against them),
// and the registry itself is exposed for the presence and RealmInfo
// bridges. This lives only in the host harness -- client bundles are
// untouched and get upstream's module, which throws off-server.
const SDK_CHUNK_SUFFIX = `
;(function () {
  var __reg = module.exports
  Object.defineProperty(__reg, '@dcl/sdk/server', {
    configurable: true,
    get: function () {
      return globalThis.__dclOneHostServerModule
    }
  })
  globalThis.__dclOneHostRegistry = function () { return __reg }
})();
`
// NOTE: the chunks' `globalThis` is the sandbox global (the loader wrapper
// shadows the real one), so the hooks must live there -- see fakeGlobal.

function isSdkChunk(fileName) {
  return /sdk-runtime.*\.js$/.test(fileName)
}

const HOST_MODULES = {
  '~system/Runtime': () => ({
    readFile: async ({ fileName }) => {
      let content = fs.readFileSync(path.join(root, fileName))
      if (isSdkChunk(fileName)) content = Buffer.concat([content, Buffer.from(SDK_CHUNK_SUFFIX)])
      else if (fileName.endsWith('.js')) {
        // a dynamic import('@dcl/sdk/server') survives bundling verbatim and
        // node would resolve it on the FILESYSTEM, past the registry -- serve
        // the chunk with the call rewritten to the grafted module instead
        const src = content.toString('utf8')
        const patched = src.replace(
          /import\(\s*["']@dcl\/sdk\/server["']\s*\)/g,
          'Promise.resolve(globalThis.__dclOneHostServerModule)'
        )
        if (patched !== src) content = Buffer.from(patched)
      }
      return { content: new Uint8Array(content), hash: 'host' }
    },
    getRealm: async () => ({
      realmInfo: {
        baseUrl: previewBase,
        realmName: 'dcl-one-host',
        networkId: 0,
        commsAdapter: 'host',
        isPreview: true
      }
    }),
    getWorldTime: async () => ({ seconds: Date.now() / 1000 }),
    getSceneInformation: async () => ({
      urn: 'urn:dcl-one-host',
      content: [],
      metadataJson: JSON.stringify(sceneJson),
      baseUrl: 'host://'
    }),
    getExplorerInformation: async () => ({
      agent: 'dcl-one-host',
      // the sdk platform helper accepts only mobile|desktop|web and logs an
      // error for anything else; the agent string carries the server identity
      platform: 'desktop',
      configurations: {}
    })
  }),
  '~system/EngineApi': () => ({
    // this isolate IS the authoritative server
    isServer: async () => ({ isServer: true }),
    // no renderer behind this isolate: the CRDT the scene emits for one is
    // dropped, what comes back is the RealmInfo feed (pushRealmInfo), and the
    // state it asks for is the composite the build produced
    crdtSendToRenderer: async () => ({ data: realmPending.splice(0) }),
    crdtGetState: async () => {
      const p = path.join(root, 'main.crdt')
      if (fs.existsSync(p)) return { data: [new Uint8Array(fs.readFileSync(p))], hasEntities: true }
      return { data: [], hasEntities: false }
    },
    sendBatch: async () => ({ events: [] }),
    subscribe: async () => ({}),
    unsubscribe: async () => ({})
  }),
  '~system/CommunicationsController': () => ({
    send: async () => ({ data: [] }),
    sendBinary: async (body) => {
      for (const bytes of body.data ?? []) doorSend(bytes)
      for (const pm of body.peerData ?? [])
        for (const bytes of pm.data ?? [])
          doorSend(bytes, pm.address && pm.address.length ? pm.address : undefined)
      const drained = inboundSync
      inboundSync = []
      return { data: drained }
    }
  }),
  '~system/CommsApi': () => ({
    getActiveVideoStreams: async () => ({ streams: [] })
  }),
  '~system/UserIdentity': () => ({
    getUserData: async () => ({
      data: {
        userId: HOST_ADDRESS,
        displayName: 'server',
        hasConnectedWeb3: true,
        version: 1,
        avatar: undefined
      }
    }),
    getUserPublicKey: async () => ({ address: HOST_ADDRESS })
  }),
  '~system/Players': () => ({
    getConnectedPlayers: async () => ({
      players: [...peers.values()].map((userId) => ({ userId }))
    }),
    getPlayersInScene: async () => ({
      players: [...peers.values()].map((userId) => ({ userId }))
    }),
    getPlayerData: async ({ userId }) => ({
      data: { userId, displayName: userId, hasConnectedWeb3: true, version: 1 }
    })
  }),
  '~system/RestrictedActions': () => ({
    movePlayerTo: async () => ({}),
    triggerEmote: async () => ({}),
    openExternalUrl: async () => ({ success: false })
  }),
  '~system/EthereumController': () => ({
    getUserAccount: async () => ({ address: HOST_ADDRESS })
  }),
  '~system/SignedFetch': () => ({
    // the host is trusted infrastructure; a plain fetch stands in until the
    // door hands out an identity chain to sign with
    signedFetch: async ({ url, init }) => {
      const r = await fetch(url, init ?? {})
      return {
        ok: r.ok,
        status: r.status,
        statusText: r.statusText,
        headers: {},
        body: await r.text()
      }
    },
    getHeaders: async () => ({ headers: {} })
  }),
  '~system/Testing': () => ({
    logTestResult: async () => ({}),
    plan: async () => ({})
  })
}

function hostRequire(spec) {
  if (typeof spec !== 'string' || !spec.startsWith('~system/')) {
    throw new Error('the scene requested a non-host module: ' + spec)
  }
  const factory = HOST_MODULES[spec]
  if (!factory) throw new Error(spec + ' is not in the host-module table; add it to host-runtime.mjs')
  return factory()
}

// ---------------------------------------------------------------------------
// sandbox + the forever frame loop
// ---------------------------------------------------------------------------
const fakeGlobal = {
  require: hostRequire,
  console,
  __dclOneHostServerModule: { Storage, EnvVar }
}
const PREAMBLE = 'const require = globalThis.require;\n'
function loadCjs(rel) {
  const code = fs.readFileSync(path.join(root, rel), 'utf8')
  const mod = { exports: {} }
  const wrapper = new Function('globalThis', 'module', 'exports', PREAMBLE + code)
  wrapper.call(fakeGlobal, fakeGlobal, mod, mod.exports)
  return mod.exports
}

process.on('unhandledRejection', (e) => console.error('[multiplayer] unhandled rejection:', e))

// the parent CLI may exit without running Drop impls (its ctrl-c path is a
// hard exit), so child-reaping cannot be its job: the CLI holds our stdin
// pipe, and any death mode closes it -- exit with it instead of squatting
// the room slot as an orphan (same pattern as data-layer-host.mjs)
process.stdin.resume()
process.stdin.on('end', () => process.exit(0))
process.stdin.on('close', () => process.exit(0))

const loader = loadCjs(mainFile)
if (typeof loader.onStart !== 'function' || typeof loader.onUpdate !== 'function') {
  console.error('[multiplayer] the scene main exports no onStart/onUpdate -- not a built scene?')
  process.exit(1)
}

const TICK_MS = 33
let last = Date.now()
try {
  await loader.onStart()
  announce()
} catch (e) {
  console.error('[multiplayer] onStart threw:', e)
  process.exit(1)
}
setInterval(async () => {
  const now = Date.now()
  const dt = (now - last) / 1000
  last = now
  if (presenceDirty) reconcilePresence()
  if (realmDirty) pushRealmInfo()
  try {
    await loader.onUpdate(dt)
  } catch (e) {
    console.error('[multiplayer] onUpdate threw:', e)
  }
}, TICK_MS)
