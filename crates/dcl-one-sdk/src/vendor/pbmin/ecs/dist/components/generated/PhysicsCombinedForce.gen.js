import { PBPhysicsCombinedForce } from './pb/decentraland/sdk/components/physics_combined_force.gen';
/**
 * @internal
 */
export const PhysicsCombinedForceSchema = {
    COMPONENT_ID: 1216,
    serialize(value, builder) {
        const writer = PBPhysicsCombinedForce.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBPhysicsCombinedForce.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBPhysicsCombinedForce.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPhysicsCombinedForce"
    }
};
