'use strict'

// The minimal data-layer host: the RPC service the browser editor talks to.
//
// Upstream's host is `src/lib/data-layer/host/*` — ~2.3 MB once its imports of
// `@dcl/asset-packs`, the undo/redo state machine and the migration passes are
// pulled in. FOUR of its 22 methods are what a live editing session actually
// turns on, and all four are cheap once `@dcl/ecs` is present (it is: the blob
// ships `dist-cjs` for the crdt dumper already):
//
//   crdtStream               the engine <-> editor wire, both directions
//   save                     dumpEngineToComposite -> assets/scene/main.composite
//   get/setInspectorPreferences
//
// The other 18 are stubs that return well-formed empty responses. They are not
// optional: `@dcl/rpc`'s `codegen.registerService` does `mod[key].bind(mod)`
// for every key in the descriptor, so a MISSING key is a TypeError at
// port-registration time that kills the whole connection — and
// `serverProcedureUnary` throws "Empty or null responses are not allowed" on
// any falsy result, so a stub must return `{}`, never `undefined`.
//
// What that costs a real browser editor, counted at creator-hub c117625: the
// asset panel is empty (`getAssetCatalog`, `getAssetData` x3, `getFiles`,
// `getFilesSizes`), drag-and-drop import is inert (`importAsset` x3), file
// reads return nothing (`getFile` x6, `getFilesList` x3) and undo/redo are
// dead (`undo` x4, `redo` x3, `getUndoRedoState`). Everything is REACHABLE and
// returns empty rather than throwing; the stubs are truthful about failing
// (`removeFiles` reports every path in `failed`) so the UI shows an error
// instead of pretending the work happened. Anyone who needs those installs the
// real `@dcl/inspector`, which still wins at resolution time — `req()` in
// `templates/data-layer-host.mjs` resolves the scene's node_modules first and
// only falls back to this package.
//
// Deliberately NOT ported:
//   * undo/redo — a 978-line state machine with 100 ms transaction batching.
//   * the 8 load-time migrations in `composite-provider.ts::runMigrations`. We
//     pass a composite through verbatim: a scene carrying
//     `inspector::SceneMetadata-v4` stays v4 here and is migrated when Creator
//     Hub next opens it. That is the safe direction — we never rewrite.
//   * `SceneProvider` / `overrideWithSceneJson`, which sync engine
//     `SceneMetadata` to and from `scene.json`. Scene-metadata edits made
//     against THIS host do not reach `scene.json`.

const {
  createEngineContext,
  serializeEngine,
  ECS
} = require('./engine')
const { dumpEngineToComposite } = require('./engine-to-composite')

const { AsyncQueue } = require('@dcl/rpc/dist/push-channel')

/**
 * The subset of upstream's `FileSystemInterface` this host calls. Supplied by
 * `templates/data-layer-host.mjs`, which sandboxes every path against the
 * project dir; nothing here may assume more than these seven.
 *
 * @typedef {object} FileSystemInterface
 * @property {(p: string) => string} dirname
 * @property {(p: string) => string} basename
 * @property {(...p: string[]) => string} join
 * @property {(p: string) => Promise<boolean>} existFile
 * @property {(p: string) => Promise<Buffer>} readFile
 * @property {(p: string, content: Buffer) => Promise<void>} writeFile
 * @property {(p: string) => Promise<{ name: string, isDirectory: boolean }[]>} readdir
 */

/**
 * Every method named by the generated service descriptor.
 *
 * This is the first of the header's two hazards made checkable. `@dcl/rpc`'s
 * `codegen.registerService` does `mod[key].bind(mod)` for every key in the
 * descriptor, so a name this object is missing is a TypeError at
 * port-registration that kills the whole connection. Keying the record on the
 * descriptor's own method names means a rename upstream fails the type check
 * here rather than the next editor session.
 *
 * The second hazard — `serverProcedureUnary` throws "Empty or null responses
 * are not allowed" on any falsy result — is NOT enforced here, and the return
 * type is written as it is only so it describes the truth. TypeScript does not
 * check return types through JSDoc in a .js file: an
 * `async f() { return undefined }` is accepted here and rejected by the
 * identical .ts. `scripts/check-editor-host-runtime.cjs` calls every method
 * instead, which is the only place that rule can actually be held.
 *
 * @typedef {keyof typeof import('./data-layer.gen').DataServiceDefinition['methods']} DataServiceMethodName
 * @typedef {Record<DataServiceMethodName, (...args: any[]) => Promise<object> | AsyncIterable<any>>} DataServiceMethods
 */

