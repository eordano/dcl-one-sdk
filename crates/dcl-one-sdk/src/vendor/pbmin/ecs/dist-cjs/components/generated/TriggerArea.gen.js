"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.TriggerAreaSchema = void 0;
const trigger_area_gen_1 = require("./pb/decentraland/sdk/components/trigger_area.gen");
/**
 * @internal
 */
exports.TriggerAreaSchema = {
    COMPONENT_ID: 1060,
    serialize(value, builder) {
        const writer = trigger_area_gen_1.PBTriggerArea.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return trigger_area_gen_1.PBTriggerArea.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return trigger_area_gen_1.PBTriggerArea.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTriggerArea"
    }
};
