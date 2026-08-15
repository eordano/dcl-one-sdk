"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.LightSourceSchema = void 0;
const light_source_gen_1 = require("./pb/decentraland/sdk/components/light_source.gen");
/**
 * @internal
 */
exports.LightSourceSchema = {
    COMPONENT_ID: 1079,
    serialize(value, builder) {
        const writer = light_source_gen_1.PBLightSource.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return light_source_gen_1.PBLightSource.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return light_source_gen_1.PBLightSource.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBLightSource"
    }
};
