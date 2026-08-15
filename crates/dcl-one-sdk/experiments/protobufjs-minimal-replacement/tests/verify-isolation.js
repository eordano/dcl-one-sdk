"use strict";
// Sanity check for the harness itself: every corpus module in each pass must be bound to
// exactly one implementation, and the two passes must be bound to different ones.
// (A silent cross-binding here would make the whole differential test meaningless.)

const H = require("./harness");
const path = require("path");

const ref = H.loadCorpus("protobufjs/minimal");
const mine = H.loadCorpus("pbmin");

const p = H.loadImpl("mine");
const q = H.loadImpl("ref");

let seen = { ref: 0, mine: 0 };
function probe(mods, label) {
    const counts = { ref: 0, mine: 0, unknown: 0 };
    const pOrig = p.Reader.create, qOrig = q.Reader.create;
    for (const [rel, mod] of mods) {
        for (const key of Object.keys(mod)) {
            const ns = mod[key];
            if (!ns || typeof ns.decode !== "function") continue;
            let hit = null;
            p.Reader.create = function (b) { hit = "mine"; return pOrig.call(this, b); };
            q.Reader.create = function (b) { hit = "ref"; return qOrig.call(this, b); };
            try { ns.decode(new Uint8Array(0)); } catch (e) { /* ignore */ }
            p.Reader.create = pOrig; q.Reader.create = qOrig;
            counts[hit || "unknown"]++;
            break; // one namespace per module is enough
        }
    }
    console.log(`${label}: bound-to-ref=${counts.ref} bound-to-mine=${counts.mine} unknown=${counts.unknown}`);
    return counts;
}

const a = probe(ref.mods, "pass1 (expect all ref)     ");
const b = probe(mine.mods, "pass2 (expect all mine)    ");

// The rpc/data-layer corpus is loaded from a different root with a different keep-predicate
// and gets its own per-root cache purge, so it needs its own proof of per-module binding.
const rpcRef = H.loadRpcCorpus("protobufjs/minimal");
const rpcMine = H.loadRpcCorpus("pbmin");
const c = probe(rpcRef.mods, "rpc pass1 (expect all ref) ");
const d = probe(rpcMine.mods, "rpc pass2 (expect all mine)");

const ok = a.mine === 0 && b.ref === 0 && a.ref > 0 && b.mine > 0
    && c.mine === 0 && d.ref === 0 && c.ref > 0 && d.mine > 0;
console.log(ok ? "ISOLATION OK" : "ISOLATION BROKEN");
process.exitCode = ok ? 0 : 1;
