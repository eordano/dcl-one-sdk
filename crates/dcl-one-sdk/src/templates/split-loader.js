// dcl-one-sdk split-bundle loader stub. Generated into the scene's `main` file by
// `dcl-one-sdk build --split-sdk`; template lives at
// crates/dcl-one-sdk/src/templates/split-loader.js.
// Every local is __dclOne-prefixed: chunk code is DIRECT-eval'd, so this scope
// chain is visible to chunk free identifiers.
'use strict'

var __dclOneSdkChunkPath = '__DCL_ONE_SDK_CHUNK__'
// Empty unless this scene uses smart items. The smart-item runtime is a second
// registry chunk layered over the first, so a scene without smart items never
// ships, reads, decodes or evaluates it.
var __dclOneSmartChunkPath = '__DCL_ONE_SMART_CHUNK__'
var __dclOneSceneChunkPath = '__DCL_ONE_SCENE_CHUNK__'
var __dclOneSceneModule = null

// Upstream bakes DCL_MAX_COMPOSITE_ENTITY into its single bundle as an esbuild
// define; the consumer (@dcl/ecs createEntityContainer) guards with a typeof
// check, so a global set before the sdk chunk evals is equivalent — and keeps
// the sdk-runtime chunk bytes independent of composite content (cache contract).
globalThis.DCL_MAX_COMPOSITE_ENTITY = __DCL_ONE_MAX_COMPOSITE_ENTITY__

// Chunks are esbuild --charset=ascii output (pure ASCII bytes), and TextDecoder is
// not a sandbox contract on either runtime, so decode with chunked
// String.fromCharCode and only opportunistically prefer TextDecoder when it exists.
function __dclOneDecode(__dclOneBytes) {
  if (typeof TextDecoder === 'function') {
    try {
      return new TextDecoder().decode(__dclOneBytes)
    } catch (__dclOneErr) {}
  }
  var __dclOneParts = []
  for (var __dclOneI = 0; __dclOneI < __dclOneBytes.length; __dclOneI += 32768) {
    var __dclOneSlice = __dclOneBytes.subarray
      ? __dclOneBytes.subarray(__dclOneI, __dclOneI + 32768)
      : __dclOneBytes.slice(__dclOneI, __dclOneI + 32768)
    __dclOneParts.push(String.fromCharCode.apply(null, __dclOneSlice))
  }
  return __dclOneParts.join('')
}

// Authoritative-multiplayer arming (scene.json authoritativeMultiplayer):
// wraps CommunicationsController so room-message envelopes (the 4-byte DCLR
// magic) are folded out of the sync transport's inbound stream into
// __dclOneMpInbox, and queued outbound envelopes in __dclOneMpOutbox ride
// the transport's next sendBinary. The mp-client entry module owns both
// queues; without the flag this is a literal false and nothing changes.
var __dclOneMp = __DCL_ONE_MP__
function __dclOneMpWrap(__dclOneHostRequire) {
  if (!__dclOneMp) return __dclOneHostRequire
  var __dclOneComms = null
  return function (__dclOneSpec) {
    if (__dclOneSpec !== '~system/CommunicationsController') {
      return __dclOneHostRequire(__dclOneSpec)
    }
    if (__dclOneComms) return __dclOneComms
    var __dclOneReal = __dclOneHostRequire(__dclOneSpec)
    var __dclOneInbox = (globalThis.__dclOneMpInbox = globalThis.__dclOneMpInbox || [])
    var __dclOneOutbox = (globalThis.__dclOneMpOutbox = globalThis.__dclOneMpOutbox || [])
    __dclOneComms = {
      send: function (__dclOneBody) {
        return __dclOneReal.send(__dclOneBody)
      },
      sendBinary: function (__dclOneBody) {
        var __dclOnePeer = (__dclOneBody && __dclOneBody.peerData) || []
        if (__dclOneOutbox.length) {
          __dclOnePeer = __dclOnePeer.slice()
          for (var __dclOneI = 0; __dclOneI < __dclOneOutbox.length; __dclOneI++) {
            __dclOnePeer.push(__dclOneOutbox[__dclOneI])
          }
          __dclOneOutbox.length = 0
        }
        var __dclOneReq = {
          data: (__dclOneBody && __dclOneBody.data) || [],
          peerData: __dclOnePeer
        }
        return __dclOneReal.sendBinary(__dclOneReq).then(function (__dclOneRes) {
          var __dclOneData = (__dclOneRes && __dclOneRes.data) || []
          var __dclOneKept = []
          for (var __dclOneJ = 0; __dclOneJ < __dclOneData.length; __dclOneJ++) {
            var __dclOneMsg = __dclOneData[__dclOneJ]
            if (
              __dclOneMsg.length > 4 &&
              __dclOneMsg[0] === 68 &&
              __dclOneMsg[1] === 67 &&
              __dclOneMsg[2] === 76 &&
              __dclOneMsg[3] === 82
            ) {
              __dclOneInbox.push(__dclOneMsg)
            } else {
              __dclOneKept.push(__dclOneMsg)
            }
          }
          return { data: __dclOneKept }
        })
      }
    }
    return __dclOneComms
  }
}

