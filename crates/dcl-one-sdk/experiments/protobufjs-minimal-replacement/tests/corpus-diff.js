"use strict";
// Phase 1: differential round-trip of every message type in the @dcl/ecs corpus.

const H = require("./harness");

const SEED = Number(process.env.SEED || 0xC0FFEE);
const ITERS = Number(process.env.ITERS || 200);

// Environment emulation must happen BEFORE the corpus is loaded, because the two ts-proto
// modules that use int64 run `if (_m0.util.Long !== Long) { _m0.util.Long = Long; _m0.configure(); }`
// at module-evaluation time - the branch we most need to exercise.
//   NO_BUFFER=1 : no node Buffer            (scene runtime: plain Writer/Reader + @protobufjs utf8)
//   NO_LONG=1   : util.Long starts unset    (scene runtime: `inquire("long")` cannot resolve,
//                 so the gen files must install Long themselves and call configure())
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
