"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.PhysicsCombinedForceSchema = void 0;
const physics_combined_force_gen_1 = require("./pb/decentraland/sdk/components/physics_combined_force.gen");
/**
 * @internal
 */
exports.PhysicsCombinedForceSchema = {
    COMPONENT_ID: 1216,
    serialize(value, builder) {
        const writer = physics_combined_force_gen_1.PBPhysicsCombinedForce.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return physics_combined_force_gen_1.PBPhysicsCombinedForce.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return physics_combined_force_gen_1.PBPhysicsCombinedForce.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPhysicsCombinedForce"
    }
};
