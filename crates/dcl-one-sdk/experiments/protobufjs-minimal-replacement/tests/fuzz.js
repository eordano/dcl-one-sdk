"use strict";
// Phase 3: hostile-input fuzzing. Random / truncated / corrupted byte strings are fed to
// both Readers (raw op scripts) and to every corpus message decoder. The implementations
// must agree on the value returned AND on which inputs throw (with the same message).

const H = require("./harness");
const REF = H.loadImpl("ref");
const MY = H.loadImpl("mine");

const SEED = Number(process.env.SEED || 0xC0FFEE);
const N_RAW = Number(process.env.N_RAW || 200000);
const N_MSG = Number(process.env.N_MSG || 60);

const rng = new H.Rng(SEED);

const OPS = ["uint32", "int32", "sint32", "int64", "uint64", "sint64", "bool",
    "fixed32", "sfixed32", "fixed64", "sfixed64", "float", "double",
    "bytes", "string", "skip", "skipType"];

function normalize(v) {
    if (v && typeof v === "object" && typeof v.low === "number") return `L(${v.low},${v.high},${!!v.unsigned})`;
    if (v instanceof Uint8Array) return "B(" + Buffer.from(v.buffer, v.byteOffset, v.length).toString("hex") + ")";
    if (typeof v === "number") return Object.is(v, -0) ? "-0" : String(v);
    if (typeof v === "string") return "S" + JSON.stringify(v);
    if (v && v.constructor && /Reader$/.test(v.constructor.name)) return "reader";
    return String(v);
}

function runScript(impl, bytes, script, container) {
    const buf = container === "buffer" ? Buffer.from(bytes) : new Uint8Array(bytes);
    const log = [];
    let r;
    try { r = impl.Reader.create(buf); } catch (e) { return { ctor: String(e.message) }; }
    for (const [op, arg] of script) {
        try {
            const v = op === "skipType" ? r.skipType(arg) : op === "skip" ? r.skip(arg) : r[op]();
            log.push(`${op}=${normalize(v)}@${r.pos}`);
        } catch (e) {
            log.push(`${op}!${e.message}@${r.pos}`);
            break;
        }
    }
    return { log: log.join("|") };
}

let rawChecks = 0, rawFails = 0;
const failures = [];
function fail(s) { rawFails++; if (failures.length < 25) failures.push(s); }

// A pool of "real" encodings we can truncate/corrupt.
const seeds = [];
{
    const w = () => MY.Writer.create();
    for (let i = 0; i < 60; i++) {
        const x = w();
        const n = rng.int(6) + 1;
        for (let k = 0; k < n; k++) {
            const f = rng.int(20) + 1;
            const kind = rng.int(4);
            if (kind === 0) x.uint32((f << 3) | 0).int64(rng.int(1e9) - 5e8);
            else if (kind === 1) x.uint32((f << 3) | 2).string("s".repeat(rng.int(50)));
            else if (kind === 2) x.uint32((f << 3) | 5).float(rng.next() * 1e6);
            else { x.uint32((f << 3) | 2).fork(); x.uint32(8).int32(rng.int(1000)); x.ldelim(); }
        }
        seeds.push(Buffer.from(x.finish()));
    }
}

for (let i = 0; i < N_RAW; i++) {
    let bytes;
    const mode = rng.int(5);
    if (mode === 0) bytes = rng.bytes(rng.int(24));                       // pure random
    else if (mode === 1) bytes = Buffer.alloc(rng.int(12), rng.int(256)); // repeated byte
    else if (mode === 2) {                                               // truncated real encoding
        const s = rng.pick(seeds);
        bytes = s.subarray(0, rng.int(s.length + 1));
    } else if (mode === 3) {                                             // corrupted real encoding
        const s = Buffer.from(rng.pick(seeds));
        for (let k = rng.int(3) + 1; k > 0; k--) if (s.length) s[rng.int(s.length)] = rng.int(256);
        bytes = s;
    } else {                                                             // long varint runs
        const n = rng.int(14);
        bytes = Buffer.from(Array.from({ length: n }, () => (rng.bool(0.8) ? 0x80 : 0) | rng.int(128)));
    }

    const script = [];
    const steps = rng.int(4) + 1;
    for (let k = 0; k < steps; k++) {
        const op = rng.pick(OPS);
        if (op === "skipType") script.push([op, rng.pick([0, 1, 2, 3, 4, 5, 6, 7])]);
        else if (op === "skip") script.push([op, rng.bool() ? rng.int(20) : undefined]);
        else script.push([op, undefined]);
    }

    for (const container of ["buffer", "u8array"]) {
        rawChecks++;
        const a = runScript(REF, bytes, script, container);
        const b = runScript(MY, bytes, script, container);
        if (JSON.stringify(a) !== JSON.stringify(b)) {
            fail(`raw[${container}] bytes=${Buffer.from(bytes).toString("hex")} script=${JSON.stringify(script)}\n     ref : ${a.log || a.ctor}\n     mine: ${b.log || b.ctor}`);
        }
    }
}