const MAIN_COMPOSITE_PATH = 'assets/scene/main.composite'
const PREFERENCES_PATH = 'inspector-preferences.json'
const COMPOSITE_SCAN_IGNORE = ['node_modules', 'dist', 'bin', 'src', '.vscode']
// Upstream's `CompositeProvider.minSaveInterval`.
const AUTOSAVE_INTERVAL_MS = 100

// --------------------------------------------------------------- preferences

// Upstream validates with ajv against a JTD schema (`logic/preferences/io.ts`).
// The schema has exactly two optional booleans, so the two checks are inlined
// rather than vendoring ajv; every parse failure falls back to the defaults,
// which is also what upstream does with an `InvalidPreferences`.
function defaultPreferences() {
  return { freeCameraInvertRotation: false, autosaveEnabled: true }
}

function parseInspectorPreferences(content) {
  const prefs = defaultPreferences()
  let parsed
  try {
    parsed = JSON.parse(content)
  } catch {
    return prefs
  }
  if (!parsed || typeof parsed !== 'object' || parsed.version !== 1) return prefs
  const data = parsed.data
  if (!data || typeof data !== 'object') return prefs
  if (typeof data.freeCameraInvertRotation === 'boolean') {
    prefs.freeCameraInvertRotation = data.freeCameraInvertRotation
  }
  if (typeof data.autosaveEnabled === 'boolean') prefs.autosaveEnabled = data.autosaveEnabled
  return prefs
}

function serializeInspectorPreferences(value) {
  return Buffer.from(JSON.stringify({ version: 1, data: value }, null, 2), 'utf-8')
}

// A port of `getFilesInDirectory` from `host/fs-utils.ts` minus the `ignore`
// package. Upstream feeds it gitignore patterns; the only call site left here
// passes bare directory names, and a bare name in gitignore matches at any
// depth — which is exactly basename matching, so the dependency buys nothing.
async function listFiles(fs, dirPath, ignore, out) {
  let entries
  try {
    entries = await fs.readdir(dirPath)
  } catch {
    return out
  }
  for (const entry of entries) {
    if (ignore.includes(entry.name) || entry.name.startsWith('.')) continue
    const sep = dirPath.length && !dirPath.endsWith('/') ? '/' : ''
    const full = dirPath + sep + entry.name
    if (entry.isDirectory) await listFiles(fs, full, ignore, out)
    else out.push(full)
  }
  return out
}

// A port of `host/utils/fs-composite-provider.ts`. The scan matters even
// though we only ever save ONE composite: `Composite.instance` resolves a
// scene's child composites through this provider, and an unresolvable child
// silently loses every entity under it.
async function createFsCompositeProvider(fs) {
  const files = await listFiles(fs, '', COMPOSITE_SCAN_IGNORE, [])
  const normalize = (v) => v.replace(/\\/g, '/').toLocaleLowerCase()
  const composites = []
  for (const file of files) {
    if (!file.endsWith('.composite') && !file.endsWith('.composite.bin')) continue
    const src = fs.join(fs.dirname(file), fs.basename(file).toLowerCase())
    try {
      const content = await fs.readFile(src)
      const composite = src.endsWith('.bin')
        ? ECS.Composite.fromBinary(new Uint8Array(content))
        : ECS.Composite.fromJson(JSON.parse(new TextDecoder().decode(content)))
      composites.push({ src, composite })
    } catch (err) {
      console.error(`Error loading composite ${src}: ${err}`)
    }
  }
  return {
    getCompositeOrNull(src) {
      return composites.find((item) => normalize(item.src) === normalize(src)) || null
    },
    async save(resource) {
      // json only: `save()` is the sole caller and upstream passes 'json' too.
      const text = JSON.stringify(ECS.Composite.toJson(resource.composite), null, 2)
      await fs.writeFile(resource.src, Buffer.from(text, 'utf-8'))
      const existing = composites.find((item) => item.src === resource.src)
      // Deep clone through the serialized form, exactly as upstream does: the
      // caller keeps a reference to the definition it handed us.
      if (existing) existing.composite = ECS.Composite.fromJson(JSON.parse(text))
    }
  }
}

