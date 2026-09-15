// dcl-one-sdk host storage client -- the `@dcl/sdk/server` surface the host
// isolate hands a scene: Storage (scene + player scopes) and EnvVar, ported
// from js-sdk-toolchain's auth-server branch (packages/@dcl/sdk/src/server).
// The preview server is the storage service: every call is one HTTP request
// to the routes it serves under the realm base URL, so what a scene observes
// here is what it observes in production -- a read cache with a short TTL
// and negative entries, in-flight reads coalesced per key, writes serialized
// per key with rapid ones collapsed to the latest value, unchanged writes
// skipped, and boolean results that never throw on a network failure.
'use strict'

export const MODULE_NAME = 'Storage'
export const DEFAULT_STORAGE_CONFIG = Object.freeze({
  skipIfUnchanged: true,
  cacheReads: true,
  cacheMaxEntries: 512,
  cacheMaxAgeMs: 60 * 1000
})
/** Every request names its writer so the preview's activity view can tell a scene from the CLI or the page. */
export const SOURCE_HEADER = 'x-dcl-one-storage-source'

export function createStorageConfig(overrides) {
  return { ...DEFAULT_STORAGE_CONFIG, ...(overrides ?? {}) }
}

// Bounded, lazily-expiring cache of a key's last confirmed server-side state:
// a serialized `{ value }` body, or a confirmed absence. Insertion order is
// refreshed on every store so eviction drops the least recently written.
export function createValueCache(config) {
  const entries = new Map()
  function insert(key, entry) {
    entries.delete(key)
    entries.set(key, { ...entry, storedAt: Date.now() })
    const maxEntries = Number.isFinite(config.cacheMaxEntries)
      ? Math.max(0, config.cacheMaxEntries)
      : DEFAULT_STORAGE_CONFIG.cacheMaxEntries
    while (entries.size > maxEntries) entries.delete(entries.keys().next().value)
  }
  return {
    get(key) {
      const entry = entries.get(key)
      if (!entry) return undefined
      const maxAgeMs = Number.isFinite(config.cacheMaxAgeMs)
        ? config.cacheMaxAgeMs
        : DEFAULT_STORAGE_CONFIG.cacheMaxAgeMs
      if (Date.now() - entry.storedAt > maxAgeMs) {
        entries.delete(key)
        return undefined
      }
      return entry
    },
    set(key, entry) {
      insert(key, entry)
    },
    setAbsent(key) {
      insert(key, { absent: true })
    },
    delete(key) {
      entries.delete(key)
    },
    get size() {
      return entries.size
    }
  }
}

// Per-key write serializer: one op on the network, at most one queued behind
// it whose payload later writes replace. N rapid writes cost at most two
// requests and the server's final state is the last write issued.
export function createWriteQueue() {
  const keys = new Map()
  function makeOp(body, execute) {
    let resolve
    const promise = new Promise((r) => (resolve = r))
    return { body, execute, promise, resolve }
  }
  async function drain(key, state) {
    for (;;) {
      const op = state.active
      let result = false
      try {
        result = await op.execute(op.body)
      } catch {
        // executors report failure through their boolean; a throw must not wedge the queue
      }
      op.resolve(result)
      if (state.queued) {
        state.active = state.queued
        state.queued = undefined
      } else {
        keys.delete(key)
        return
      }
    }
  }
  return {
    pending(key) {
      const state = keys.get(key)
      if (!state) return undefined
      return (state.queued ?? state.active).body
    },
    isPending(key) {
      return keys.has(key)
    },
    enqueue(key, body, execute, joinActive) {
      const state = keys.get(key)
      if (!state) {
        const op = makeOp(body, execute)
        const fresh = { active: op }
        keys.set(key, fresh)
        void drain(key, fresh)
        return op.promise
      }
      if (state.queued) {
        if (state.queued.body !== body) {
          state.queued.body = body
          state.queued.execute = execute
        }
        return state.queued.promise
      }
      if (joinActive && state.active.body === body) return state.active.promise
      const op = makeOp(body, execute)
      state.queued = op
      return op.promise
    }
  }
}

