// dcl-one-sdk host runtime -- the authoritative-server isolate (design doc
// M1). Runs a built scene under node with isServer() == true, bridged to the
// preview's mini-comms room through the JSON host door. Pure node built-ins:
// the only modules loaded are the scene's own bundle and chunks.
//
//   node host-runtime.mjs <sceneRoot> <doorWsUrl> <storagePath>
//
// The ~system table is the golden harness's grown live: readFile serves the
// real files (and grafts the auth-server API surface onto the sdk chunk it
// serves -- see PATCH below), CommunicationsController bridges the room, and
// Storage/EnvVar land on disk. Room messages ride a "DCLR" magic-prefixed
// JSON envelope so they never collide with the sync transport's CRDT bytes;
// the relay stamps every inbound frame with the handshake-verified sender.
'use strict'

import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(process.argv[2] ?? '.')
const doorUrl = process.argv[3]
const storagePath = path.resolve(process.argv[4] ?? path.join(root, '.dcl-one', 'storage.json'))
if (!doorUrl) {
  console.error('usage: host-runtime.mjs <sceneRoot> <doorWsUrl> <storagePath>')
  process.exit(2)
}
const sceneJson = JSON.parse(fs.readFileSync(path.join(root, 'scene.json'), 'utf8'))
const mainFile = sceneJson.main
const log = (...a) => console.log('[multiplayer]', ...a)
// indented detail under a headline, the way a person reads a log
const tech = (...a) => console.log('  -> technical info:', ...a)
// one headline no matter which side of the boot/welcome race prints first
let announced = false
const announce = () => {
  if (announced) return
  announced = true
  log('scene started with multiplayer support')
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
      if (welcomed) tech('scene id is', sceneId)
    }
  } catch {}
}
learnSceneId()

// ---------------------------------------------------------------------------
// room door: JSON websocket, relay-verified senders
// ---------------------------------------------------------------------------
const ROOM_MAGIC = Buffer.from('DCLR')
let ws = null
let welcomed = false
let conceded = false
// alias -> address, from welcome/join/leave frames
const peers = new Map()
// inbound non-room binary (sync transport bytes), drained by sendBinary
let inboundSync = []
// inbound room envelopes, dispatched to registered handlers
const roomHandlers = new Map()
let reconnectDelay = 1000

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
      tech(
        'scene id is ' + (sceneId || '(pending)') + ' (alias ' + frame.alias + ') at ' + doorUrl +
          (peers.size ? ', ' + peers.size + ' player(s) already here' : '')
      )
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
          tech('scene id corrected from a client packet: ' + sceneId)
        }
        if (sceneId && unwrapped.sceneId && unwrapped.sceneId !== sceneId) return
        body = unwrapped.data
      } else if (raw.subarray(0, 4).equals(ROOM_MAGIC)) {
        body = raw
      } else {
        return
      }
      if (body.subarray(0, 4).equals(ROOM_MAGIC)) {
        let msg
        try {
          msg = JSON.parse(body.subarray(4).toString('utf8'))
        } catch {
          return
        }
        const handler = roomHandlers.get(msg.t)
        if (handler) {
          try {
            handler(msg.p, { from })
          } catch (e) {
            console.error('[multiplayer] onMessage handler threw:', e)
          }
        }
      } else {
        inboundSync.push(new Uint8Array(body))
      }
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

// ---------------------------------------------------------------------------
// auth-server API surface, grafted onto the sdk chunk (see PATCH)
// ---------------------------------------------------------------------------
let roomSingleton = null
function hostRegisterMessages(_schemas) {
  // schema-validated parity is a doc'd TODO; the envelope is JSON either way
  if (roomSingleton) return roomSingleton
  roomSingleton = {
    send(type, payload, opts) {
      const body = Buffer.concat([
        ROOM_MAGIC,
        Buffer.from(JSON.stringify({ t: type, p: payload }), 'utf8')
      ])
      doorSend(body, opts && opts.to ? opts.to.map((a) => String(a).toLowerCase()) : undefined)
    },
    onMessage(type, cb) {
      roomHandlers.set(type, cb)
    }
  }
  return roomSingleton
}

