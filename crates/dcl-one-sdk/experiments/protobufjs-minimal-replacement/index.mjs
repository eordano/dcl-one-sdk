// ESM facade over the CommonJS core. Named + default exports mirror `protobufjs/minimal`.
import protobuf from "./index.js";

export const build = protobuf.build;
export const Writer = protobuf.Writer;
export const BufferWriter = protobuf.BufferWriter;
export const Reader = protobuf.Reader;
export const BufferReader = protobuf.BufferReader;
export const util = protobuf.util;
export const rpc = protobuf.rpc;
export const roots = protobuf.roots;
export const configure = protobuf.configure;
export default protobuf;
