"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.PhysicsCombinedImpulseSchema = void 0;
const physics_combined_impulse_gen_1 = require("./pb/decentraland/sdk/components/physics_combined_impulse.gen");
/**
 * @internal
 */
exports.PhysicsCombinedImpulseSchema = {
    COMPONENT_ID: 1215,
    serialize(value, builder) {
        const writer = physics_combined_impulse_gen_1.PBPhysicsCombinedImpulse.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return physics_combined_impulse_gen_1.PBPhysicsCombinedImpulse.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return physics_combined_impulse_gen_1.PBPhysicsCombinedImpulse.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPhysicsCombinedImpulse"
    }
};
