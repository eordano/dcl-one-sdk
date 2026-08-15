"use strict";
// Phase 2: value-level differential tests of the whole Writer/Reader surface, including
// the wire types the @dcl/ecs corpus never uses (sint*, fixed*, sfixed*) so the module is
// safe if the .proto schemas ever start using them.

const H = require("./harness");
const REF = H.loadImpl("ref");
const MY = H.loadImpl("mine");

const SEED = Number(process.env.SEED || 0xC0FFEE);
const rng = new H.Rng(SEED);

let checks = 0, fails = 0;
const failures = [];

function fail(group, detail) {
    fails++;
    if (failures.length < 40) failures.push(`[${group}] ${detail}`);
}

function hexOf(b) { return Buffer.from(b.buffer || b, b.byteOffset || 0, b.length).toString("hex"); }

// --- writer parity -------------------------------------------------------------------

function checkWrite(group, method, value) {
    // NOTE: comparisons are strictly like-for-like. protobufjs itself is NOT self-consistent
    // across its two writer flavours (BufferWriter uses Buffer.byteLength/utf8Write, which maps
    // lone surrogates to U+FFFD; the pure Writer uses @protobufjs/utf8, which emits WTF-8).
    // A drop-in replacement must reproduce BOTH behaviours, so ref/create is only ever compared
    // against mine/create and ref/plain only against mine/plain.
    let firstBytes = null;
    for (const wname of ["create", "plain"]) {
        const run = (impl) => {
            try {
                const w = wname === "create" ? impl.Writer.create() : new impl.Writer();
                w[method](value);
                return { ok: true, bytes: Buffer.from(w.finish()) };
            } catch (e) { return { ok: false, msg: String(e.message) }; }
        };
        checks++;
        const a = run(REF), b = run(MY);
        if (a.ok !== b.ok) { fail(group, `${method}(${String(value)}) [${wname}] throw parity ref=${a.ok}(${a.msg}) mine=${b.ok}(${b.msg})`); continue; }
        if (!a.ok) { if (a.msg !== b.msg) fail(group, `${method}(${String(value)}) [${wname}] msg ${a.msg} vs ${b.msg}`); continue; }
        if (!a.bytes.equals(b.bytes))
            fail(group, `${method}(${String(value)}) [${wname}] bytes ref=${a.bytes.toString("hex")} mine=${b.bytes.toString("hex")}`);
        if (firstBytes === null) firstBytes = a.bytes;
    }
    return firstBytes;
}

// --- reader parity -------------------------------------------------------------------

function readAll(bytes, method) {
    // Like-for-like again: BufferReader (node Buffer input) vs plain Reader (Uint8Array input)
    // legitimately disagree inside protobufjs on invalid UTF-8 and on truncated strings.
    const out = {};
    for (const cname of ["buffer", "u8array"]) {
        out[cname] = [];
        for (const [impl, name] of [[REF, "ref"], [MY, "mine"]]) {
            const container = cname === "buffer" ? Buffer.from(bytes) : new Uint8Array(bytes);
            try {
                const r = impl.Reader.create(container);
                const v = r[method]();
                out[cname].push([`${name}/${cname}`, { ok: true, v, pos: r.pos }]);
            } catch (e) {
                out[cname].push([`${name}/${cname}`, { ok: false, msg: String(e.message) }]);
            }
        }
    }
    return out;
}

function normalize(v) {
    if (v && typeof v === "object" && typeof v.low === "number") return `long(${v.low},${v.high},${!!v.unsigned})`;
    if (v instanceof Uint8Array) return `bytes(${hexOf(v)})`;
    if (typeof v === "number") return Object.is(v, -0) ? "-0" : String(v);
    if (typeof v === "string") return "str:" + JSON.stringify(v);
    return String(v);
}