// storage: one JSON file, world + per-player namespaces, debounced writes
let store = { world: {}, player: {} }
try {
  store = JSON.parse(fs.readFileSync(storagePath, 'utf8'))
  store.world ??= {}
  store.player ??= {}
} catch {}
let storeTimer = null
function persistStore() {
  if (storeTimer) return
  storeTimer = setTimeout(() => {
    storeTimer = null
    try {
      fs.mkdirSync(path.dirname(storagePath), { recursive: true })
      const tmp = storagePath + '.tmp'
      fs.writeFileSync(tmp, JSON.stringify(store))
      fs.renameSync(tmp, storagePath)
    } catch (e) {
      console.error('[multiplayer] storage write failed:', e)
    }
  }, 500)
}
const Storage = {
  async get(key) {
    return key in store.world ? store.world[key] : null
  },
  async set(key, value) {
    store.world[key] = String(value)
    persistStore()
    return true
  },
  player: {
    async get(address, key) {
      const p = store.player[String(address).toLowerCase()]
      return p && key in p ? p[key] : null
    },
    async set(address, key, value) {
      const a = String(address).toLowerCase()
      store.player[a] ??= {}
      store.player[a][key] = String(value)
      persistStore()
      return true
    }
  }
}
const EnvVar = {
  async get(name) {
    if (name in process.env) return process.env[name]
    try {
      for (const line of fs.readFileSync(path.join(root, '.env'), 'utf8').split('\n')) {
        const eq = line.indexOf('=')
        if (eq > 0 && line.slice(0, eq).trim() === name) return line.slice(eq + 1).trim()
      }
    } catch {}
    return null
  }
}

// presence -> the sdk players lib, best effort: once the chunks are loaded the
// registry hook exposes the bundle's engine, and joins/leaves become
// PlayerIdentityData entities the lib's onEnterScene watches
let presenceDirty = false
function onPresence() {
  presenceDirty = true
}
const presenceEntities = new Map()
function reconcilePresence() {
  presenceDirty = false
  const reg = fakeGlobal.__dclOneHostRegistry && fakeGlobal.__dclOneHostRegistry()
  if (!reg) return
  let engine, PlayerIdentityData
  try {
    engine = reg['@dcl/sdk/ecs'].engine
    PlayerIdentityData = reg['@dcl/sdk/ecs'].components.PlayerIdentityData
  } catch {
    return
  }
  if (!engine || !PlayerIdentityData) return
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
// suffix appended here runs inside the chunk wrapper, so it can wrap the
// registry before the scene chunk evaluates: the network module namespace
// gains the auth-server functions, '@dcl/sdk/server' resolves at all, and
// the registry itself is exposed for the presence bridge. This lives only in
// the host harness -- client bundles are untouched.
const SDK_CHUNK_SUFFIX = `
;(function () {
  var __reg = module.exports
  var __netDesc = Object.getOwnPropertyDescriptor(__reg, '@dcl/sdk/network')
  if (__netDesc) {
    Object.defineProperty(__reg, '@dcl/sdk/network', {
      configurable: true,
      get: function () {
        var m = __netDesc.get ? __netDesc.get() : __netDesc.value
        if (globalThis.__dclOneHostPatchNetwork) m = globalThis.__dclOneHostPatchNetwork(m)
        return m
      }
    })
  }
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
        baseUrl: new URL(doorUrl.replace(/^ws/, 'http')).origin,
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
    // no renderer behind this isolate: the CRDT the scene emits for one is
    // dropped, the state it asks for is the composite the build produced
    crdtSendToRenderer: async () => ({ data: [] }),
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
  __dclOneHostServerModule: { Storage, EnvVar },
  __dclOneHostPatchNetwork: (m) => {
    // the 7.26 namespace exports getter-only properties (isStateSyncronized
    // among them), so grafting happens on a facade whose prototype is the
    // real module: additions shadow, everything else reads through
    const facade = Object.create(m)
    if (typeof m.registerMessages !== 'function')
      Object.defineProperty(facade, 'registerMessages', { value: hostRegisterMessages })
    Object.defineProperty(facade, 'isServer', { value: () => true })
    if (typeof m.isStateSyncronized !== 'function')
      Object.defineProperty(facade, 'isStateSyncronized', { value: () => welcomed })
    return facade
  }
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
  try {
    await loader.onUpdate(dt)
  } catch (e) {
    console.error('[multiplayer] onUpdate threw:', e)
  }
}, TICK_MS)