// ~system/* passes through to the host require; everything else must be a
// registry key or fail loudly (design section 4: wildcard externals are broader
// than the registry on purpose).
function __dclOneMakeRequire(__dclOneRegistry, __dclOneHostRequire) {
  return function (__dclOneSpec) {
    if (__dclOneSpec.lastIndexOf('~system/', 0) === 0) return __dclOneHostRequire(__dclOneSpec)
    if (__dclOneSpec in __dclOneRegistry) return __dclOneRegistry[__dclOneSpec]
    throw new Error(
      'dcl-one split bundle: "' + __dclOneSpec + '" is not in the sdk runtime registry'
    )
  }
}

// Layer one registry over another, later wins. Property *descriptors* are copied,
// not values: registry entries are lazy getters (`@dcl/sdk/platform` calls the
// host at module scope, so reading one eagerly here would run it before the scene
// starts), and the generated registries mark them configurable so the second
// defineProperty of the same key is legal. The one key that is deliberately in
// both chunks is '~sdk/script-utils' — core has the no-op stub, smart has the
// real runScripts runtime — so this shadowing is what makes smart items run.
function __dclOneOverlay(__dclOneBase, __dclOneTop) {
  var __dclOneOut = {}
  __dclOneCopyDescriptors(__dclOneOut, __dclOneBase)
  __dclOneCopyDescriptors(__dclOneOut, __dclOneTop)
  return __dclOneOut
}

function __dclOneCopyDescriptors(__dclOneTarget, __dclOneSource) {
  var __dclOneKeys = Object.keys(__dclOneSource)
  for (var __dclOneI = 0; __dclOneI < __dclOneKeys.length; __dclOneI++) {
    var __dclOneKey = __dclOneKeys[__dclOneI]
    Object.defineProperty(
      __dclOneTarget,
      __dclOneKey,
      Object.getOwnPropertyDescriptor(__dclOneSource, __dclOneKey)
    )
  }
}

// DIRECT eval, never new Function: the web sandbox provides console/fetch/Deno/etc.
// as lexical preamble consts of the stub wrapper, and only direct eval keeps them
// on the chunk's scope chain (hazard 8.4). The sourceURL suffix names the chunk in
// stack traces.
function __dclOneEvalChunk(__dclOneCode, __dclOnePath, __dclOneRequire) {
  var __dclOneModule = { exports: {} }
  var __dclOneGlobal = globalThis
  var __dclOneFactory = eval(
    '"use strict";(function(globalThis,module,exports,require){' +
      __dclOneCode +
      '\n})\n//# sourceURL=dcl-one:///' +
      __dclOnePath
  )
  __dclOneFactory.call(
    __dclOneGlobal,
    __dclOneGlobal,
    __dclOneModule,
    __dclOneModule.exports,
    __dclOneRequire
  )
  return __dclOneModule.exports
}

// Both runtimes fully await onStart before the first onUpdate, so the null guard
// in onUpdate is sufficient (design section 1).
module.exports.onStart = async function () {
  var __dclOneHostRequire = __dclOneMpWrap(require)
  var __dclOneRuntime = require('~system/Runtime')
  var __dclOneSdkSrc = __dclOneDecode(
    (await __dclOneRuntime.readFile({ fileName: __dclOneSdkChunkPath })).content
  )
  var __dclOneSceneSrc = __dclOneDecode(
    (await __dclOneRuntime.readFile({ fileName: __dclOneSceneChunkPath })).content
  )
  var __dclOneSmartSrc = __dclOneSmartChunkPath
    ? __dclOneDecode(
        (await __dclOneRuntime.readFile({ fileName: __dclOneSmartChunkPath })).content
      )
    : null
  var __dclOneRegistry = __dclOneEvalChunk(
    __dclOneSdkSrc,
    __dclOneSdkChunkPath,
    __dclOneMakeRequire({}, __dclOneHostRequire)
  )
  if (__dclOneSmartSrc !== null) {
    __dclOneRegistry = __dclOneOverlay(
      __dclOneRegistry,
      __dclOneEvalChunk(
        __dclOneSmartSrc,
        __dclOneSmartChunkPath,
        __dclOneMakeRequire(__dclOneRegistry, __dclOneHostRequire)
      )
    )
  }
  var __dclOneScene = __dclOneEvalChunk(
    __dclOneSceneSrc,
    __dclOneSceneChunkPath,
    __dclOneMakeRequire(__dclOneRegistry, __dclOneHostRequire)
  )
  __dclOneSceneModule = __dclOneScene
  if (typeof __dclOneScene.onStart === 'function') return __dclOneScene.onStart()
}

module.exports.onUpdate = function (__dclOneDeltaTime) {
  if (__dclOneSceneModule && typeof __dclOneSceneModule.onUpdate === 'function') {
    return __dclOneSceneModule.onUpdate(__dclOneDeltaTime)
  }
}
