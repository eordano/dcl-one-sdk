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

const ref = H.loadCorpus("protobufjs/minimal");
const mine = H.loadCorpus(H.MINE_ID);

console.log(`corpus modules loaded: ref=${ref.mods.size} mine=${mine.mods.size}`);
if (ref.failures.length || mine.failures.length) {
    console.log("load failures ref:", ref.failures);
    console.log("load failures mine:", mine.failures);
}

const refMsgs = H.collectMessages(ref.mods);
const myMsgs = H.collectMessages(mine.mods);

console.log(`message namespaces (encode+decode): ref=${refMsgs.length} mine=${myMsgs.length}`);

const st = H.roundTrip(refMsgs, myMsgs, { seed: SEED, iters: ITERS });
process.exitCode = H.reportRoundTrip("PHASE 1: corpus differential round-trip", SEED, ITERS, st);
