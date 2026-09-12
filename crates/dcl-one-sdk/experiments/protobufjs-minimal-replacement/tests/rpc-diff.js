"use strict";

const H = require("./harness");

const SEED = Number(process.env.SEED || 0xC0FFEE);
const ITERS = Number(process.env.ITERS || 200);

{
    const impls = [H.loadImpl("ref"), H.loadImpl("mine")];
    const notes = [];
    for (const impl of impls) {
        if (process.env.NO_BUFFER === "1") impl.util.Buffer = null;
        if (process.env.NO_LONG === "1") impl.util.Long = null;
        impl.configure();
    }
    if (process.env.NO_BUFFER === "1") notes.push("no node Buffer");
    if (process.env.NO_LONG === "1") notes.push("util.Long initially unset");
    console.log(`environment          : ${notes.length ? notes.join(", ") + " (scene-runtime emulation)" : "node defaults (Buffer + resolvable long)"}`);
}

const ref = H.loadRpcCorpus("protobufjs/minimal");
const mine = H.loadRpcCorpus(H.MINE_ID);

console.log(`corpus modules loaded: ref=${ref.mods.size} mine=${mine.mods.size}`);
if (ref.failures.length || mine.failures.length) {
    console.log("load failures ref:", ref.failures);
    console.log("load failures mine:", mine.failures);
    process.exitCode = 1;
}
if (ref.mods.size !== 2) {
    console.log(`FATAL: expected 2 catalogue modules, found ${ref.mods.size}`);
    process.exitCode = 1;
}

const refMsgs = H.collectMessages(ref.mods);
const myMsgs = H.collectMessages(mine.mods);

console.log(`message namespaces (encode+decode): ref=${refMsgs.length} mine=${myMsgs.length}`);
for (const [rel] of ref.mods) {
    const n = refMsgs.filter((m) => m.rel === rel).length;
    console.log(`  ${rel}: ${n} namespaces`);
}
if (refMsgs.length === 0) {
    console.log("FATAL: no message namespaces found");
    process.exitCode = 1;
}

const dl = ref.mods.get("datalayer/data-layer.gen.js");
if (dl && dl.DataServiceDefinition) {
    const covered = new Set(refMsgs.map((m) => m.ns));
    const methods = Object.entries(dl.DataServiceDefinition.methods);
    const missing = [];
    for (const [name, def] of methods) {
        if (!covered.has(def.requestType)) missing.push(`${name}.request`);
        if (!covered.has(def.responseType)) missing.push(`${name}.response`);
    }
    console.log(`descriptor methods   : ${methods.length} (expect 22), request/response types uncovered: ${missing.length}`);
    if (methods.length !== 22 || missing.length) {
        console.log("  uncovered:", missing);
        process.exitCode = 1;
    }
}

const st = H.roundTrip(refMsgs, myMsgs, { seed: SEED, iters: ITERS });
const rc = H.reportRoundTrip("PHASE 1c: @dcl/rpc + data-layer descriptor round-trip", SEED, ITERS, st);
if (rc) process.exitCode = rc;
