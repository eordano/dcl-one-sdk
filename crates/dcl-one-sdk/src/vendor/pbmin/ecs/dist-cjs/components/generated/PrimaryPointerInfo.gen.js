"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.PrimaryPointerInfoSchema = void 0;
const primary_pointer_info_gen_1 = require("./pb/decentraland/sdk/components/primary_pointer_info.gen");
/**
 * @internal
 */
exports.PrimaryPointerInfoSchema = {
    COMPONENT_ID: 1209,
    serialize(value, builder) {
        const writer = primary_pointer_info_gen_1.PBPrimaryPointerInfo.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return primary_pointer_info_gen_1.PBPrimaryPointerInfo.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return primary_pointer_info_gen_1.PBPrimaryPointerInfo.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPrimaryPointerInfo"
    }
};
