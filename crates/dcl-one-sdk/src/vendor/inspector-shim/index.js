'use strict'

// Minimal stand-in for @dcl/inspector, covering only what this toolchain needs.
//
// templates/data-layer-host.mjs reaches for @dcl/inspector in two places:
//
//   dump-crdt  -> inspector.dumpEngineToCrdtCommands(engine)
//   serve      -> inspector.createDataLayerHost / DataServiceDefinition
//
// Both are implemented here. What is NOT here is the reason the real package
// weighs 119 MB: the browser editor UI — an 18 MB bundle.js of which ~10.8 MB
// is a non-tree-shaken Babylon, plus 2,967 Font Awesome icons. This package
// serves no `public/`, so `start --data-layer` gets a working data layer and
// no editor to point at it unless a real @dcl/inspector supplies one.
//
// A genuine @dcl/inspector in the scene's node_modules always wins: req() in
// data-layer-host.mjs resolves against the scene first and only falls back
// here. This is a fallback, not a takeover.
//
// The pieces, and where each came from:
//
//   engine-to-composite.js  verbatim ports of dumpEngineToCrdtCommands and
//                           dumpEngineToComposite from upstream
//                           src/lib/data-layer/host/utils/engine-to-composite.ts
//   engine.js               upstream host/utils/engine.ts, with the editor's
//                           component set read from component-schemas.json
//                           instead of from @dcl/asset-packs + the inspector's
//                           own versioning registry
//   host.js                 4 live rpc methods of 22; the rest are inert,
//                           well-formed stubs. Read its header before assuming
//                           a method does anything.
//   data-layer.gen.js       the service descriptor. NOT hand-written: it is
//                           upstream's protoc output (proto/gen/data-layer.gen.ts,
//                           emitted by the @dcl/ts-proto FORK) transpiled to
//                           CommonJS by scripts/build-base-blob.py with the
//                           vendored typescript. Its only imports are `long`
//                           and `protobufjs/minimal`, both already in the blob.
//
// Not implemented, and honestly so:
//
//   generateEntityNamesType  upstream writes assets/scene/entity-names.ts on
//                            every save so scene code can name entities through
//                            a typed union. Nothing in this toolchain reads it.
//   createEditorComponents   the named component handles. createEngineContext
//                            defines the same components, it just does not hand
//                            back a keyed object of them.

const { createEngineContext, serializeEngine } = require('./engine')
const { dumpEngineToCrdtCommands, dumpEngineToComposite } = require('./engine-to-composite')
const { createDataLayerHost } = require('./host')
const { DataServiceDefinition } = require('./data-layer.gen.js')

function editorOnly(name) {
  return () => {
    throw new Error(
      `@dcl/inspector.${name} needs the full inspector package, which carries the ` +
        'editor UI: `npm install --save-dev @dcl/inspector` in the scene, or point ' +
        'DCL_ONE_INSPECTOR_DIR at one. Building main.crdt and editing over the data ' +
        'layer both work without it.'
    )
  }
}

module.exports = {
  dumpEngineToCrdtCommands,
  dumpEngineToComposite,
  createEngineContext,
  serializeEngine,
  createDataLayerHost,
  DataServiceDefinition,
  createEditorComponents: editorOnly('createEditorComponents'),
  generateEntityNamesType: editorOnly('generateEntityNamesType')
}
