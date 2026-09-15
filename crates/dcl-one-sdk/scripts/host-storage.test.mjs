// Behavioural contract of the host's `@dcl/sdk/server` client
// (src/templates/host-storage.mjs) against an in-process stand-in for the
// preview's storage routes: JSON values round-trip, reads are cached for the
// TTL with negative entries, in-flight reads coalesce, writes serialize per
// key and collapse to the latest value, unchanged writes are skipped, and
// nothing throws on a failing request.
import test from 'node:test'
import assert from 'node:assert/strict'
import http from 'node:http'
import { createServerModule, createWriteQueue, SOURCE_HEADER } from '../src/templates/host-storage.mjs'

function serve() {
  const world = new Map()
  const players = new Map()
  const env = new Map([['FROM_SERVER', 'yes']])
  const log = []
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://x')
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      log.push({ method: req.method, path: url.pathname + url.search, source: req.headers[SOURCE_HEADER], body })
      const json = (status, value) => {
        res.writeHead(status, { 'content-type': 'application/json' })
        res.end(value === undefined ? '' : JSON.stringify(value))
      }
      const parts = url.pathname.split('/').filter(Boolean)
      const scope =
        parts[0] === 'values'
          ? { map: world, key: parts[1] }
          : parts[0] === 'players'
            ? { map: players.get(parts[1].toLowerCase()) ?? players.set(parts[1].toLowerCase(), new Map()).get(parts[1].toLowerCase()), key: parts[3] }
            : null
      if (parts[0] === 'env') {
        const v = env.get(decodeURIComponent(parts[1]))
        return v === undefined ? json(404, { message: 'nope' }) : json(200, { value: v })
      }
      if (parts[0] === 'boom') return json(500, { message: 'boom' })
      if (!scope) return json(404, { message: 'no route' })
      const key = scope.key === undefined ? undefined : decodeURIComponent(scope.key)
      if (key === undefined) {
        const prefix = url.searchParams.get('prefix') ?? ''
        const entries = [...scope.map].filter(([k]) => k.startsWith(prefix)).map(([k, value]) => ({ key: k, value }))
        const offset = Number(url.searchParams.get('offset') ?? 0)
        const limit = Number(url.searchParams.get('limit') ?? entries.length)
        return json(200, { data: entries.slice(offset, offset + limit), pagination: { offset, total: entries.length } })
      }
      if (req.method === 'GET') {
        return scope.map.has(key) ? json(200, { value: scope.map.get(key) }) : json(404, { message: 'nope' })
      }
      if (req.method === 'PUT') {
        const { value } = JSON.parse(body)
        scope.map.set(key, value)
        return json(200, { value })
      }
      if (req.method === 'DELETE') {
        scope.map.delete(key)
        return json(204)
      }
      json(405, { message: 'method' })
    })
  })
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const base = `http://127.0.0.1:${server.address().port}`
      resolve({ base, world, players, env, log, close: () => new Promise((r) => server.close(r)) })
    })
  })
}

const quiet = () => {
  const original = console.error
  const lines = []
  console.error = (...a) => lines.push(a.map(String).join(' '))
  return { lines, restore: () => (console.error = original) }
}

test('values round-trip as JSON, not strings', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    assert.equal(await Storage.set('obj', { a: 1, b: [true, null] }), true)
    assert.equal(await Storage.set('num', 7), true)
    assert.deepEqual(await Storage.get('obj', { fresh: true }), { a: 1, b: [true, null] })
    assert.equal(await Storage.get('num', { fresh: true }), 7)
    assert.equal(s.world.get('num'), 7, 'the server holds the number, not "7"')
    assert.equal(s.log[0].source, 'scene', 'requests name their writer')
    assert.deepEqual(JSON.parse(s.log[0].body), { value: { a: 1, b: [true, null] } })
  } finally {
    await s.close()
  }
})