// [error, data, status]: the shape upstream's wrapSignedFetch returns, so the
// scopes below read as the upstream source does.
function createFetchJson(fetchImpl, source) {
  return async function fetchJson(url, init) {
    const headers = { ...(init?.headers ?? {}), [SOURCE_HEADER]: source }
    let response
    try {
      response = await fetchImpl(url, { ...init, headers })
    } catch (error) {
      console.error(`Error in ${url} endpoint`, { error })
      return [error?.message ?? String(error), null, undefined]
    }
    if (!response.ok) {
      // 404 is a first-class outcome for storage/env lookups (key not created
      // yet), not a failure: every caller branches on the status.
      if (response.status !== 404) console.error(`Error in ${url} endpoint`, { status: response.status })
      return [`${response.status} ${response.statusText}`, null, response.status]
    }
    let body
    try {
      const text = await response.text()
      body = text ? JSON.parse(text) : {}
    } catch {
      console.error(`Failed to parse response from ${url}`)
      return ['Failed to parse response', null, response.status]
    }
    return [null, body ?? {}, response.status]
  }
}

// One key-value scope. `naming` turns the scope's identifiers (a key, or an
// address and a key) into the request URL, the cache key and a label for
// error lines; everything else is shared between scene and player storage.
function createScope(config, fetchJson, naming) {
  const cache = createValueCache(config)
  // Each in-flight GET is a wrapper whose identity marks ownership: a
  // set()/delete() drops it, so a stale response cannot overwrite the newer
  // cache entry.
  const inflightGets = new Map()
  const writes = createWriteQueue()

  async function executeSet(ids, ck, body) {
    const [error] = await fetchJson(naming.url(ids), {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body
    })
    inflightGets.delete(ck)
    if (error) {
      cache.delete(ck)
      console.error(`Failed to set ${naming.label(ids)}: ${error}`)
      return false
    }
    cache.set(ck, { body })
    return true
  }

  async function executeDelete(ids, ck) {
    const [error, , status] = await fetchJson(naming.url(ids), { method: 'DELETE', headers: {} })
    inflightGets.delete(ck)
    if (error) {
      if (status === 404) cache.setAbsent(ck)
      console.error(`Failed to delete ${naming.label(ids)}: ${error}`)
      return false
    }
    cache.setAbsent(ck)
    return true
  }

  return {
    async get(ids, options) {
      const ck = naming.cacheKey(ids)
      if (config.cacheReads && !options?.fresh) {
        const entry = cache.get(ck)
        if (entry?.absent) return null
        if (entry?.body !== undefined) return JSON.parse(entry.body).value
      }
      const joined = inflightGets.get(ck)
      if (joined) return joined.promise
      const inflight = {}
      inflight.promise = (async () => {
        try {
          const [error, data, status] = await fetchJson(naming.url(ids))
          const isOwner = inflightGets.get(ck) === inflight
          if (error) {
            if (status === 404) {
              if (isOwner) cache.setAbsent(ck)
              return null
            }
            console.error(`Failed to get ${naming.label(ids)}: ${error}`)
            return null
          }
          if (data && data.value !== undefined) {
            const body = JSON.stringify({ value: data.value })
            if (isOwner) cache.set(ck, { body })
            return data.value
          }
          return null
        } finally {
          if (inflightGets.get(ck) === inflight) inflightGets.delete(ck)
        }
      })()
      inflightGets.set(ck, inflight)
      return inflight.promise
    },

    async set(ids, value, options) {
      const ck = naming.cacheKey(ids)
      const body = JSON.stringify({ value })
      const skipIfUnchanged = options?.skipIfUnchanged ?? config.skipIfUnchanged
      if (skipIfUnchanged && writes.pending(ck) === undefined && cache.get(ck)?.body === body) return true
      return writes.enqueue(ck, body, (b) => executeSet(ids, ck, b), skipIfUnchanged)
    },

    async delete(ids) {
      const ck = naming.cacheKey(ids)
      cache.delete(ck)
      inflightGets.delete(ck)
      return writes.enqueue(ck, null, () => executeDelete(ids, ck), true)
    },

    async getValues(ids, options) {
      const { prefix, limit, offset } = options ?? {}
      const parts = []
      if (prefix) parts.push(`prefix=${enc(prefix)}`)
      if (limit) parts.push(`limit=${limit}`)
      if (offset) parts.push(`offset=${offset}`)
      const query = parts.join('&')
      const [error, response] = await fetchJson(query ? `${naming.listUrl(ids)}?${query}` : naming.listUrl(ids))
      if (error) {
        console.error(`Failed to get ${naming.listLabel(ids)}: ${error}`)
        return { data: [], pagination: { offset: 0, total: 0 } }
      }
      const data = response?.data ?? []
      // Seed the per-key cache so subsequent get()/set() on returned keys can
      // skip the network. Only keys with no live entry and no pending write
      // are seeded: existing per-key state comes from a confirmed operation
      // that this page snapshot must not clobber. Absence is never seeded.
      for (const entry of data) {
        const ck = naming.cacheKey({ ...ids, key: entry.key })
        if (entry.value !== undefined && !writes.isPending(ck) && cache.get(ck) === undefined) {
          cache.set(ck, { body: JSON.stringify({ value: entry.value }) })
        }
      }
      const requestedOffset = offset ?? 0
      return {
        data,
        pagination: {
          offset: response?.pagination?.offset ?? requestedOffset,
          total: response?.pagination?.total ?? data.length
        }
      }
    },

    cache
  }
}

