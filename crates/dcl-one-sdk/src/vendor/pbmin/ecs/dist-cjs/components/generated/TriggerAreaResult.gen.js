"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.TriggerAreaResultSchema = void 0;
const trigger_area_result_gen_1 = require("./pb/decentraland/sdk/components/trigger_area_result.gen");
/**
 * @internal
 */
exports.TriggerAreaResultSchema = {
    COMPONENT_ID: 1061,
    serialize(value, builder) {
        const writer = trigger_area_result_gen_1.PBTriggerAreaResult.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return trigger_area_result_gen_1.PBTriggerAreaResult.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return trigger_area_result_gen_1.PBTriggerAreaResult.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTriggerAreaResult"
    }
};