test('reads are cached within the TTL, including confirmed absences, and fresh bypasses', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    assert.equal(await Storage.get('missing'), null)
    assert.equal(await Storage.get('missing'), null)
    assert.equal(s.log.length, 1, 'the 404 is cached as absent')
    s.world.set('k', 'server-side')
    assert.equal(await Storage.get('k'), 'server-side')
    s.world.set('k', 'changed out of band')
    assert.equal(await Storage.get('k'), 'server-side', 'served from cache')
    assert.equal(await Storage.get('k', { fresh: true }), 'changed out of band')
    Storage.configure({ cacheReads: false })
    s.world.set('k', 'again')
    assert.equal(await Storage.get('k'), 'again')
  } finally {
    await s.close()
  }
})

test('concurrent reads of one key share a single request', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    s.world.set('k', 1)
    const results = await Promise.all([Storage.get('k'), Storage.get('k'), Storage.get('k', { fresh: true })])
    assert.deepEqual(results, [1, 1, 1])
    assert.equal(s.log.filter((l) => l.method === 'GET').length, 1)
  } finally {
    await s.close()
  }
})

test('rapid writes collapse to at most two requests and the last value wins', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    const writes = []
    for (let i = 0; i < 10; i++) writes.push(Storage.set('k', i))
    assert.deepEqual(await Promise.all(writes), Array(10).fill(true))
    const puts = s.log.filter((l) => l.method === 'PUT')
    assert.ok(puts.length <= 2, `${puts.length} PUTs`)
    assert.equal(s.world.get('k'), 9)
  } finally {
    await s.close()
  }
})

test('an unchanged write is skipped only once a round-trip proved the value stored', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    assert.equal(await Storage.set('k', 'v'), true)
    assert.equal(await Storage.set('k', 'v'), true)
    assert.equal(s.log.filter((l) => l.method === 'PUT').length, 1, 'the second set is a no-op')
    assert.equal(await Storage.set('k', 'v', { skipIfUnchanged: false }), true)
    assert.equal(s.log.filter((l) => l.method === 'PUT').length, 2)
    Storage.configure({ skipIfUnchanged: false })
    assert.equal(await Storage.set('k', 'v'), true)
    assert.equal(s.log.filter((l) => l.method === 'PUT').length, 3)
  } finally {
    await s.close()
  }
})

test('delete resolves true, invalidates the cache and answers null afterwards', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    await Storage.set('k', 1)
    assert.equal(await Storage.get('k'), 1)
    assert.equal(await Storage.delete('k'), true)
    assert.equal(await Storage.get('k'), null)
    assert.equal(s.world.has('k'), false)
    const gets = s.log.filter((l) => l.method === 'GET')
    assert.equal(gets.length, 0, 'both reads came from the cache')
  } finally {
    await s.close()
  }
})

test('getValues pages through the scene scope', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    for (const k of ['a1', 'a2', 'a3', 'b1']) s.world.set(k, k.toUpperCase())
    const page = await Storage.getValues({ prefix: 'a', limit: 2, offset: 1 })
    assert.deepEqual(page, {
      data: [
        { key: 'a2', value: 'A2' },
        { key: 'a3', value: 'A3' }
      ],
      pagination: { offset: 1, total: 3 }
    })
    assert.equal(s.log.at(-1).path, '/values?prefix=a&limit=2&offset=1')
    // upstream builds the query from truthy options only
    await Storage.getValues({ prefix: '', limit: 0, offset: 0 })
    assert.equal(s.log.at(-1).path, '/values')
    // a page seeds the per-key cache: the next get of a listed key is free
    const requests = s.log.length
    assert.equal(await Storage.get('a1'), 'A1')
    assert.equal(s.log.length, requests, 'served from the seeded cache')
    // and a failing list answers upstream's empty page
    const q = quiet()
    try {
      assert.deepEqual(await createServerModule({ baseUrl: `${s.base}/boom` }).Storage.getValues({ offset: 4 }), {
        data: [],
        pagination: { offset: 0, total: 0 }
      })
    } finally {
      q.restore()
    }
  } finally {
    await s.close()
  }
})

