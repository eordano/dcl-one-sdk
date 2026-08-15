import { PBSkyboxTime } from './pb/decentraland/sdk/components/skybox_time.gen';
/**
 * @internal
 */
export const SkyboxTimeSchema = {
    COMPONENT_ID: 1210,
    serialize(value, builder) {
        const writer = PBSkyboxTime.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBSkyboxTime.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBSkyboxTime.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBSkyboxTime"
    }
};
