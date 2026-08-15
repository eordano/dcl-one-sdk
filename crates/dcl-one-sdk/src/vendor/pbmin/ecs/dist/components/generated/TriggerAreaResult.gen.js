import { PBTriggerAreaResult } from './pb/decentraland/sdk/components/trigger_area_result.gen';
/**
 * @internal
 */
export const TriggerAreaResultSchema = {
    COMPONENT_ID: 1061,
    serialize(value, builder) {
        const writer = PBTriggerAreaResult.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBTriggerAreaResult.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBTriggerAreaResult.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTriggerAreaResult"
    }
};
