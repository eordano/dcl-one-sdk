#!/usr/bin/env node
// golden-runtime.mjs — the RUNTIME tier of the golden scene-snapshot harness.
//
// Usage: node scripts/golden-runtime.mjs <scene-root>
//
// Loads a built scene's `main` (the split loader stub) in a CommonJS sandbox
// whose only door to the outside is an injected `require` over a fixed
// `~system/*` mock table, runs upstream's short frame loop, and prints the
// block that `tests/golden.rs` appends to the golden file.
//
// Two different failures, two different mechanisms:
//
//   * An error that escapes a phase, or an unhandled rejection anywhere, exits
//     non-zero — a scene that silently died on frame 2.
//   * An error the SCENE swallows never reaches this process: the SDK's
//     generated startup wraps main() in `try { … } catch (e) { console.error(e.stack) }`
//     and a caught throw changes no CRDT traffic, so exit status cannot see it.
//     Console output is therefore recorded INTO the golden as `CONSOLE(<level>):`
//     lines under the phase that emitted them. A scene that starts throwing
//     changes its golden and the test fails as stale.
//
// Plain .mjs rather than the .mts the other harnesses use, because this one is
// spawned by `cargo test` on every run and node's TypeScript stripping is a
// version-dependent flag; there is nothing here that needs types.

import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'

const root = path.resolve(process.argv[2] ?? '.')
const sceneJson = JSON.parse(fs.readFileSync(path.join(root, 'scene.json'), 'utf8'))
const mainFile = sceneJson.main

// Decoding with the scene's copy rather than one of our own is what makes the
// `data=null` lines meaningful: a component id the installed @dcl/ecs does not
// know (asset-packs writes several) renders as null exactly the way upstream's
// goldens show it, instead of being silently dropped or guessed at.
const sceneRequire = createRequire(path.join(root, 'golden-runtime-anchor.js'))
const { ReadWriteByteBuffer } = sceneRequire('@dcl/ecs/dist-cjs/serialization/ByteBuffer')
const { readMessage } = sceneRequire('@dcl/ecs/dist-cjs/serialization/crdt/message')
const { CrdtMessageType } = sceneRequire('@dcl/ecs/dist-cjs/serialization/crdt/types')
const hostEngine = sceneRequire('@dcl/ecs/dist-cjs').engine

/** Upstream's serializer, character for character. */
function serializeMessage(prefix, msg) {
  const type = CrdtMessageType[msg.type] ?? `UNKNOWN(${msg.type})`
  const head = `  ${prefix}: ${type} e=0x${msg.entityId.toString(16)}`
  if (msg.componentId === undefined) {
    return head
  }
  let data = 'null'
  if (msg.data !== undefined) {
    const component = hostEngine.getComponentOrNull(msg.componentId)
    if (component) {
      data = JSON.stringify(component.schema.deserialize(new ReadWriteByteBuffer(msg.data)))
    }
  }
  return `${head} c=${msg.componentId} t=${msg.timestamp} data=${data}`
}

/** Decode one wire buffer, in wire order — never sorted. */
function decodeBuffer(prefix, bytes) {
  const buffer = new ReadWriteByteBuffer(bytes)
  const lines = []
  let msg
  while ((msg = readMessage(buffer))) {
    lines.push(serializeMessage(prefix, msg))
    traffic.messages += 1
    traffic.bytes += msg.length
  }
  return lines
}

const hostCalls = { readFile: 0, crdtGetState: 0, crdtSendToRenderer: 0, sendBatch: 0 }
const traffic = { messages: 0, bytes: 0 }
const readFiles = []
const requiredHostModules = []
const consoleLines = []
const rejections = []
/** Lines emitted by the phase currently running. */
let phase = []

process.on('unhandledRejection', (err) => {
  rejections.push(String(err && err.stack ? err.stack : err))
})

/** A `    at …` stack frame, the only place a path or an eval offset appears. */
const STACK_FRAME = /^\s+at\s/

// Stack FRAMES are dropped before a console line reaches the golden: they carry
// absolute paths and `new Function` eval offsets, neither of which may ever sit
// in a checked-in snapshot. The message line survives, and that is the line
// that names the failure ("Error: golden fixture boom").
function consoleText(args) {
  return args
    .map((a) => (a instanceof Error && a.stack ? String(a.stack) : String(a)))
    .join(' ')
    .split('\n')
    .filter((line) => line.trim() !== '' && !STACK_FRAME.test(line))
    .map((line) => line.split(root).join('<SCENE>').trimEnd())
}

const sandboxConsole = {}
for (const level of ['log', 'info', 'warn', 'error', 'debug', 'trace']) {
  sandboxConsole[level] = (...args) => {
    for (const line of consoleText(args)) {
      consoleLines.push(`[${level}] ${line}`)
      phase.push(`  CONSOLE(${level}): ${line}`)
    }
  }
}

