"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.SkyboxTimeSchema = void 0;
const skybox_time_gen_1 = require("./pb/decentraland/sdk/components/skybox_time.gen");
/**
 * @internal
 */
exports.SkyboxTimeSchema = {
    COMPONENT_ID: 1210,
    serialize(value, builder) {
        const writer = skybox_time_gen_1.PBSkyboxTime.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return skybox_time_gen_1.PBSkyboxTime.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return skybox_time_gen_1.PBSkyboxTime.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBSkyboxTime"
    }
};
