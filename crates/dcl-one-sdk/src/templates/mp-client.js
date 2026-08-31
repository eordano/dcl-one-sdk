// dcl-one-sdk mp-client -- the client half of the authoritative-server API
// surface (docs/multiplayer-server-design.md, M2). Generated into .dcl-one/
// and imported before the scene when scene.json carries
// authoritativeMultiplayer: true; the armed split loader owns the transport
// side (folding DCLR room envelopes out of the sync stream into
// __dclOneMpInbox, sending __dclOneMpOutbox with the next sync flush).
//
// registerMessages/isServer are ADDED to the '@dcl/sdk/network' namespace --
// new keys on an extensible exports object -- so scene feature detection
// finds the room exactly as it would under the upstream auth-server SDK.
// Inbound sender identity is not verifiable at a client (the platform hands
// scenes bytes, not senders); state authority lives server-side where the
// relay stamps verified addresses.
import * as network from '@dcl/sdk/network'
import { engine } from '@dcl/sdk/ecs'

var inbox = (globalThis.__dclOneMpInbox = globalThis.__dclOneMpInbox || [])
var outbox = (globalThis.__dclOneMpOutbox = globalThis.__dclOneMpOutbox || [])
var handlers = new Map()
var room = null

function utf8Encode(str) {
  var out = []
  for (var i = 0; i < str.length; i++) {
    var c = str.codePointAt(i)
    if (c > 0xffff) i++
    if (c < 0x80) out.push(c)
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63))
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63))
    else
      out.push(
        0xf0 | (c >> 18),
        0x80 | ((c >> 12) & 63),
        0x80 | ((c >> 6) & 63),
        0x80 | (c & 63)
      )
  }
  return out
}
function utf8Decode(bytes, from) {
  var out = ''
  var i = from
  while (i < bytes.length) {
    var b = bytes[i++]
    var c
    if (b < 0x80) c = b
    else if (b < 0xe0) c = ((b & 31) << 6) | (bytes[i++] & 63)
    else if (b < 0xf0) c = ((b & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63)
    else
      c =
        ((b & 7) << 18) |
        ((bytes[i++] & 63) << 12) |
        ((bytes[i++] & 63) << 6) |
        (bytes[i++] & 63)
    out += String.fromCodePoint(c)
  }
  return out
}

function makeRoom() {
  if (room) return room
  room = {
    send: function (type, payload, opts) {
      var body = utf8Encode(JSON.stringify({ t: type, p: payload }))
      var bytes = new Uint8Array(4 + body.length)
      bytes[0] = 68 // D
      bytes[1] = 67 // C
      bytes[2] = 76 // L
      bytes[3] = 82 // R
      bytes.set(body, 4)
      outbox.push({ data: [bytes], address: (opts && opts.to) || [] })
    },
    onMessage: function (type, cb) {
      handlers.set(type, cb)
    }
  }
  return room
}

if (typeof network.registerMessages !== 'function') {
  Object.defineProperty(network, 'registerMessages', {
    configurable: true,
    value: function () {
      return makeRoom()
    }
  })
  Object.defineProperty(network, 'isServer', { configurable: true, value: () => false })
}

engine.addSystem(function () {
  while (inbox.length) {
    var bytes = inbox.shift()
    var msg
    try {
      msg = JSON.parse(utf8Decode(bytes, 4))
    } catch (e) {
      continue
    }
    var h = handlers.get(msg.t)
    if (h) {
      try {
        h(msg.p, { from: 'server' })
      } catch (e) {
        console.error('mp-client onMessage handler threw:', e)
      }
    }
  }
})
