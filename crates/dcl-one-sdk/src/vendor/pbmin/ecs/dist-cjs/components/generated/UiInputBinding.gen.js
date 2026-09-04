"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.UiInputBindingSchema = void 0;
const ui_input_binding_gen_1 = require("./pb/decentraland/sdk/components/ui_input_binding.gen");
/**
 * @internal
 */
exports.UiInputBindingSchema = {
    COMPONENT_ID: 1219,
    serialize(value, builder) {
        const writer = ui_input_binding_gen_1.PBUiInputBinding.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return ui_input_binding_gen_1.PBUiInputBinding.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return ui_input_binding_gen_1.PBUiInputBinding.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBUiInputBinding"
    }
};
