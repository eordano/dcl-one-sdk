// Phase 1b: same differential round-trip, but against @dcl/ecs's ESM build (dist/), which is
// the tree a scene bundler actually consumes. Two independent module graphs are obtained by
// tagging every corpus URL with ?__impl=ref|mine, so each graph binds its own implementation.

import { registerHooks } from "node:module";
import { pathToFileURL, fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const H = require("./harness.js");

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const ESM_CORPUS = path.join(ROOT, "ecs", "dist");
const IMPL_URL = {
    ref: pathToFileURL(path.join(ROOT, "ref/node_modules/protobufjs/minimal.js")).href,
    mine: pathToFileURL(path.join(ROOT, (process.env.MINE_ID || "pbmin") === "pbmin" ? "pbmin/index.js" : "mutant/index.js")).href,
};

function implOf(parentURL) {
    if (!parentURL) return null;
    const m = /[?&]__impl=(ref|mine)/.exec(parentURL);
    return m ? m[1] : null;
}

registerHooks({
    resolve(specifier, context, nextResolve) {
        const impl = implOf(context.parentURL);
        if (specifier === "protobufjs/minimal") {
            if (!impl) return nextResolve(specifier, context);
            // No query here on purpose: a query string would make Node instantiate a SECOND
            // copy of the CJS module graph, and protobufjs's internal require cycle then sees
            // a half-initialised util (symptom: "utf8.write is not a function"). ref and mine
            // are different files, so the plain URLs already keep the graphs apart.
            return { url: IMPL_URL[impl], shortCircuit: true, format: "commonjs" };
        }
        // extensionless relative imports inside the corpus
        if (impl && /^\.{1,2}\//.test(specifier)) {
            const base = new URL(specifier, context.parentURL);
            let p = fileURLToPath(base.href.split("?")[0]);
            const isFile = (x) => { try { return fs.statSync(x).isFile(); } catch { return false; } };
            if (!isFile(p)) {
                if (isFile(p + ".js")) p += ".js";
                else if (isFile(path.join(p, "index.js"))) p = path.join(p, "index.js");
            }
            return { url: pathToFileURL(p).href + "?__impl=" + impl, shortCircuit: true, format: "module" };
        }
        return nextResolve(specifier, context);
    },
});

function listEsm() {
    const out = [];
    (function walk(d) {
        for (const e of fs.readdirSync(d, { withFileTypes: true })) {
            const p = path.join(d, e.name);
            if (e.isDirectory()) walk(p);
            else if (e.name.endsWith(".gen.js")) out.push(p);
        }
    })(ESM_CORPUS);
    return out.sort();
}

async function loadEsm(impl) {
    const mods = new Map();
    const failures = [];
    for (const f of listEsm()) {
        try {
            mods.set(path.relative(ESM_CORPUS, f), await import(pathToFileURL(f).href + "?__impl=" + impl));
        } catch (e) { failures.push([path.relative(ESM_CORPUS, f), e.message]); }
    }
    return { mods, failures };
}

const SEED = Number(process.env.SEED || 0xC0FFEE);
const ITERS = Number(process.env.ITERS || 100);

const ref = await loadEsm("ref");
const mine = await loadEsm("mine");
console.log(`esm modules loaded   : ref=${ref.mods.size} mine=${mine.mods.size}` +
    (ref.failures.length || mine.failures.length ? ` (failures ref=${ref.failures.length} mine=${mine.failures.length})` : ""));
// global.gen.js is not a schema module - it re-exports index.gen.js and additionally pulls in
// the @dcl/ecs engine runtime, which is not part of the extracted corpus. It fails identically
// for both implementations and contributes no encode/decode of its own.
if (ref.failures.length) console.log("  (expected, impl-independent):", ref.failures.map((f) => f[0] + ": " + f[1]).join("; "));

const refMsgs = H.collectMessages(ref.mods);
const myIndex = new Map(H.collectMessages(mine.mods).map((m) => [m.rel + "#" + m.name, m]));

// prove the two graphs really are bound to different implementations
{
    const refImpl = require(path.join(ROOT, "ref/node_modules/protobufjs/minimal.js"));
    const myImpl = require(path.join(ROOT, "pbmin/index.js"));
    let hits = { ref: 0, mine: 0, unknown: 0 };
    const ro = refImpl.Reader.create, mo = myImpl.Reader.create;
    for (const [mods, want] of [[ref.mods, "ref"], [mine.mods, "mine"]]) {
        for (const [, mod] of mods) {
            const ns = Object.values(mod).find((x) => x && typeof x.decode === "function");
            if (!ns) continue;
            let hit = null;
            refImpl.Reader.create = function (b) { hit = "ref"; return ro.call(this, b); };
            myImpl.Reader.create = function (b) { hit = "mine"; return mo.call(this, b); };
            try { ns.decode(new Uint8Array(0)); } catch { /* ignore */ }
            refImpl.Reader.create = ro; myImpl.Reader.create = mo;
            hits[hit === want ? want : "unknown"]++;
        }
    }
    console.log(`esm graph binding    : ref-graph->ref + mine-graph->mine = ${hits.ref + hits.mine}, mismatched = ${hits.unknown}`);
    if (hits.unknown) { console.log("ESM ISOLATION BROKEN"); process.exit(1); }
}

const allTags = new Set();
for (const m of refMsgs) for (const t of H.mineTags(m.ns)) allTags.add(t);
const ALL = [...allTags];

let covered = 0, instances = 0, diverge = 0;
const failures = [];
function hashStr(s) { let h = 2166136261; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); } return h >>> 0; }
const call = (f) => { try { return { ok: true, v: f() }; } catch (e) { return { ok: false, e }; } };

