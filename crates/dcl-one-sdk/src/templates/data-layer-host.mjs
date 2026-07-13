import { createRequire } from 'node:module'
import path from 'node:path'
import fs from 'node:fs'

const projectDir = path.resolve(process.argv[2])
const mode = process.argv[3] || 'serve'
process.chdir(projectDir)

const inspectorDir = process.env.DCL_ONE_INSPECTOR_DIR || ''
const sceneRequire = createRequire(path.join(projectDir, 'package.json'))
const inspectorRequire = inspectorDir
  ? createRequire(path.join(inspectorDir, 'package.json'))
  : null

function req(name) {
  let lastErr
  try {
    return sceneRequire(name)
  } catch (e) {
    lastErr = e
  }
  if (inspectorRequire) {
    if (name === '@dcl/inspector') {
      try {
        return inspectorRequire(inspectorDir)
      } catch (e) {
        lastErr = e
      }
    } else {
      try {
        return inspectorRequire(name)
      } catch (e) {
        lastErr = e
      }
    }
  }
  throw lastErr
}

function pathToPosix(v) {
  return v.replace(/\\/g, '/')
}

function makeFsInterface(workdir) {
  const resolve = (p) => (path.isAbsolute(p) ? p : path.resolve(workdir, p))
  return {
    dirname: (v) => pathToPosix(path.dirname(v)),
    basename: (v) => pathToPosix(path.basename(v)),
    join: (...paths) => path.join(...paths),
    async existFile(filePath) {
      try {
        return (await fs.promises.stat(resolve(filePath))).isFile()
      } catch {
        return false
      }
    },
    async readFile(filePath) {
      return fs.promises.readFile(resolve(filePath))
    },
    async writeFile(filePath, content) {
      const resolved = resolve(filePath)
      await fs.promises.mkdir(path.dirname(resolved), { recursive: true })
      await fs.promises.writeFile(resolved, content)
    },
    async rm(filePath) {
      await fs.promises.rm(resolve(filePath))
    },
    async rmdir(dirPath) {
      await fs.promises.rm(resolve(dirPath), { recursive: true })
    },
    async readdir(dirPath) {
      if (dirPath.indexOf('/../') !== -1) throw new Error('The usage of /../ is not allowed')
      const root = dirPath === '.' || dirPath === './' || dirPath === ''
      const resolved = root ? workdir : dirPath
      const entries = await fs.promises.readdir(resolved)
      return Promise.all(
        entries.map(async (name) => {
          let isDirectory = false
          try {
            isDirectory = (await fs.promises.stat(path.resolve(resolved, name))).isDirectory()
          } catch {}
          return { name: pathToPosix(name), isDirectory }
        })
      )
    },
    cwd: () => pathToPosix(workdir),
    async stat(filePath) {
      const stats = await fs.promises.stat(resolve(filePath))
      return { size: Number(stats.size) }
    }
  }
}

function findComposites(dir, base, out) {
  let entries
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true })
  } catch {
    return out
  }
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue
    const abs = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'bin' || entry.name === 'dist') continue
      findComposites(abs, base, out)
    } else if (entry.name.endsWith('.composite')) {
      out.push(pathToPosix(path.relative(base, abs)))
    }
  }
  return out
}

async function dumpCrdtFallback() {
  const inspector = req('@dcl/inspector')
  const { Composite, Engine, EntityMappingMode } = req('@dcl/ecs/dist-cjs')
  const files = findComposites(projectDir, projectDir, []).sort()
  const composites = {}
  for (const file of files) {
    const raw = await fs.promises.readFile(path.join(projectDir, file), 'utf8')
    composites[file] = Composite.fromJson(JSON.parse(raw))
  }
  const provider = {
    getCompositeOrNull(src) {
      return src in composites ? { src, composite: composites[src] } : null
    }
  }
  const engine = Engine()
  for (const src of Object.keys(composites)) {
    Composite.instance(engine, provider.getCompositeOrNull(src), provider, {
      entityMapping: {
        type: EntityMappingMode.EMM_DIRECT_MAPPING,
        getCompositeEntity: (e) => e
      }
    })
  }
  const crdt = inspector.dumpEngineToCrdtCommands(engine)
  await fs.promises.writeFile(path.join(projectDir, 'main.crdt'), crdt)
  process.stdout.write(JSON.stringify({ ok: true, composites: files.length }) + '\n')
}

async function dumpCrdt() {
  let getAllComposites
  try {
    ;({ getAllComposites } = req('@dcl/sdk-commands/dist/logic/composite.js'))
  } catch {}
  if (!getAllComposites) return dumpCrdtFallback()
  const log = (...args) => console.error(...args)
  const components = {
    fs: {
      readFile: (p) => fs.promises.readFile(p),
      writeFile: (p, data) => fs.promises.writeFile(p, data)
    },
    logger: { log, info: log, warn: log, error: log, debug: log }
  }
  const data = await getAllComposites(components, projectDir)
  process.stdout.write(
    JSON.stringify({ ok: !data.withErrors, composites: data.compositeLines.length }) + '\n'
  )
}

async function serve() {
  const inspector = req('@dcl/inspector')
  const { createRpcServer } = req('@dcl/rpc')
  const codegen = req('@dcl/rpc/dist/codegen')
  const { WebSocketTransport } = req('@dcl/rpc/dist/transports/WebSocket')
  const { WebSocketServer } = req('ws')

  const fsInterface = makeFsInterface(projectDir)
  const host = await inspector.createDataLayerHost(fsInterface)
  const logToStderr = (...args) => console.error(...args)
  const rpcServer = createRpcServer({
    logger: {
      log: logToStderr,
      info: logToStderr,
      warn: logToStderr,
      error: logToStderr,
      debug: logToStderr
    }
  })
  rpcServer.setHandler(async (serverPort) => {
    codegen.registerService(
      serverPort,
      inspector.DataServiceDefinition,
      async () => host.rpcMethods
    )
  })
  const wss = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  wss.on('connection', (ws) => {
    const transport = WebSocketTransport(ws)
    rpcServer.attachTransport(transport, { fs: fsInterface, engine: host.engine })
    ws.on('error', () => ws.close())
  })
  wss.on('listening', () => {
    process.stdout.write(JSON.stringify({ ready: true, port: wss.address().port }) + '\n')
  })
  process.stdin.resume()
  process.stdin.on('end', () => process.exit(0))
  process.stdin.on('close', () => process.exit(0))
}

const run = mode === 'dump-crdt' ? dumpCrdt : serve
run().catch((e) => {
  console.error(e && e.stack ? e.stack : String(e))
  process.exit(1)
})
