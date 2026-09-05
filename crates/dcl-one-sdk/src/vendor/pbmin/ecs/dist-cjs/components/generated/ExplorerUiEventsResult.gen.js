"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExplorerUiEventsResultSchema = void 0;
const explorer_ui_events_result_gen_1 = require("./pb/decentraland/sdk/components/explorer_ui_events_result.gen");
/**
 * @internal
 */
exports.ExplorerUiEventsResultSchema = {
    COMPONENT_ID: 1220,
    serialize(value, builder) {
        const writer = explorer_ui_events_result_gen_1.PBExplorerUiEventsResult.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return explorer_ui_events_result_gen_1.PBExplorerUiEventsResult.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return explorer_ui_events_result_gen_1.PBExplorerUiEventsResult.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBExplorerUiEventsResult"
    }
};