// Every entry is the narrowest shape the SDK actually reads. An unknown module
// id THROWS: that is what turns the REQUIRE lines below from a log into an
// assertion about the host-API surface a scene depends on, so a new host
// module reaching the runtime fails the suite instead of quietly working.
const HOST_MODULES = {
  '~system/Runtime': () => ({
    readFile: async ({ fileName }) => {
      hostCalls.readFile += 1
      readFiles.push(fileName)
      return { content: new Uint8Array(fs.readFileSync(path.join(root, fileName))), hash: 'golden' }
    },
    getRealm: async () => ({
      realmInfo: {
        baseUrl: 'http://127.0.0.1:8000',
        realmName: 'golden',
        networkId: 0,
        commsAdapter: 'offline',
        isPreview: true,
      },
    }),
    getWorldTime: async () => ({ seconds: 0 }),
    getSceneInformation: async () => ({
      urn: 'urn:golden',
      content: [],
      metadataJson: JSON.stringify(sceneJson),
      baseUrl: 'http://127.0.0.1:8000/content/contents/',
    }),
    // Missing from scripts/split-harness.mts, which is why that harness dies
    // with "(0 , p.getExplorerInformation) is not a function" inside
    // @dcl/sdk/platform for any react-ecs or smart-item scene.
    getExplorerInformation: async () => ({
      agent: 'golden',
      platform: 'desktop',
      configurations: {},
    }),
  }),
  '~system/EngineApi': () => ({
    crdtSendToRenderer: async ({ data }) => {
      hostCalls.crdtSendToRenderer += 1
      for (const buffer of toBuffers(data)) {
        phase.push(...decodeBuffer('Scene', buffer))
      }
      return { data: [] }
    },
    // Feeding the scene's own main.crdt back in is upstream's
    // `<bundle>.js-main.crdt` trick, except the file is the one this build just
    // produced rather than a hand-placed sidecar.
    crdtGetState: async () => {
      hostCalls.crdtGetState += 1
      const state = mainCrdtBytes()
      if (state) {
        phase.push(...decodeBuffer('main.crdt', state))
        return { data: [state], hasEntities: true }
      }
      return { data: [], hasEntities: false }
    },
    sendBatch: async () => {
      hostCalls.sendBatch += 1
      return { events: [] }
    },
    subscribe: async () => ({}),
    unsubscribe: async () => ({}),
  }),
  '~system/CommunicationsController': () => ({
    send: async () => ({ data: [] }),
    sendBinary: async () => ({ data: [] }),
  }),
  '~system/CommsApi': () => ({
    getActiveVideoStreams: async () => ({ streams: [] }),
    subscribeToTopic: async () => ({}),
    unsubscribeFromTopic: async () => ({}),
    publishData: async () => ({}),
    consumeMessages: async () => ({ messages: [] }),
  }),
  '~system/EthereumController': () => ({
    requirePayment: async () => ({ jsonAnyResponse: '{}' }),
    signMessage: async () => ({ message: '', hexEncodedMessage: '', signature: '' }),
    convertMessageToObject: async () => ({ dict: [] }),
    sendAsync: async () => ({ jsonAnyResponse: '{}' }),
    getUserAccount: async () => ({ address: '0x0000000000000000000000000000000000000000' }),
  }),
  '~system/UserIdentity': () => ({
    getUserData: async () => ({
      data: {
        userId: '0xgolden',
        displayName: 'golden',
        hasConnectedWeb3: false,
        version: 1,
        avatar: {
          bodyShape: 'urn:decentraland:off-chain:base-avatars:BaseMale',
          wearables: [],
          emotes: [],
          snapshots: { face256: '', body: '' },
          eyeColor: '#000000',
          hairColor: '#000000',
          skinColor: '#000000',
        },
      },
    }),
    getUserPublicKey: async () => ({ address: '0xgolden' }),
  }),
  '~system/Players': () => ({
    getConnectedPlayers: async () => ({ players: [] }),
    getPlayersInScene: async () => ({ players: [] }),
    getPlayerData: async () => ({}),
  }),
  '~system/RestrictedActions': () => ({
    movePlayerTo: async () => ({}),
    teleportTo: async () => ({}),
    triggerEmote: async () => ({}),
    changeRealm: async () => ({ success: true }),
    openExternalUrl: async () => ({ success: true }),
    openNftDialog: async () => ({}),
    setCommunicationsAdapter: async () => ({ success: true }),
    triggerSceneEmote: async () => ({}),
  }),
  '~system/SignedFetch': () => ({
    signedFetch: async () => ({ ok: true, status: 200, statusText: 'OK', headers: {}, body: '' }),
    getHeaders: async () => ({ headers: {} }),
  }),
  '~system/Testing': () => ({
    logTestResult: async () => ({}),
    plan: async () => ({}),
    setCameraTransform: async () => ({}),
  }),
}

