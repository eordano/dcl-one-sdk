import { PBPhysicsCombinedImpulse } from './pb/decentraland/sdk/components/physics_combined_impulse.gen';
/**
 * @internal
 */
export const PhysicsCombinedImpulseSchema = {
    COMPONENT_ID: 1215,
    serialize(value, builder) {
        const writer = PBPhysicsCombinedImpulse.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBPhysicsCombinedImpulse.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBPhysicsCombinedImpulse.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPhysicsCombinedImpulse"
    }
};
