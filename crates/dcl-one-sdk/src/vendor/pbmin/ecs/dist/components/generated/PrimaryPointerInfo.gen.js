import { PBPrimaryPointerInfo } from './pb/decentraland/sdk/components/primary_pointer_info.gen';
/**
 * @internal
 */
export const PrimaryPointerInfoSchema = {
    COMPONENT_ID: 1209,
    serialize(value, builder) {
        const writer = PBPrimaryPointerInfo.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBPrimaryPointerInfo.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBPrimaryPointerInfo.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBPrimaryPointerInfo"
    }
};
