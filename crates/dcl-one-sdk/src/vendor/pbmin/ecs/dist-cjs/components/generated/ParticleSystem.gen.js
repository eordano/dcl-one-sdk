"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ParticleSystemSchema = void 0;
const particle_system_gen_1 = require("./pb/decentraland/sdk/components/particle_system.gen");
/**
 * @internal
 */
exports.ParticleSystemSchema = {
    COMPONENT_ID: 1217,
    serialize(value, builder) {
        const writer = particle_system_gen_1.PBParticleSystem.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return particle_system_gen_1.PBParticleSystem.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return particle_system_gen_1.PBParticleSystem.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBParticleSystem"
    }
};
