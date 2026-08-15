import { PBParticleSystem } from './pb/decentraland/sdk/components/particle_system.gen';
/**
 * @internal
 */
export const ParticleSystemSchema = {
    COMPONENT_ID: 1217,
    serialize(value, builder) {
        const writer = PBParticleSystem.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBParticleSystem.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBParticleSystem.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBParticleSystem"
    }
};