test('a missing key is an outcome, not an error line', async () => {
  const s = await serve()
  const q = quiet()
  try {
    const { Storage, EnvVar } = createServerModule({ baseUrl: s.base })
    assert.equal(await Storage.get('nope'), null)
    assert.deepEqual(q.lines, [], 'a 404 is not logged')
    assert.equal(await Storage.get('nope'), null)
    assert.equal(s.log.length, 1, 'the absence is cached too')
    // EnvVar keeps upstream's one line: it has no cache and no absent state
    assert.equal(await EnvVar.get('NOPE'), '')
    assert.deepEqual(q.lines, ["Failed to fetch environment variable 'NOPE': 404 Not Found"])
  } finally {
    q.restore()
    await s.close()
  }
})

test('player storage is scoped per lowercase address', async () => {
  const s = await serve()
  try {
    const { Storage } = createServerModule({ baseUrl: s.base })
    assert.equal(await Storage.player.set('0xABC', 'score', 3), true)
    assert.equal(await Storage.player.get('0xabc', 'score'), 3, 'the cache key is case-insensitive')
    assert.equal(await Storage.player.get('0xabc', 'score', { fresh: true }), 3)
    assert.equal(await Storage.player.get('0xdef', 'score'), null)
    assert.equal(s.players.get('0xabc').get('score'), 3)
    assert.equal(s.log.filter((l) => l.method === 'GET').length, 2)
    assert.deepEqual(await Storage.player.getValues('0xABC'), {
      data: [{ key: 'score', value: 3 }],
      pagination: { offset: 0, total: 1 }
    })
    assert.equal(await Storage.player.delete('0xabc', 'score'), true)
    assert.equal(s.players.get('0xabc').size, 0)
  } finally {
    await s.close()
  }
})

test('EnvVar answers the empty string for an unset variable', async () => {
  const s = await serve()
  try {
    const { EnvVar } = createServerModule({ baseUrl: s.base })
    assert.equal(await EnvVar.get('FROM_SERVER'), 'yes')
    const q = quiet()
    try {
      assert.equal(await EnvVar.get('NOPE'), '')
    } finally {
      q.restore()
    }
  } finally {
    await s.close()
  }
})

test('a failing request resolves false or null and never throws', async () => {
  const s = await serve()
  const q = quiet()
  try {
    const { Storage } = createServerModule({ baseUrl: `${s.base}/boom` })
    assert.equal(await Storage.set('k', 1), false)
    assert.equal(await Storage.get('k'), null)
    assert.equal(await Storage.delete('k'), false)
    const dead = createServerModule({ baseUrl: 'http://127.0.0.1:1' }).Storage
    assert.equal(await dead.set('k', 1), false)
    assert.equal(await dead.get('k'), null)
    assert.ok(q.lines.some((l) => l.includes("Failed to set storage value 'k'")), q.lines.join('\n'))
  } finally {
    q.restore()
    await s.close()
  }
})

test('the module refuses use outside a server isolate', async () => {
  const { Storage, EnvVar } = createServerModule({ baseUrl: 'http://127.0.0.1:1', isServer: () => false })
  await assert.rejects(Storage.get('k'), /Storage is only available on server-side scenes/)
  await assert.rejects(Storage.player.set('0x1', 'k', 1), /Storage is only available/)
  await assert.rejects(EnvVar.get('X'), /EnvVar is only available/)
})

test('the write queue keeps one op in flight and one queued, latest wins', async () => {
  const q = createWriteQueue()
  const seen = []
  let release
  const gate = new Promise((r) => (release = r))
  const exec = async (body) => {
    seen.push(body)
    await gate
    return true
  }
  const first = q.enqueue('k', 'a', exec, true)
  const second = q.enqueue('k', 'b', exec, true)
  const third = q.enqueue('k', 'c', exec, true)
  assert.equal(q.pending('k'), 'c')
  assert.equal(q.isPending('k'), true)
  release()
  assert.deepEqual(await Promise.all([first, second, third]), [true, true, true])
  assert.deepEqual(seen, ['a', 'c'], 'b was superseded before it was ever sent')
  assert.equal(q.isPending('k'), false)
})
