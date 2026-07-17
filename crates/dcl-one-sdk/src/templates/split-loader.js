// dcl-one-sdk split-bundle loader stub. Generated into the scene's `main` file by
// `dcl-one-sdk build --split-sdk`; template lives at
// crates/dcl-one-sdk/src/templates/split-loader.js.
// Every local is __dclOne-prefixed: chunk code is DIRECT-eval'd, so this scope
// chain is visible to chunk free identifiers.
'use strict'

var __dclOneSdkChunkPath = '__DCL_ONE_SDK_CHUNK__'
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
  var __dclOneRuntime = require('~system/Runtime')
  var __dclOneSdkSrc = __dclOneDecode(
    (await __dclOneRuntime.readFile({ fileName: __dclOneSdkChunkPath })).content
  )
  var __dclOneSceneSrc = __dclOneDecode(
    (await __dclOneRuntime.readFile({ fileName: __dclOneSceneChunkPath })).content
  )
  var __dclOneRegistry = __dclOneEvalChunk(
    __dclOneSdkSrc,
    __dclOneSdkChunkPath,
    __dclOneMakeRequire({}, require)
  )
  var __dclOneScene = __dclOneEvalChunk(
    __dclOneSceneSrc,
    __dclOneSceneChunkPath,
    __dclOneMakeRequire(__dclOneRegistry, require)
  )
  __dclOneSceneModule = __dclOneScene
  if (typeof __dclOneScene.onStart === 'function') return __dclOneScene.onStart()
}

module.exports.onUpdate = function (__dclOneDeltaTime) {
  if (__dclOneSceneModule && typeof __dclOneSceneModule.onUpdate === 'function') {
    return __dclOneSceneModule.onUpdate(__dclOneDeltaTime)
  }
}