// A port of `host/stream.ts` + `logic/consume-stream.ts`.
//
// The ORDER here is the contract: the engine's whole state is enqueued BEFORE
// the transport is registered and before the first `engine.update`, so the
// first message the editor receives is the scene it is about to render. Get
// that wrong and the editor opens on an empty world.
function createStream(iter, engine) {
  const queue = new AsyncQueue(() => {})
  queue.enqueue({ data: serializeEngine(engine) })

  const transport = {
    name: 'DataLayerHost',
    filter() {
      return !queue.closed
    },
    async send(message) {
      if (queue.closed) return
      queue.enqueue({ data: message })
    }
  }
  engine.addTransport(transport)

  const consume = async () => {
    for await (const message of iter) {
      if (message.data && message.data.byteLength) {
        transport.onmessage(message.data)
        void engine.update(1)
      }
    }
  }
  consume().catch((err) => {
    // The editor closing its tab is a normal end of stream, not a fault.
    if (err instanceof Error && !err.message.includes('RPC Transport closed')) {
      console.error('Failed to consume stream from data layer ', err)
    }
    queue.close()
  })

  void engine.update(1)
  return queue
}

/**
 * `dist-cjs`, not `@dcl/ecs`: engine.js requires the CommonJS build, and the
 * two builds' `IEngine` are distinct types to the compiler.
 *
 * @param {FileSystemInterface} fs from templates/data-layer-host.mjs
 * @returns {Promise<{ rpcMethods: DataServiceMethods, engine: import('@dcl/ecs/dist-cjs').IEngine }>}
 */