/* --------------------------------------------------- message-level hostile decode */

const ref = H.loadCorpus("protobufjs/minimal");
const mine = H.loadCorpus(H.MINE_ID);
const refMsgs = H.collectMessages(ref.mods);
const myIndex = new Map(H.collectMessages(mine.mods).map((m) => [m.rel + "#" + m.name, m]));

let msgChecks = 0, msgFails = 0, msgThrows = 0;
const msgFailures = [];

const allTags = new Set();
for (const m of refMsgs) for (const t of H.mineTags(m.ns)) allTags.add(t);
const ALL = [...allTags];

for (const rm of refMsgs) {
    const mm = myIndex.get(rm.rel + "#" + rm.name);
    if (!mm) continue;
    const tags = H.mineTags(rm.ns).length ? H.mineTags(rm.ns) : ALL;
    for (let i = 0; i < N_MSG; i++) {
        let bytes;
        const mode = i % 3;
        if (mode === 0) bytes = rng.bytes(rng.int(40));
        else if (mode === 1) {
            const good = Buffer.from(H.genMessageBytes(rng, 2, tags));
            bytes = good.subarray(0, rng.int(good.length + 1)); // truncated
        } else {
            const good = Buffer.from(H.genMessageBytes(rng, 2, tags));
            if (good.length) for (let k = rng.int(3) + 1; k > 0; k--) good[rng.int(good.length)] = rng.int(256);
            bytes = good; // corrupted
        }
        for (const container of ["buffer", "u8array"]) {
            msgChecks++;
            const mk = () => (container === "buffer" ? Buffer.from(bytes) : new Uint8Array(bytes));
            let a, b;
            try { a = { ok: true, v: rm.ns.decode(mk()) }; } catch (e) { a = { ok: false, m: String(e.message) }; }
            try { b = { ok: true, v: mm.ns.decode(mk()) }; } catch (e) { b = { ok: false, m: String(e.message) }; }
            if (!a.ok) msgThrows++;
            if (a.ok !== b.ok || (!a.ok && a.m !== b.m)) {
                msgFails++;
                if (msgFailures.length < 15) msgFailures.push(`${rm.rel}#${rm.name}[${container}] ${Buffer.from(bytes).toString("hex")} ref=${a.ok ? "ok" : a.m} mine=${b.ok ? "ok" : b.m}`);
                continue;
            }
            if (a.ok) {
                const d = H.deepEq(a.v, b.v);
                if (d.length) {
                    msgFails++;
                    if (msgFailures.length < 15) msgFailures.push(`${rm.rel}#${rm.name}[${container}] ${Buffer.from(bytes).toString("hex")} diffs=${d.slice(0, 3)}`);
                }
            }
        }
    }
}

console.log("\n=== PHASE 3: hostile-input fuzz ===");
console.log(`seed                        : 0x${SEED.toString(16)}`);
console.log(`raw reader op-script checks : ${rawChecks} (${N_RAW} inputs x 2 containers)`);
console.log(`raw divergences             : ${rawFails}`);
for (const f of failures) console.log("  " + f);
console.log(`message decode checks       : ${msgChecks} (${refMsgs.length} types x ${N_MSG} inputs x 2 containers)`);
console.log(`  of which reference threw  : ${msgThrows}`);
console.log(`message divergences         : ${msgFails}`);
for (const f of msgFailures) console.log("  " + f);
process.exitCode = rawFails + msgFails ? 1 : 0;