function checkRead(group, bytes, method) {
    const groups = readAll(bytes, method);
    for (const cname of Object.keys(groups)) {
        checks++;
        const [[la, a], [lb, b]] = groups[cname];
        if (a.ok !== b.ok) { fail(group, `read ${method} <${hexOf(bytes)}> [${cname}] throw parity ${la}=${a.ok} ${lb}=${b.ok} (${a.msg || b.msg})`); continue; }
        if (!a.ok) { if (a.msg !== b.msg) fail(group, `read ${method} <${hexOf(bytes)}> [${cname}] msg ${a.msg} vs ${b.msg}`); continue; }
        if (normalize(a.v) !== normalize(b.v) || a.pos !== b.pos)
            fail(group, `read ${method} <${hexOf(bytes)}> [${cname}] ${la}=${normalize(a.v)}@${a.pos} ${lb}=${normalize(b.v)}@${b.pos}`);
    }
}

function roundTrip(group, method, readMethod, values) {
    for (const v of values) {
        const bytes = checkWrite(group, method, v);
        if (bytes) checkRead(group, bytes, readMethod);
    }
}

/* -------------------------------------------------- varints (incl. > 2^31, negatives) */

const U32 = [0, 1, 127, 128, 255, 16383, 16384, 2097151, 2097152, 268435455, 268435456,
    2147483647, 2147483648, 4294967295, 3000000000];
const I32 = [0, 1, -1, 127, -128, 2147483647, -2147483648, -1000000, 1000000, -2, -127, -128, -129];

roundTrip("varint/uint32", "uint32", "uint32", U32);
roundTrip("varint/int32", "int32", "int32", I32);
roundTrip("varint/int32-as-uint32", "int32", "uint32", I32);
roundTrip("varint/sint32", "sint32", "sint32", I32);
roundTrip("varint/bool", "bool", "bool", [true, false, 0, 1, "", "x", null, undefined]);

// negative int32 must be a 10-byte varint, not 5
{
    const b = checkWrite("varint/int32-width", "int32", -1);
    checks++;
    if (b && b.length !== 10) fail("varint/int32-width", `int32(-1) encoded ${b.length} bytes, expected 10`);
    const b2 = checkWrite("varint/int32-width", "int32", -2147483648);
    checks++;
    if (b2 && b2.length !== 10) fail("varint/int32-width", `int32(INT_MIN) encoded ${b2.length} bytes, expected 10`);
}

/* -------------------------------------------------- 64 bit + Long interop */

const LongLib = require("long");
function asLong(bigint, unsigned) {
    return LongLib.fromString(BigInt.asUintN(64, bigint).toString(), unsigned);
}

const U64_BIG = H.INTERESTING_U64;
const long64 = [];
for (const v of U64_BIG) { long64.push(asLong(v, true)); long64.push(asLong(v, false)); }
const num64 = [0, 1, -1, 2 ** 31, 2 ** 32, 2 ** 52, 2 ** 53, -(2 ** 53), 9007199254740991, -9007199254740991];
const str64 = ["0", "1", "-1", "9223372036854775807", "-9223372036854775808", "18446744073709551615"];

for (const m of ["int64", "uint64", "sint64", "fixed64", "sfixed64"]) {
    const rm = m;
    roundTrip("long/" + m, m, rm, long64);
    roundTrip("long-num/" + m, m, rm, num64);
    roundTrip("long-str/" + m, m, rm, str64);
}

