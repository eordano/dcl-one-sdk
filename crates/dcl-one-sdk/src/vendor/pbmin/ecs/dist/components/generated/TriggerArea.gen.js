import { PBTriggerArea } from './pb/decentraland/sdk/components/trigger_area.gen';
/**
 * @internal
 */
export const TriggerAreaSchema = {
    COMPONENT_ID: 1060,
    serialize(value, builder) {
        const writer = PBTriggerArea.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBTriggerArea.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBTriggerArea.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTriggerArea"
    }
};