const enc = encodeURIComponent
// Addresses are case-insensitive and NUL never appears in one, so the pair
// `address NUL key` is an unambiguous cache key.
const NUL = String.fromCharCode(0)

/**
 * The module a scene imports as `@dcl/sdk/server`, bound to the preview
 * serving the storage routes at `baseUrl`. `fetch` and `isServer` are
 * injectable for tests; the host passes neither.
 */
export function createServerModule({ baseUrl, fetch: fetchImpl, source = 'scene', isServer = () => true, config } = {}) {
  if (!baseUrl) throw new Error('createServerModule needs the preview base URL')
  const base = String(baseUrl).replace(/\/+$/, '')
  const fetchJson = createFetchJson(fetchImpl ?? globalThis.fetch, source)
  const resolved = createStorageConfig(config)

  const assertIsServer = (moduleName) => {
    if (!isServer()) throw new Error(`${moduleName} is only available on server-side scenes`)
  }

  const scene = createScope(resolved, fetchJson, {
    url: ({ key }) => `${base}/values/${enc(key)}`,
    listUrl: () => `${base}/values`,
    cacheKey: ({ key }) => key,
    label: ({ key }) => `storage value '${key}'`,
    listLabel: () => 'storage values'
  })
  const player = createScope(resolved, fetchJson, {
    url: ({ address, key }) => `${base}/players/${enc(address)}/values/${enc(key)}`,
    listUrl: ({ address }) => `${base}/players/${enc(address)}/values`,
    cacheKey: ({ address, key }) => `${String(address).toLowerCase()}${NUL}${key}`,
    label: ({ address, key }) => `player storage value '${key}' for '${address}'`,
    listLabel: ({ address }) => `player storage values for '${address}'`
  })

  // async like upstream's: a call outside a server isolate rejects
  const Storage = {
    async get(key, options) {
      assertIsServer(MODULE_NAME)
      return scene.get({ key }, options)
    },
    async set(key, value, options) {
      assertIsServer(MODULE_NAME)
      return scene.set({ key }, value, options)
    },
    async delete(key) {
      assertIsServer(MODULE_NAME)
      return scene.delete({ key })
    },
    async getValues(options) {
      assertIsServer(MODULE_NAME)
      return scene.getValues({}, options)
    },
    player: {
      async get(address, key, options) {
        assertIsServer(MODULE_NAME)
        return player.get({ address, key }, options)
      },
      async set(address, key, value, options) {
        assertIsServer(MODULE_NAME)
        return player.set({ address, key }, value, options)
      },
      async delete(address, key) {
        assertIsServer(MODULE_NAME)
        return player.delete({ address, key })
      },
      async getValues(address, options) {
        assertIsServer(MODULE_NAME)
        return player.getValues({ address }, options)
      }
    },
    configure(options) {
      for (const [key, value] of Object.entries(options ?? {})) {
        if (value !== undefined) resolved[key] = value
      }
    }
  }

  const EnvVar = {
    async get(key) {
      assertIsServer('EnvVar')
      const [error, data] = await fetchJson(`${base}/env/${enc(key)}`)
      if (error) {
        console.error(`Failed to fetch environment variable '${key}': ${error}`)
        return ''
      }
      return data?.value ?? ''
    }
  }

  return { Storage, EnvVar }
}
