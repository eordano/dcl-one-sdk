// Usage: node measure.mjs path/to/@dcl/inspector/public/bundle.css
import fs from 'node:fs'
import crypto from 'node:crypto'

const file = process.argv[2]
const css = fs.readFileSync(file, 'utf8')
const total = Buffer.byteLength(css)
const re = /data:([-\w.+/]+)?(;charset=[-\w]+)?(;base64)?,([^)"']*)/g

const CAT = {
  'application/vnd.ms-fontobject': 'font eot',
  'font/woff2': 'font woff2',
  'font/ttf': 'font ttf',
  'font/woff': 'font woff',
  'application/font-woff': 'font woff',
  'application/x-font-ttf': 'font ttf'
}

const rows = []
for (const m of css.matchAll(re)) {
  const mime = (m[1] || '').toLowerCase()
  const bytes = m[3] ? Buffer.from(m[4], 'base64') : Buffer.from(m[4], 'utf8')
  rows.push({
    cat: CAT[mime] || mime,
    inline: m[0].length,
    decoded: bytes.length,
    sha: crypto.createHash('sha256').update(bytes).digest('hex')
  })
}

const by = new Map()
for (const r of rows) {
  const e = by.get(r.cat) || { n: 0, inline: 0, decoded: 0 }
  e.n++; e.inline += r.inline; e.decoded += r.decoded
  by.set(r.cat, e)
}

const sum = (k) => rows.reduce((a, r) => a + r[k], 0)
console.log(`${file}: ${total} B`)
console.log('category           n   inline chars   %file      decoded B')
for (const [c, e] of [...by].sort((a, b) => b[1].inline - a[1].inline)) {
  console.log(
    `${c.padEnd(16)} ${String(e.n).padStart(4)} ${String(e.inline).padStart(14)} ` +
      `${((100 * e.inline) / total).toFixed(2).padStart(7)}% ${String(e.decoded).padStart(14)}`
  )
}
console.log(
  `${'TOTAL'.padEnd(16)} ${String(rows.length).padStart(4)} ${String(sum('inline')).padStart(14)} ` +
    `${((100 * sum('inline')) / total).toFixed(2).padStart(7)}% ${String(sum('decoded')).padStart(14)}`
)
console.log(`base64 encoding overhead: ${sum('inline') - sum('decoded')} B`)

const legacy = ['font eot', 'font ttf', 'font woff']
  .map((c) => by.get(c)?.inline || 0)
  .reduce((a, b) => a + b, 0)
console.log(`legacy font formats (eot+ttf+woff): ${legacy} B = ${((100 * legacy) / total).toFixed(2)}% of file`)

const seen = new Map()
for (const r of rows) seen.set(r.sha, (seen.get(r.sha) || 0) + 1)
const dupShas = [...seen].filter(([, n]) => n > 1)
let redundant = 0
const first = new Set()
for (const r of rows) {
  if (seen.get(r.sha) > 1) {
    if (first.has(r.sha)) redundant += r.inline
    else first.add(r.sha)
  }
}
console.log(
  `duplicates: ${dupShas.length} distinct payloads appear more than once, ` +
    `${dupShas.reduce((a, [, n]) => a + n, 0)} occurrences, ${redundant} redundant B ` +
    `(${((100 * redundant) / total).toFixed(2)}% of file)`
)
console.log(`css with every data: URI removed: ${total - sum('inline')} B`)
