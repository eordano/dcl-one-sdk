import { PBLightSource } from './pb/decentraland/sdk/components/light_source.gen';
/**
 * @internal
 */
export const LightSourceSchema = {
    COMPONENT_ID: 1079,
    serialize(value, builder) {
        const writer = PBLightSource.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBLightSource.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBLightSource.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBLightSource"
    }
};
