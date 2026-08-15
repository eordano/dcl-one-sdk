// Type surface compatible with `protobufjs/minimal` as consumed by ts-proto output.

export interface Long { low: number; high: number; unsigned: boolean }

export class Writer {
    static create(): Writer;
    static alloc(size: number): Uint8Array;
    len: number;
    uint32(value: number): Writer;
    int32(value: number): Writer;
    sint32(value: number): Writer;
    uint64(value: Long | number | string): Writer;
    int64(value: Long | number | string): Writer;
    sint64(value: Long | number | string): Writer;
    bool(value: boolean): Writer;
    fixed32(value: number): Writer;
    sfixed32(value: number): Writer;
    fixed64(value: Long | number | string): Writer;
    sfixed64(value: Long | number | string): Writer;
    float(value: number): Writer;
    double(value: number): Writer;
    bytes(value: Uint8Array | string): Writer;
    string(value: string): Writer;
    fork(): Writer;
    reset(): Writer;
    ldelim(): Writer;
    finish(): Uint8Array;
}

export class BufferWriter extends Writer {
    static alloc(size: number): Uint8Array;
    finish(): Uint8Array;
}

export class Reader {
    constructor(buffer: Uint8Array);
    static create(buffer: Uint8Array): Reader;
    buf: Uint8Array;
    pos: number;
    len: number;
    uint32(): number;
    int32(): number;
    sint32(): number;
    int64(): Long | number;
    uint64(): Long | number;
    sint64(): Long | number;
    bool(): boolean;
    fixed32(): number;
    sfixed32(): number;
    fixed64(): Long | number;
    sfixed64(): Long | number;
    float(): number;
    double(): number;
    bytes(): Uint8Array;
    string(): string;
    skip(length?: number): Reader;
    skipType(wireType: number): Reader;
}

export class BufferReader extends Reader {
    constructor(buffer: Uint8Array);
}

export namespace util {
    let Long: any;
    let Buffer: any;
    const LongBits: any;
    const utf8: { length(s: string): number; read(b: Uint8Array, start: number, end: number): string; write(s: string, b: Uint8Array, off: number): number };
    const base64: { length(s: string): number; encode(b: Uint8Array, start: number, end: number): string; decode(s: string, b: Uint8Array, off: number): number; test(s: string): boolean };
    const float: any;
    const pool: any;
    const Array: any;
    const isNode: boolean;
    const global: any;
    function isString(v: any): boolean;
    function isInteger(v: any): boolean;
    function isObject(v: any): boolean;
    function isSet(obj: any, prop: string): boolean;
    function merge<T>(dst: T, src: any, ifNotSet?: boolean): T;
    function newBuffer(sizeOrArray?: number | number[]): Uint8Array;
    function _configure(): void;
}

export const build: string;
export const roots: { [k: string]: any };
export namespace rpc { class Service { constructor(rpcImpl: any, requestDelimited?: boolean, responseDelimited?: boolean) } }
export function configure(): void;

declare const protobuf: {
    build: string;
    Writer: typeof Writer; BufferWriter: typeof BufferWriter;
    Reader: typeof Reader; BufferReader: typeof BufferReader;
    util: typeof util; rpc: typeof rpc; roots: typeof roots;
    configure: typeof configure;
};
export default protobuf;