// util.Long unset -> readers must return plain numbers; and configure() must re-bind.
{
    checks++;
    const savedR = REF.util.Long, savedM = MY.util.Long;
    REF.util.Long = null; REF.configure();
    MY.util.Long = null; MY.configure();
    for (const v of num64) {
        const b = checkWrite("long-off", "int64", v);
        if (b) checkRead("long-off", b, "int64");
    }
    for (const b of [[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
                     [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]]) {
        for (const m of ["int64", "uint64", "sint64"]) checkRead("long-off-read", Buffer.from(b), m);
    }
    REF.util.Long = savedR; REF.configure();
    MY.util.Long = savedM; MY.configure();
    // after restoring, int64 must be Longs again in both
    const b = checkWrite("long-reconfigure", "int64", 42);
    if (b) checkRead("long-reconfigure", b, "int64");
    const rv = REF.Reader.create(Buffer.from(b)).int64();
    const mv = MY.Reader.create(Buffer.from(b)).int64();
    if (typeof rv !== typeof mv || normalize(rv) !== normalize(mv))
        fail("long-reconfigure", `${normalize(rv)} vs ${normalize(mv)}`);
}

/* -------------------------------------------------- fixed 32 */

roundTrip("fixed32", "fixed32", "fixed32", U32);
roundTrip("sfixed32", "sfixed32", "sfixed32", I32);
roundTrip("fixed32-as-sfixed32", "fixed32", "sfixed32", U32);

/* -------------------------------------------------- floats */

roundTrip("float", "float", "float", H.INTERESTING_F32.concat(
    Array.from({ length: 200 }, () => (rng.next() - 0.5) * Math.pow(2, rng.int(80) - 40))));
roundTrip("double", "double", "double", H.INTERESTING_F64.concat(
    Array.from({ length: 200 }, () => (rng.next() - 0.5) * Math.pow(2, rng.int(600) - 300))));

// float/double read of arbitrary bit patterns (NaN payloads, subnormals, ...)
for (let i = 0; i < 400; i++) {
    checkRead("float-bits", rng.bytes(4), "float");
    checkRead("double-bits", rng.bytes(8), "double");
}

/* -------------------------------------------------- strings */

const strings = H.INTERESTING_STRINGS.concat([
    " ", "a b", "  ", "￿", "﻿",
    "\u{10FFFF}", "\u{1F600}\u{1F601}", "\ud83d", "\ude00", "\ud83d😀",
    "é", "é", "ß".repeat(41), "😀".repeat(41), " ".repeat(50),
    "A".repeat(39), "A".repeat(40), "A".repeat(41),   // BufferWriter 40-char fast-path boundary
    "é".repeat(39) + "z", "\ud800" + "x".repeat(45),
]);
for (let i = 0; i < 300; i++) {
    let s = "";
    const n = rng.int(60);
    for (let k = 0; k < n; k++) s += String.fromCharCode(rng.int(0x11000));
    strings.push(s);
}
roundTrip("string", "string", "string", strings);

// decoding arbitrary (often invalid) UTF-8
for (let i = 0; i < 600; i++) {
    const payload = rng.bytes(rng.int(20));
    const buf = Buffer.concat([Buffer.from([payload.length]), payload]);
    checkRead("string-invalid-utf8", buf, "string");
}

/* -------------------------------------------------- bytes */

const byteVals = [Buffer.alloc(0), new Uint8Array(0), Buffer.from([0]), Buffer.from([255, 0, 128]),
    Buffer.from("hello"), new Uint8Array([1, 2, 3]), Buffer.alloc(300, 7)];
for (let i = 0; i < 100; i++) byteVals.push(rng.bytes(rng.int(64)));
roundTrip("bytes", "bytes", "bytes", byteVals);
// base64-string input to writer.bytes()
roundTrip("bytes-base64", "bytes", "bytes", ["", "AAAA", "aGVsbG8=", "/w==", "AQIDBA=="]);

/* -------------------------------------------------- fork / ldelim nesting */

function nestScript(impl, depth, rngLocal) {
    const w = impl.Writer.create();
    (function build(d) {
        w.uint32((d + 1) << 3 | 2).fork();
        w.uint32(8).int32(rngLocal.int(1000) - 500);
        w.uint32(18).string("lvl" + d);
        if (d > 0) build(d - 1);
        w.ldelim();
    })(depth);
    return Buffer.from(w.finish());
}
for (let d = 0; d < 12; d++) {
    checks++;
    const a = nestScript(REF, d, new H.Rng(SEED + d));
    const b = nestScript(MY, d, new H.Rng(SEED + d));
    if (!a.equals(b)) fail("fork/ldelim", `depth ${d}: ${a.toString("hex")} vs ${b.toString("hex")}`);
}

// fork + reset (discard), and empty fork
for (const impl of [[REF, "ref"], [MY, "mine"]]) { /* structural, compared below */ }
{
    function forkReset(impl) {
        const w = impl.Writer.create();
        w.uint32(10).string("keep");
        w.fork(); w.uint32(999999).string("discard"); w.reset();
        w.uint32(18).fork().ldelim();          // empty nested message
        w.uint32(26).string("tail");
        return Buffer.from(w.finish());
    }
    checks++;
    const a = forkReset(REF), b = forkReset(MY);
    if (!a.equals(b)) fail("fork/reset", `${a.toString("hex")} vs ${b.toString("hex")}`);
}

/* -------------------------------------------------- skip / skipType */

function skipScript(impl, bytes, wireTypes) {
    const r = impl.Reader.create(Buffer.from(bytes));
    const log = [];
    try {
        for (const wt of wireTypes) { r.skipType(wt); log.push(r.pos); }
        return { ok: true, log };
    } catch (e) { return { ok: false, msg: String(e.message), log }; }
}

for (let i = 0; i < 2000; i++) {
    const wts = Array.from({ length: rng.int(4) + 1 }, () => rng.pick([0, 1, 2, 3, 5, 4, 6, 7]));
    const bytes = rng.bytes(rng.int(30));
    checks++;
    const a = skipScript(REF, bytes, wts), b = skipScript(MY, bytes, wts);
    if (a.ok !== b.ok || a.msg !== b.msg || JSON.stringify(a.log) !== JSON.stringify(b.log))
        fail("skipType", `wts=${wts} bytes=${bytes.toString("hex")} ref=${JSON.stringify(a)} mine=${JSON.stringify(b)}`);
}

// skip(n) and skip() over varints incl. truncated
for (let i = 0; i < 500; i++) {
    const bytes = rng.bytes(rng.int(12));
    const n = rng.bool() ? rng.int(15) : undefined;
    checks++;
    const run = (impl) => {
        const r = impl.Reader.create(Buffer.from(bytes));
        try { r.skip(n); return { ok: true, pos: r.pos }; } catch (e) { return { ok: false, msg: String(e.message) }; }
    };
    const a = run(REF), b = run(MY);
    if (JSON.stringify(a) !== JSON.stringify(b))
        fail("skip", `n=${n} bytes=${bytes.toString("hex")} ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
}

/* -------------------------------------------------- misc surface */

checks++;
for (const k of ["build", "Writer", "BufferWriter", "Reader", "BufferReader", "util", "rpc", "roots", "configure"])
    if (!(k in MY)) fail("surface", `missing export ${k}`);
for (const k of Object.keys(REF)) if (!(k in MY)) fail("surface", `missing export ${k} (present in ref)`);
for (const k of Object.keys(REF.util)) if (!(k in MY.util)) fail("surface", `missing util.${k}`);
for (const k of Object.keys(REF.Reader.prototype)) if (!(k in MY.Reader.prototype)) fail("surface", `missing Reader.prototype.${k}`);
for (const k of Object.keys(REF.Writer.prototype)) if (!(k in MY.Writer.prototype)) fail("surface", `missing Writer.prototype.${k}`);

// instanceof relationships relied on by ts-proto (`input instanceof _m0.Reader`)
checks++;
if (!(MY.Reader.create(Buffer.alloc(1)) instanceof MY.Reader)) fail("surface", "BufferReader not instanceof Reader");
if (!(MY.Reader.create(new Uint8Array(1)) instanceof MY.Reader)) fail("surface", "Reader not instanceof Reader");
if (!(MY.Writer.create() instanceof MY.Writer)) fail("surface", "BufferWriter not instanceof Writer");
if (Buffer.isBuffer(REF.Writer.create().finish()) !== Buffer.isBuffer(MY.Writer.create().finish()))
    fail("surface", "finish() container type differs");

console.log("\n=== PHASE 2: primitive / API-surface differential ===");
console.log(`seed        : 0x${SEED.toString(16)}`);
console.log(`checks      : ${checks}`);
console.log(`failures    : ${fails}`);
for (const f of failures) console.log("  " + f);
process.exitCode = fails ? 1 : 0;
