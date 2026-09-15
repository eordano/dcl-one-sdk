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

// Bundle bytes are UTF-8; TextDecoder is not guaranteed in Explorer sandboxes.
// Keep a dependency-free fallback with the native decoder's replacement and BOM
// behavior. Flush UTF-16 code units in bounded chunks to avoid argument limits.
function __dclOneDecode(__dclOneBytes) {
  if (typeof TextDecoder === 'function') {
    try {
      return new TextDecoder().decode(__dclOneBytes)
    } catch (__dclOneErr) {}
  }
  var __dclOneParts = []
  var __dclOneUnits = []
  var __dclOneI =
    __dclOneBytes[0] === 0xef && __dclOneBytes[1] === 0xbb && __dclOneBytes[2] === 0xbf ? 3 : 0
  while (__dclOneI < __dclOneBytes.length) {
    var __dclOneFirst = __dclOneBytes[__dclOneI++]
    var __dclOnePoint = __dclOneFirst
    if (__dclOneFirst >= 0x80) {
      var __dclOneCount =
        __dclOneFirst >= 0xc2 && __dclOneFirst <= 0xdf ? 1 :
        __dclOneFirst >= 0xe0 && __dclOneFirst <= 0xef ? 2 :
        __dclOneFirst >= 0xf0 && __dclOneFirst <= 0xf4 ? 3 : 0
      __dclOnePoint = __dclOneFirst & (0x7f >> __dclOneCount)
      var __dclOneValid = __dclOneCount !== 0
      for (var __dclOneN = 0; __dclOneN < __dclOneCount; __dclOneN++) {
        var __dclOneNext = __dclOneBytes[__dclOneI]
        if (__dclOneNext === undefined || __dclOneNext < 0x80 || __dclOneNext > 0xbf ||
            (__dclOneN === 0 && (
              (__dclOneFirst === 0xe0 && __dclOneNext < 0xa0) ||
              (__dclOneFirst === 0xed && __dclOneNext > 0x9f) ||
              (__dclOneFirst === 0xf0 && __dclOneNext < 0x90) ||
              (__dclOneFirst === 0xf4 && __dclOneNext > 0x8f)))) {
          __dclOneValid = false
          break
        }
        __dclOnePoint = (__dclOnePoint << 6) | (__dclOneNext & 0x3f)
        __dclOneI++
      }
      if (!__dclOneValid) __dclOnePoint = 0xfffd
    }
    if (__dclOnePoint > 0xffff) {
      __dclOnePoint -= 0x10000
      __dclOneUnits.push(0xd800 + (__dclOnePoint >> 10), 0xdc00 + (__dclOnePoint & 0x3ff))
    } else {
      __dclOneUnits.push(__dclOnePoint)
    }
    if (__dclOneUnits.length >= 32768) {
      __dclOneParts.push(String.fromCharCode.apply(null, __dclOneUnits))
      __dclOneUnits = []
    }
  }
  if (__dclOneUnits.length) __dclOneParts.push(String.fromCharCode.apply(null, __dclOneUnits))
  return __dclOneParts.join('')
}

// Authoritative-multiplayer arming (scene.json authoritativeMultiplayer).
// Two effects, both read by the sdk chunk's sync transport:
//
// 1. globalThis.__dclOneAuthoritative tells the chunk whether the scene has a
//    server to trust. The auth-server @dcl/sdk line applies CRDT, state
//    responses and room events only from the sender 'authoritative-server';
//    without the flag the blob's overlay (scripts/blob_overlays.py,
//    patch_sdk_peer_trust) falls back to mainline's peer trust so a
//    serverless-multiplayer scene keeps syncing.
// 2. With the flag, CommunicationsController is wrapped so inbound frames
//    the engine stamped with the preview host's zero address (mini-comms
//    HOST_ADDRESS) are re-labelled as 'authoritative-server': the engine
//    names peers by hex address and cannot present the host as anything
//    else, while production comms already present the scene-state server
//    under that name (and no real peer owns the zero address).
//
// Without the flag __dclOneMp is a literal false and nothing changes.
var __dclOneMp = __DCL_ONE_MP__
globalThis.__dclOneAuthoritative = __dclOneMp
var __dclOneAuthorityLabel = 'authoritative-server'
// [senderLen u8][sender utf8][payload]: is the sender 0x + 40 zeros?
function __dclOneFromHost(__dclOneMsg) {
  if (!__dclOneMsg || __dclOneMsg.length < 43 || __dclOneMsg[0] !== 42) return false
  if (__dclOneMsg[1] !== 48 || (__dclOneMsg[2] | 32) !== 120) return false
  for (var __dclOneI = 3; __dclOneI < 43; __dclOneI++) {
    if (__dclOneMsg[__dclOneI] !== 48) return false
  }
  return true
}
function __dclOneRelabelHost(__dclOneMsg) {
  if (!__dclOneFromHost(__dclOneMsg)) return __dclOneMsg
  var __dclOneOut = new Uint8Array(1 + __dclOneAuthorityLabel.length + (__dclOneMsg.length - 43))
  __dclOneOut[0] = __dclOneAuthorityLabel.length
  for (var __dclOneI = 0; __dclOneI < __dclOneAuthorityLabel.length; __dclOneI++) {
    __dclOneOut[1 + __dclOneI] = __dclOneAuthorityLabel.charCodeAt(__dclOneI)
  }
  __dclOneOut.set(__dclOneMsg.subarray(43), 1 + __dclOneAuthorityLabel.length)
  return __dclOneOut
}
function __dclOneMpWrap(__dclOneHostRequire) {
  if (!__dclOneMp) return __dclOneHostRequire
  var __dclOneComms = null
  return function (__dclOneSpec) {
    if (__dclOneSpec !== '~system/CommunicationsController') {
      return __dclOneHostRequire(__dclOneSpec)
    }
    if (__dclOneComms) return __dclOneComms
    var __dclOneReal = __dclOneHostRequire(__dclOneSpec)
    __dclOneComms = {
      send: function (__dclOneBody) {
        return __dclOneReal.send(__dclOneBody)
      },
      sendBinary: function (__dclOneBody) {
        return __dclOneReal.sendBinary(__dclOneBody).then(function (__dclOneRes) {
          var __dclOneData = (__dclOneRes && __dclOneRes.data) || []
          for (var __dclOneJ = 0; __dclOneJ < __dclOneData.length; __dclOneJ++) {
            __dclOneData[__dclOneJ] = __dclOneRelabelHost(__dclOneData[__dclOneJ])
          }
          return { data: __dclOneData }
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