for (const rm of refMsgs) {
    const mm = myIndex.get(rm.rel + "#" + rm.name);
    if (!mm) continue;
    let tags = H.mineTags(rm.ns); if (!tags.length) tags = ALL;
    const rng = new H.Rng((SEED ^ hashStr(rm.rel + "#" + rm.name)) >>> 0);
    for (let i = 0; i < ITERS; i++) {
        const raw = Buffer.from(H.genMessageBytes(rng, 2, tags));
        const input = i % 2 === 0 ? () => Buffer.from(raw) : () => new Uint8Array(raw);
        instances++;
        const a = call(() => rm.ns.decode(input())), b = call(() => mm.ns.decode(input()));
        if (a.ok !== b.ok || (!a.ok && a.e.message !== b.e.message)) {
            diverge++; if (failures.length < 5) failures.push(`${rm.rel}#${rm.name} throw ${raw.toString("hex")}`); continue;
        }
        if (!a.ok) continue;
        const d = H.deepEq(a.v, b.v);
        if (d.length) { diverge++; if (failures.length < 5) failures.push(`${rm.rel}#${rm.name} decode ${raw.toString("hex")} ${d.slice(0, 3)}`); continue; }
        const ea = call(() => rm.ns.encode(a.v).finish()), eb = call(() => mm.ns.encode(b.v).finish());
        const ex = call(() => mm.ns.encode(a.v).finish());
        if (ea.ok !== eb.ok || ea.ok !== ex.ok) { diverge++; continue; }
        if (ea.ok && (!H.bytesEq(ea.v, eb.v) || !H.bytesEq(ea.v, ex.v))) {
            diverge++; if (failures.length < 5) failures.push(`${rm.rel}#${rm.name} encode ${raw.toString("hex")}`);
        }
    }
    covered++;
}

console.log("\n=== PHASE 1b: ESM (dist/) corpus differential ===");
console.log(`seed                 : 0x${SEED.toString(16)}`);
console.log(`message types covered: ${covered} / ${refMsgs.length}`);
console.log(`unique message names : ${new Set(refMsgs.map((m) => m.name)).size}`);
console.log(`instances tested     : ${instances}`);
console.log(`divergences          : ${diverge}`);
for (const f of failures) console.log("  " + f);
process.exitCode = diverge ? 1 : 0;