async function createDataLayerHost(fs) {
  const listeners = []
  const { engine } = createEngineContext({
    onChangeFunction: (entity, operation, component, componentValue) => {
      for (const fn of listeners) fn(entity, operation, component, componentValue)
    }
  })

  let preferences = defaultPreferences()
  if (await fs.existFile(PREFERENCES_PATH)) {
    preferences = parseInspectorPreferences(
      new TextDecoder().decode(await fs.readFile(PREFERENCES_PATH))
    )
  }

  // Boot must MATERIALIZE the composite, not wait for the first save: the
  // preview server's file watcher is what turns a composite into main.crdt,
  // and a scene scaffolded without one would otherwise never get either.
  if (!(await fs.existFile(MAIN_COMPOSITE_PATH))) {
    const minimal = JSON.stringify(require('./minimal-composite.json'), null, 2)
    await fs.writeFile(MAIN_COMPOSITE_PATH, Buffer.from(minimal, 'utf-8'))
  }

  const provider = await createFsCompositeProvider(fs)
  const resource = provider.getCompositeOrNull(MAIN_COMPOSITE_PATH)
  if (!resource) throw new Error(`could not load ${MAIN_COMPOSITE_PATH}`)
  ECS.Composite.instance(engine, resource, provider, {
    entityMapping: {
      // DIRECT mapping is the identity, which is the point: composite entity
      // ids ARE engine entity ids here, so a scene round-trips through save
      // with its ids intact. The signature admits a bare `number` for the
      // generating modes, hence the cast on the way back out.
      type: ECS.EntityMappingMode.EMM_DIRECT_MAPPING,
      getCompositeEntity: (entity) => /** @type {import('@dcl/ecs/dist-cjs').Entity} */ (entity)
    }
  })

  // One tick after the load, and it is NOT cosmetic. `Composite.instance` only
  // marks components dirty; a component's crdt timestamp map — the thing
  // `dumpCrdtStateToBuffer` iterates — is not populated until an
  // `engine.update` has run the crdt system. Skip this and `serializeEngine`
  // returns zero bytes, so the first message of `crdtStream` is empty and the
  // editor opens on an empty world.
  //
  // Upstream gets the same tick by accident: `composite-provider.ts`'s
  // `runMigrations()` calls `fixNetworkEntityValues` and `createTagsComponent`,
  // both of which end in `engine.update(1)`. We do not port the migrations
  // (see the header), so the tick has to be explicit.
  await engine.update(1)

  let saving = null
  async function save() {
    while (saving) await saving
    saving = (async () => {
      const composite = dumpEngineToComposite(engine, 'json')
      await provider.save({ src: MAIN_COMPOSITE_PATH, composite })
    })()
    try {
      await saving
    } finally {
      saving = null
    }
  }

  // Autosave. `getInspectorPreferences` reports `autosaveEnabled: true` by
  // upstream's default, and an editor that believes it loses work if we only
  // ever write on an explicit `save()`. Upstream drives this from its state
  // manager's transaction boundaries with a 100 ms floor; without transactions
  // the same guarantee is a trailing-edge debounce over engine changes.
  let autosaveTimer = null
  let booted = false
  listeners.push((_entity, _operation, component) => {
    if (!booted || !preferences.autosaveEnabled) return
    // Selection is editor UI state, not scene content; upstream's
    // `CompositeProvider.canHandle` refuses it too.
    if (component && component.componentName === 'inspector::Selection') return
    if (autosaveTimer) return
    autosaveTimer = setTimeout(() => {
      autosaveTimer = null
      save().catch((e) => console.error('autosave failed:', e))
    }, AUTOSAVE_INTERVAL_MS)
    if (autosaveTimer.unref) autosaveTimer.unref()
  })
  booted = true

  /** @type {DataServiceMethods} */
  const rpcMethods = {
    crdtStream(iter) {
      return createStream(iter, engine)
    },
    async save() {
      await save()
      return {}
    },
    async getInspectorPreferences() {
      return preferences
    },
    async setInspectorPreferences(req) {
      preferences = {
        freeCameraInvertRotation: !!req.freeCameraInvertRotation,
        autosaveEnabled: !!req.autosaveEnabled
      }
      await fs.writeFile(PREFERENCES_PATH, serializeInspectorPreferences(preferences))
      return {}
    },

    // undo/redo: no history is kept, so nothing can be undone.
    async undo() {
      return { type: '' }
    },
    async redo() {
      return { type: '' }
    },
    async getUndoRedoState() {
      return { canUndo: false, canRedo: false }
    },

    // file i/o: this host owns exactly one file, the main composite.
    async getFiles() {
      return { files: [] }
    },
    async getFilesSizes() {
      return { files: [] }
    },
    async saveFile() {
      return {}
    },
    async copyFile() {
      return {}
    },
    async getFile() {
      return { content: new Uint8Array(0) }
    },
    async getFilesList(req) {
      return {
        files: (req.paths || []).map((path) => ({
          path,
          content: new Uint8Array(0),
          success: false,
          error: 'file i/o is not implemented by the offline data-layer host'
        }))
      }
    },
    async removeFiles(req) {
      // Truthful, not optimistic: nothing was removed, so nothing succeeded.
      return { success: [], failed: req.filePaths || [] }
    },

    // asset catalog: the base path is real so the editor builds valid URLs,
    // but nothing is indexed.
    async getAssetCatalog() {
      return { basePath: 'assets', assets: [] }
    },
    async getAssetData() {
      return { data: new Uint8Array(0) }
    },
    async importAsset() {
      return {}
    },
    async removeAsset() {
      return {}
    },

    // custom assets.
    async createCustomAsset() {
      return { asset: { data: new Uint8Array(0) } }
    },
    async getCustomAssets() {
      return { assets: [] }
    },
    async deleteCustomAsset() {
      return {}
    },
    async renameCustomAsset() {
      return {}
    }
  }

  return { rpcMethods, engine }
}

module.exports = {
  createDataLayerHost,
  createStream,
  createFsCompositeProvider,
  parseInspectorPreferences,
  serializeInspectorPreferences,
  defaultPreferences,
  MAIN_COMPOSITE_PATH,
  PREFERENCES_PATH
}
