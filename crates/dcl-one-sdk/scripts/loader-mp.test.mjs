// The split loader's authoritative-multiplayer arming
// (src/templates/split-loader.js): the trust flag the sdk chunk's sync
// transport reads, and the re-labelling of the preview host's frames as
// 'authoritative-server' -- only with scene.json's flag, and only for the
// zero address mini-comms gives the host.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'
const template = readFileSync(
  process.env.LOADER_TEMPLATE || new URL('../src/templates/split-loader.js', import.meta.url),
  'utf8'
)
function sandbox(mp) {
  const context = vm.createContext({ module: { exports: {} }, TextDecoder })
  const source = template
    .replaceAll('__DCL_ONE_SDK_CHUNK__', 'sdk.js')
    .replaceAll('__DCL_ONE_SCENE_CHUNK__', 'scene.js')
    .replaceAll('__DCL_ONE_SMART_CHUNK__', '')
    .replaceAll('__DCL_ONE_MAX_COMPOSITE_ENTITY__', '0')
    .replaceAll('__DCL_ONE_MP__', mp ? 'true' : 'false')
  vm.runInContext(source, context)
  return context
}
const HOST = '0x0000000000000000000000000000000000000000'
const PEER = '0x1111111111111111111111111111111111111111'
function frame(sender, payload) {
  const s = Buffer.from(sender, 'utf8')
  return new Uint8Array(Buffer.concat([Buffer.from([s.length]), s, Buffer.from(payload)]))
}
function sender(frame) {
  return Buffer.from(frame.subarray(1, 1 + frame[0])).toString('utf8')
}
function payload(frame) {
  return Buffer.from(frame.subarray(1 + frame[0]))
}
function comms(inbound) {
  const calls = []
  return {
    calls,
    require: (spec) => {
      assert.equal(spec, '~system/CommunicationsController')
      return {
        send: async (b) => ({ sent: b }),
        sendBinary: async (b) => {
          calls.push(b)
          return { data: inbound }
        }
      }
    }
  }
}

test('without the flag the chunk sees no authority and the require is untouched', () => {
  const c = sandbox(false)
  assert.equal(c.__dclOneAuthoritative, false)
  const real = () => 'real'
  assert.equal(c.__dclOneMpWrap(real), real)
})

test('with the flag the chunk trusts only the authority', () => {
  assert.equal(sandbox(true).__dclOneAuthoritative, true)
})

test('the wrap re-labels the zero address and nothing else', async () => {
  const c = sandbox(true)
  const inbound = [
    frame(HOST, [7, 1, 2, 3]),
    frame(PEER, [7, 4]),
    frame(HOST.replace('0x', '0X'), [9]),
    frame('authoritative-server', [8]),
    new Uint8Array([42, 48]), // truncated: shorter than its own sender length
    new Uint8Array([])
  ]
  const host = comms(inbound)
  const wrapped = c.__dclOneMpWrap(host.require)('~system/CommunicationsController')
  const body = { data: [new Uint8Array([1])], peerData: [{ address: [PEER], data: [new Uint8Array([2])] }] }
  const res = await wrapped.sendBinary(body)
  assert.equal(host.calls.length, 1)
  assert.equal(host.calls[0], body, 'the outbound body passes through by identity')
  assert.equal(res.data.length, inbound.length)
  assert.equal(sender(res.data[0]), 'authoritative-server')
  assert.deepEqual([...payload(res.data[0])], [7, 1, 2, 3])
  assert.equal(sender(res.data[1]), PEER)
  assert.deepEqual([...payload(res.data[1])], [7, 4])
  assert.equal(sender(res.data[2]), 'authoritative-server', 'case-insensitive 0x')
  assert.deepEqual([...payload(res.data[2])], [9])
  assert.equal(res.data[3], inbound[3], 'a frame already from the authority is untouched')
  assert.equal(res.data[4], inbound[4])
  assert.equal(res.data[5], inbound[5])
  const again = c.__dclOneMpWrap(host.require)
  assert.equal(again('~system/CommunicationsController'), again('~system/CommunicationsController'))
  assert.equal((await wrapped.send({ x: 1 })).sent.x, 1)
})

test('a zero address that is not all zeros is a real peer', () => {
  const c = sandbox(true)
  const f = frame('0x0000000000000000000000000000000000000001', [1])
  assert.equal(c.__dclOneRelabelHost(f), f)
  const short = frame('0x00', [1])
  assert.equal(c.__dclOneRelabelHost(short), short)
})
