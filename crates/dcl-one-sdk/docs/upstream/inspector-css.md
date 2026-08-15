# `@dcl/inspector`: 66% of `public/bundle.css` is base64 data URIs, and `public/bin/index.js` is a third byte-identical copy of the smart-item runtime

Target repo: `decentraland/creator-hub` (`packages/inspector`; `@dcl/inspector`'s
`package.json` `repository.directory` points there, as does `@dcl/asset-packs`').

Status of this draft: not filed. Every number below was measured on this machine and the
commands to re-derive them are inline. The "Verified vs inferred" section at the end says
exactly which conclusions rest on measurement and which on reasoning we could not close
offline.

---

## Summary

`@dcl/inspector@7.36.3` ships `public/bundle.css` at 5,884,637 B, of which 3,904,602 B
(66.35%) is 246 `url(data:...)` assets — 1.91 MiB of PNG, 1.68 MiB of fonts in four formats
(`eot`/`ttf`/`woff`/`woff2`), 24 payloads duplicated in place. In a full editor session
(load, add entity, edit transform, autosave) the browser activates exactly one of the 246.
Separately, `public/bin/index.js` is 11,421,444 B and byte-identical to both
`@dcl/asset-packs/bin/index.js` and `@dcl/asset-packs/dist/bin/index.js`; it is fetched only
when a host opts in via `binIndexJsUrl`, and our host never does.

---

## Environment

| | |
|---|---|
| `@dcl/inspector` | 7.36.3 (measured); Creator Hub 0.43.0 pins `^7.35.2` |
| `@dcl/asset-packs` | 2.17.2 |
| Creator Hub app | 0.43.0 (`/Applications/Decentraland Creator Hub.app`) |
| Host OS | macOS 26.5.2, arm64 |
| Node | v24.18.0 |
| Browser for the runtime trace | Google Chrome 150.0.7871.187, `--headless=new`, CDP |
| Editor host used for the trace | `dcl-one-sdk` (our Rust re-implementation of `@dcl/sdk-commands`), serving the stock `@dcl/inspector` `public/` tree unmodified |

The shipped desktop app carries the same bytes. `grep -a -c` against
`Decentraland Creator Hub.app/Contents/Resources/app.asar` (279,410,852 B) finds
`vnd.ms-fontobject` (2 hits) and `fonts.googleapis.com/css?family=Lato:400,700,400italic,700italic`
(3 hits), so this is not an artifact of how we serve the package.

---

## Reproduction

No Creator Hub checkout required.

```sh
npm pack @dcl/inspector@7.36.3
tar xzf dcl-inspector-7.36.3.tgz
ls -l package/public/
node measure-bundle-css.mjs package/public/bundle.css
```

`measure-bundle-css.mjs` is next to this file. It parses every `data:` URI out of the
stylesheet, decodes it, and buckets by MIME. Output on `@dcl/inspector@7.36.3`:

```
package/public/bundle.css: 5884637 B
category           n   inline chars   %file      decoded B
image/png          19        2003602   34.05%        1502367
font eot            6         629060   10.69%         471600
font woff2          4         612252   10.40%         459116
font ttf            7         330239    5.61%         247476
font woff           8         190458    3.24%         142608
image/svg+xml     202         138991    2.36%         100660
TOTAL             246        3904602   66.35%        2923827
base64 encoding overhead: 980775 B
legacy font formats (eot+ttf+woff): 1149757 B = 19.54% of file
duplicates: 24 distinct payloads appear more than once, 153 occurrences, 366391 redundant B (6.23% of file)
css with every data: URI removed: 1980035 B
```

For the runtime half, point any Chromium at the inspector with CDP `Network.enable` and
record `Network.requestWillBeSent`. Ours drives the real UI end to end: navigate to
`/inspector/`, wait for the hierarchy, right-click Scene, "Add child", name it, select it,
type into the Transform X field, wait for `assets/scene/main.composite` to hit disk and for
`main.crdt` to be regenerated. Full log kept at `/tmp/nrepro/evidence/network.json`.

---

## Expected vs actual

**Expected.** A stylesheet inlines only assets small enough that a separate request costs
more than the bytes — the usual cutoff is 4–8 KB. Everything above that is emitted as a file
so the browser fetches it on demand, caches it, and can skip it entirely when the rule never
matches. Fonts ship in the one format the runtime supports.

**Actual.** Every asset is inlined regardless of size — the largest single data URI is
535,590 chars — so the entire 5.61 MiB stylesheet must be downloaded, decoded and parsed
before first paint even though almost none of it is used, and nothing is separately
cacheable. Fonts ship in four formats including EOT, which only IE ≤ 11 ever read.

---

## Evidence

### 1. What the bytes are, and where they come from

`bundle.css.map` ships alongside the stylesheet, so each data URI can be attributed to its
input file. Decoding the VLQ mappings and locating each `data:` URI:

| source | n | inline chars | % of bundle.css |
|---|---:|---:|---:|
| `node_modules/decentraland-ui/lib/styles.css` | 132 | 3,412,616 | 57.99% |
| `packages/inspector/src/theme/index.css` | 1 | 469,679 | 7.98% |
| `node_modules/semantic-ui-css/semantic.min.css` | 2 | 9,325 | 0.16% |
| `node_modules/decentraland-ui/lib/dark-theme.css` | 1 | 4,414 | 0.08% |
| `packages/inspector/src/components/AssetsCatalog/Asset/Asset.css` | 2 | 3,300 | 0.06% |
| 34 other files (mostly `decentraland-ui/dist/components/*.css`) | 108 | 5,268 | 0.09% |

Two things follow. First, 58% of the stylesheet enters through one dependency,
`decentraland-ui/lib/styles.css`, which appears to arrive with its assets **already
inlined** — see "Verified vs inferred". Second, the single largest item this repo owns
directly is one 352,240 B variable-axis WOFF2 (`wOF2` magic, 19 tables, `VAR`/`STAT`
present, 882,104 B uncompressed sfnt) inlined from `packages/inspector/src/theme/index.css`,
costing 469,679 chars — 7.98% of the file — for one `@font-face`.

### 2. Four font formats, three of them dead

`eot` + `ttf` + `woff` = 1,149,757 chars, 19.54% of `bundle.css`. The inspector renders
only inside Electron/Chromium, which has supported WOFF2 since Chrome 36. EOT is
Internet Explorer ≤ 11 only and cannot be loaded by any engine the inspector runs on.

The three largest duplicate payloads in the whole file are EOT fonts inlined twice each:
141,382 + 131,562 + 41,586 = 314,530 chars of the 366,391 redundant bytes, all from
`decentraland-ui/lib/styles.css`.

### 3. Almost none of it is used

CDP `Network.requestWillBeSent` for a complete editor session. Chromium raises a request
event for a `data:` URI at the moment the resource is actually decoded and attached, so this
distinguishes "shipped" from "used".

- 246 assets inlined in `bundle.css`.
- **1** activated during the session: the 469,679-char WOFF2 from `src/theme/index.css`.
- 0 of the 221 inlined images activated. 1 of the 25 inlined fonts activated.
- That is 12.0% of the inlined bytes, and it is the one asset that is genuinely needed on
  first paint.

The other 15 `data:` URIs in the trace (14 images, 1 octet-stream) are emitted by
`bundle.js` at runtime; none of their SHA-256s match anything in `bundle.css`.

This is a lower bound on usage, not proof of deadness — Chromium activates a `@font-face`
only when a glyph needs it and a CSS background only when the element renders, so screens we
did not open (login, wearable preview, chain selector, modals) would pull more. That is
precisely the argument for emitting files: with `url(./x.woff2)` the browser fetches on
demand and caches; inlined, all 3.72 MiB is parsed and retained on every launch regardless.

### 4. Requests actually made from the local origin

Of the 22 HTTP requests in the session, exactly four went to the editor's own origin:

```
GET /inspector/            200 text/html
GET /inspector/bundle.css  200 text/css
GET /inspector/bundle.js   200 application/javascript
GET /favicon.ico           404 text/plain
```

`/inspector/bin/index.js` is never requested. The remaining 18 are cross-origin:
`fonts.googleapis.com` (1), `builder-items.decentraland.org` (12: `catalog.json` plus
thumbnails), `assets.babylonjs.com` (3: `backgroundGround.png`, `backgroundSkybox.dds`,
`environmentSpecular.env`), `cdn.decentraland.org` (1: `@dcl/builder-site/6.35.0/static/media/icons.5ff54946.svg`,
the only non-`data:` `url()` left in the stylesheet).

### 5. `public/bin/index.js` — three copies of one file

```
$ shasum -a 256 node_modules/@dcl/asset-packs/bin/index.js \
                node_modules/@dcl/asset-packs/dist/bin/index.js \
                node_modules/@dcl/inspector/public/bin/index.js
5c5b05bd065217a3addc8b2d47cbb5a11511aed548dcb83b580a7146dc86214b  .../asset-packs/bin/index.js
5c5b05bd065217a3addc8b2d47cbb5a11511aed548dcb83b580a7146dc86214b  .../asset-packs/dist/bin/index.js
5c5b05bd065217a3addc8b2d47cbb5a11511aed548dcb83b580a7146dc86214b  .../inspector/public/bin/index.js
```

11,421,444 B each, 34,264,332 B for the set. The `@dcl/inspector` copy is produced by the
package's own `copy-bin` script — visible in the published `package.json`:

```json
"build": "npm run copy-bin && node ./build.js --production",
"copy-bin": "ts-node --project ./scripts/tsconfig.json ./scripts/copy-bin.ts"
```

It is only ever fetched when a host opts in. From `public/bundle.js` (identifiers minified;
this is the whole function):

```js
async function gn0(r) {
  let t = _8()
  if (!t.binIndexJsUrl) return
  console.log('Installing binaries')
  let e = await fetch(t.binIndexJsUrl).then((a) => a.text())
  await r.writeFile('bin/index.js', Buffer.from(e))
}
```

and `binIndexJsUrl` is `?binIndexJsUrl=` ?? `globalThis.InspectorConfig.binIndexJsUrl` ??
`null`. Our host never sets it, which matches the network log above. Since `binIndexJsUrl`
is an absolute URL the host chooses, the copy inside the `@dcl/inspector` tarball is only
load-bearing for hosts that point it at their own static mount of `public/`.

### 6. Source maps ship to every consumer

Not in the original scope, but it dwarfs everything else and is one line to fix. The
package's `files` field is `["dist", "public"]`, so:

| file | bytes | MiB |
|---|---:|---:|
| `public/bundle.js.map` | 81,635,004 | 77.85 |
| `public/bundle.js` | 18,917,777 | 18.04 |
| `public/bin/index.js` | 11,421,444 | 10.89 |
| `public/bundle.css` | 5,884,637 | 5.61 |
| `public/bundle.css.map` | 2,808,779 | 2.68 |
| `dist/` (all, incl. `dist/tooling-entrypoint.js.map`) | 4,382,057 | 4.18 |
| **package total** | **125,062,573** | **119.27** |

The two `public/` maps are 84,443,783 B (80.53 MiB); counting
`dist/tooling-entrypoint.js.map` as well, source maps are 85,927,010 B — 81.95 MiB, **68.7%
of the unpacked package**. `bundle.css` references `bundle.css.map` via a `sourceMappingURL`
comment, so browsers only pull it when devtools are open — but it is on every user's disk
and inside every `app.asar`.

---

## Proposed fix

We do not have a `creator-hub` checkout, so this is an issue rather than a PR and the
snippets below are suggestions, not a tested diff. `packages/inspector/build.js` is not
published to npm, so we could not read the current bundler configuration.

The bundler is **esbuild** (`"esbuild": "^0.28.0"` in `devDependencies`, driven by
`node ./build.js --production`). Note that this is esbuild's `loader` map, not webpack's
`asset/resource` vs `asset/inline` — if you have seen this reported in webpack terms, it
does not apply here.

**1. Stop inlining assets above a threshold.** esbuild has no built-in size cutoff, so it is
either a per-extension loader flip or a small plugin. The minimal version, which fixes the
352 KB variable font this repo owns directly:

```js
// packages/inspector/build.js
 loader: {
-  '.woff2': 'dataurl',
-  '.woff': 'dataurl',
-  '.ttf': 'dataurl',
-  '.png': 'dataurl',
+  '.woff2': 'file',
+  '.woff': 'file',
+  '.ttf': 'file',
+  '.png': 'file',
   '.svg': 'dataurl',   // keep: 202 SVGs, 138,991 B total, median 46 B, 149 under 1 KB
 },
+assetNames: 'assets/[name]-[hash]',
+publicPath: '.',
```

Keeping `.svg` inline is deliberate: all 202 of them together are 2.36% of the file, the
median is 46 B and the largest is 7,038 B, so they are below any sane threshold. If you want a true byte
cutoff instead of a per-extension rule, an `onLoad` plugin that returns `contents` for small
files and `{ loader: 'file' }` for large ones is about fifteen lines.

Whatever the mechanism, the host must serve the emitted `assets/` directory next to
`bundle.css`. That is a real compatibility consideration for third-party hosts of `public/`
(ours included) and worth a line in the release notes.

**2. Drop the legacy font formats.** 1,149,757 B, 19.54% of the stylesheet, unreachable in
Chromium. Most of this is inside `decentraland-ui`, so the fix likely belongs there — see
item 5.

**3. Remove the Google Fonts `@import`.** `bundle.css` opens with:

```css
@import"https://fonts.googleapis.com/css?family=Lato:400,700,400italic,700italic&subset=latin";
```

A render-blocking cross-origin request, from a desktop application, in a stylesheet that
already inlines 1.68 MiB of fonts. It fires on every editor launch (confirmed in the network
trace) and is present in the shipped `app.asar`. Self-hosting Lato — or dropping it if the
inlined faces already cover it — removes a startup network dependency and makes the editor
work offline.

**4. Do not ship `public/bin/index.js`.** It is byte-identical to
`@dcl/asset-packs/bin/index.js`, which is already a dependency and resolvable from the same
tree. Either drop `copy-bin` and have hosts point `binIndexJsUrl` at the `@dcl/asset-packs`
copy, or keep the copy step for local dev and add `!public/bin` to the published `files`.
Worth checking whether `@dcl/asset-packs` needs both `bin/index.js` and `dist/bin/index.js`
while you are there — that is another 11.4 MB.

**5. Upstream in `decentraland-ui`.** The 58% that arrives pre-inlined through
`lib/styles.css` cannot be fixed by any esbuild setting in this repo, because esbuild sees
`url(data:...)` tokens it can only pass through. That change has to happen in
`decentraland-ui`'s own build.

**6. `"files": ["dist", "public", "!**/*.map"]`.** 81.95 MiB, 68.7% of the unpacked package.
(`!public/*.map` alone would leave `dist/tooling-entrypoint.js.map` behind.)

### Expected effect

Items 1 and 2 alone, assuming a ~45 B `url()` per externalised asset:

| | before | after |
|---|---:|---:|
| `bundle.css` (parsed on every launch) | 5,884,637 B | ~1,991,000 B |
| assets on disk, fetched on demand | 0 | ~2,062,000 B (2,923,827 B minus the 861,684 B of eot/ttf/woff) |
| total on disk | 5,884,637 B | ~4,053,000 B |

The stylesheet the browser must parse before first paint drops by 66%; ~980 KB of pure
base64 encoding overhead disappears; and the remaining assets become individually cacheable
and lazily fetched instead of unconditionally resident. This is every Creator Hub launch,
for every user.

---

## Verified vs inferred

**Verified by measurement on this machine:**

- Every byte count, percentage, MIME breakdown, duplicate count and SHA-256 above.
- The source attribution table (decoded from the shipped `bundle.css.map`).
- That exactly one of the 246 inlined assets is activated during a full add-entity /
  edit-transform / autosave session, via CDP.
- That `/inspector/bin/index.js` is never requested by that session.
- That the three `bin/index.js` copies are byte-identical.
- That the shipped Creator Hub 0.43.0 `app.asar` contains the same EOT data URIs and the
  same Google Fonts `@import`.
- That the bundler is esbuild `^0.28.0`, from the published `devDependencies` and `scripts`.

**Inferred, not confirmed:**

- **That `decentraland-ui/lib/styles.css` arrives with assets already inlined.** It is a
  pruned devDependency here and we did not install it. The evidence is that a single source
  file yields both modern MIME strings (`font/woff2`, `font/ttf`) and legacy ones
  (`application/font-woff`, `application/x-font-ttf`); esbuild derives the data-URL MIME
  from the file extension, so a single generator would be self-consistent, and
  `semantic-ui-css/semantic.min.css` — a published, minified artifact nobody's build step
  rewrote — contributes the same legacy strings. One command settles it:
  `grep -c 'url(data:' node_modules/decentraland-ui/lib/styles.css`.
- **That esbuild never emits `application/font-woff` / `application/x-font-ttf`.** We could
  not read esbuild's MIME table offline. If it does, the argument above weakens and more of
  the inlining is fixable in this repo, which would be the better outcome.
- **That Creator Hub's own Electron host does not set `binIndexJsUrl`.** We verified this
  for our host only. If Creator Hub does set it, `public/bin/index.js` is load-bearing there
  and fix 4 must be limited to the `!public/bin` variant, or to pointing the URL at the
  `@dcl/asset-packs` copy.
- **That the 245 unactivated assets are largely dead.** They are unused in the session we
  drove; wider UI coverage would activate more. The externalisation argument does not depend
  on this and holds either way.
- The "expected effect" table is arithmetic on the measured numbers, not a rebuild. We did
  not build the inspector.

**Not investigated:** whether `bundle.js` (18.04 MiB) has the same problem. It contains 14
runtime `data:` image URIs and a Babylon.js dependency tree; nobody measured it.