/** main.crdt as bytes, or undefined when the fixture has no composite. */
let cachedMainCrdt
function mainCrdtBytes() {
  if (cachedMainCrdt === undefined) {
    const p = path.join(root, 'main.crdt')
    cachedMainCrdt = fs.existsSync(p) ? new Uint8Array(fs.readFileSync(p)) : null
  }
  return cachedMainCrdt
}

/** crdtSendToRenderer takes one buffer; crdtGetState-shaped payloads take many. */
function toBuffers(data) {
  if (!data) return []
  return ArrayBuffer.isView(data) ? [data] : data
}

function hostRequire(spec) {
  if (typeof spec !== 'string' || !spec.startsWith('~system/')) {
    throw new Error(`the scene requested a non-host module: ${spec}`)
  }
  const factory = HOST_MODULES[spec]
  if (!factory) {
    throw new Error(
      `${spec} is not in the golden harness host-module table; add it to scripts/golden-runtime.mjs`
    )
  }
  if (!requiredHostModules.includes(spec)) {
    requiredHostModules.push(spec)
  }
  return factory()
}

const fakeGlobal = { require: hostRequire, console: sandboxConsole }

// `TextDecoder = undefined` is not laziness: it forces the loader's
// String.fromCharCode fallback, which is the path the real QuickJS host takes,
// so the golden covers the branch scenes actually run.
const PREAMBLE = [
  'const require = globalThis.require;',
  'const console = globalThis.console;',
  'const TextDecoder = undefined;',
  '',
].join('\n')

function loadCjs(rel) {
  const code = fs.readFileSync(path.join(root, rel), 'utf8')
  const mod = { exports: {} }
  const wrapper = new Function('globalThis', 'module', 'exports', PREAMBLE + code)
  wrapper.call(fakeGlobal, fakeGlobal, mod, mod.exports)
  return mod.exports
}

function fail(what, err) {
  process.stderr.write(`GOLDEN RUNTIME FAIL: ${what}\n`)
  if (err) process.stderr.write(`${err && err.stack ? err.stack : err}\n`)
  if (consoleLines.length) {
    process.stderr.write(`scene console:\n  ${consoleLines.join('\n  ')}\n`)
  }
  process.exit(1)
}

const out = []
/** Run one phase, collecting whatever CRDT lines it emits under its header. */
async function runPhase(header, body) {
  phase = []
  try {
    await body()
  } catch (e) {
    fail(header, e)
  }
  out.push(header, ...phase)
}

let loader
try {
  loader = loadCjs(mainFile)
} catch (e) {
  fail(`eval ${mainFile}`, e)
}
// Whatever module evaluation logged, before runPhase resets the buffer.
const evalConsole = phase
if (typeof loader.onStart !== 'function') fail(`${mainFile} exports no onStart`)
if (typeof loader.onUpdate !== 'function') fail(`${mainFile} exports no onUpdate`)

// The EVAL section carries the whole run's file reads and host-module requests
// rather than only what module evaluation itself touched: with the split loader
// the chunks are fetched and evaluated inside onStart, so listing them here is
// what keeps "bringing the scene up" in one block, as upstream's goldens read.
const evalSection = out.length
out.push(`EVAL ${mainFile}`)

await runPhase('CALL onStart()', () => loader.onStart())
for (const dt of [0.0, 0.1, 0.1, 0.1]) {
  await runPhase(`CALL onUpdate(${dt})`, () => loader.onUpdate(dt))
}

// Drain anything the scene queued in a microtask before reading the counters,
// so a rejection raised by the last frame is reported rather than swallowed by
// process exit.
await new Promise((resolve) => setImmediate(resolve))
if (rejections.length) fail(`unhandled rejection\n${rejections.join('\n')}`)

out.splice(
  evalSection + 1,
  0,
  ...readFiles.map((f) => `  READFILE: ${f}`),
  ...requiredHostModules.map((m) => `  REQUIRE: ${m}`),
  ...evalConsole
)
out.push(
  `HOSTCALLS readFile=${hostCalls.readFile} crdtGetState=${hostCalls.crdtGetState}` +
    ` crdtSendToRenderer=${hostCalls.crdtSendToRenderer} sendBatch=${hostCalls.sendBatch}`,
  `CRDT_TRAFFIC messages=${traffic.messages} bytes=${traffic.bytes}`
)
process.stdout.write(out.join('\n') + '\n')
