import { PBGltfNodeModifiers } from './pb/decentraland/sdk/components/gltf_node_modifiers.gen';
/**
 * @internal
 */
export const GltfNodeModifiersSchema = {
    COMPONENT_ID: 1099,
    serialize(value, builder) {
        const writer = PBGltfNodeModifiers.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBGltfNodeModifiers.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBGltfNodeModifiers.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBGltfNodeModifiers"
    }
};
