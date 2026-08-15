"use strict";
// Meta-test: a differential suite that reports "0 divergences" is only meaningful if it can
// actually see divergences. This injects deliberate, realistic wire-format bugs into a copy
// of the module and asserts that each phase catches them.

const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const ROOT = path.resolve(__dirname, "..");
const SRC = fs.readFileSync(path.join(ROOT, "pbmin/index.js"), "utf8");
const MUT = path.join(ROOT, "mutant/index.js");

const MUTANTS = [
    ["int32-negative-not-10-bytes",
        `Writer.prototype.int32 = function write_int32(value) {
    return value < 0
        ? this._push(writeVarint64, 10, LongBits.fromNumber(value))`,
        `Writer.prototype.int32 = function write_int32(value) {
    return value < 0
        ? this._push(writeVarint64, 5, LongBits.fromNumber(value))`],

    ["sint32-zigzag-dropped",
        "return this.uint32((value << 1 ^ value >> 31) >>> 0);",
        "return this.uint32((value << 1) >>> 0);"],

    // NB: the obvious "(& 15) -> (& 127)" mutation on the 5th varint byte is an EQUIVALENT
    // mutant - JS `<<` discards bits above 31, so both expressions are identical for every
    // input byte. It is deliberately not used here. These two are real:
    ["uint32-read-shift",
        "value = (value | (this.buf[this.pos] & 127) << 21) >>> 0; if (this.buf[this.pos++] < 128) return value;",
        "value = (value | (this.buf[this.pos] & 127) << 22) >>> 0; if (this.buf[this.pos++] < 128) return value;"],

    ["uint32-overlong-varint-advance",
        `        if ((this.pos += 5) > this.len) {`,
        `        if ((this.pos += 4) > this.len) {`],

    ["varint-length-boundary-off-by-one",
        "? part0 < 128 ? 1 : 2",
        "? part0 <= 128 ? 1 : 2"],

    ["utf8-surrogate-pair-length",
        "{ ++i; len += 4; }",
        "{ ++i; len += 3; }"],

    ["sint64-zzdecode-mask",
        "var mask = -(this.lo & 1);",
        "var mask = -(this.lo & 3);"],

    ["fixed32-byte-order",
        `    return (buf[end - 4]
        | buf[end - 3] << 8
        | buf[end - 2] << 16
        | buf[end - 1] << 24) >>> 0;`,
        `    return (buf[end - 1]
        | buf[end - 2] << 8
        | buf[end - 3] << 16
        | buf[end - 4] << 24) >>> 0;`],

    ["reader-string-truncation-not-clamped",
        "? this.buf.utf8Slice(this.pos, this.pos = Math.min(this.pos + len, this.len))",
        "? this.buf.utf8Slice(this.pos, this.pos = this.pos + len)"],

    ["ldelim-drops-empty-submessage",
        `    this.reset().uint32(len);
    if (len) {`,
        `    if (len) this.reset().uint32(len); else this.reset();
    if (len) {`],

    ["skipType-group-terminator",
        "while ((wireType = this.uint32() & 7) !== 4) {",
        "while ((wireType = this.uint32() & 7) !== 3) {"],

    ["float-written-as-double",
        "return this._push(util.float.writeFloatLE, 4, value);",
        "return this._push(util.float.writeDoubleLE, 4, value);"],

    ["bytes-empty-writes-nothing",
        `    var len = value.length >>> 0;
    this.uint32(len);
    if (len)
        this._push(BufferWriter.writeBytesBuffer, len, value);`,
        `    var len = value.length >>> 0;
    if (len) { this.uint32(len); this._push(BufferWriter.writeBytesBuffer, len, value); }`],
];

const phases = [
    ["phase1", ["tests/corpus-diff.js"], { ITERS: "40" }],
    ["phase1-nobuf", ["tests/corpus-diff.js"], { ITERS: "40", NO_BUFFER: "1" }],
    // The rpc/data-layer catalogue is 41 namespaces against the ecs corpus's 336, so it is
    // listed here to show it has detection power of its own rather than riding on phase 1.
    ["phase1c-rpc", ["tests/rpc-diff.js"], { ITERS: "400" }],
    ["phase2", ["tests/primitive-diff.js"], {}],
    ["phase3", ["tests/fuzz.js"], { N_RAW: "40000", N_MSG: "8" }],
];

let allCaught = true;
console.log("\n=== MUTATION TEST: does the differential suite have detection power? ===\n");
for (const [name, from, to] of MUTANTS) {
    if (!SRC.includes(from)) { console.log(`  ${name.padEnd(38)} !! mutation anchor not found - SKIPPED`); allCaught = false; continue; }
    fs.writeFileSync(MUT, SRC.replace(from, to));
    const caughtBy = [];
    for (const [pname, argv, env] of phases) {
        try {
            execFileSync(process.execPath, argv, {
                cwd: ROOT, stdio: "pipe",
                env: { ...process.env, ...env, MINE_ID: "pbmutant", SEED: "12648430" },
            });
        } catch (e) {
            caughtBy.push(pname); // non-zero exit == divergence detected
        }
    }
    const ok = caughtBy.length > 0;
    if (!ok) allCaught = false;
    console.log(`  ${name.padEnd(38)} ${ok ? "CAUGHT" : "*** MISSED ***"}  by: ${caughtBy.join(", ") || "-"}`);
}
fs.writeFileSync(MUT, SRC); // leave the mutant dir holding a clean copy
console.log(`\n${allCaught ? "All mutants detected." : "SOME MUTANTS SURVIVED - suite has blind spots."}`);
process.exitCode = allCaught ? 0 : 1;
