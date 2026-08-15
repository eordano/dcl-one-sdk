"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.GltfNodeModifiersSchema = void 0;
const gltf_node_modifiers_gen_1 = require("./pb/decentraland/sdk/components/gltf_node_modifiers.gen");
/**
 * @internal
 */
exports.GltfNodeModifiersSchema = {
    COMPONENT_ID: 1099,
    serialize(value, builder) {
        const writer = gltf_node_modifiers_gen_1.PBGltfNodeModifiers.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return gltf_node_modifiers_gen_1.PBGltfNodeModifiers.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return gltf_node_modifiers_gen_1.PBGltfNodeModifiers.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBGltfNodeModifiers"
    }
};
