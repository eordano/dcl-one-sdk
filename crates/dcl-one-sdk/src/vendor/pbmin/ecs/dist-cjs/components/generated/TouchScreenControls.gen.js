"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.TouchScreenControlsSchema = void 0;
const touch_screen_controls_gen_1 = require("./pb/decentraland/sdk/components/touch_screen_controls.gen");
/**
 * @internal
 */
exports.TouchScreenControlsSchema = {
    COMPONENT_ID: 1218,
    serialize(value, builder) {
        const writer = touch_screen_controls_gen_1.PBTouchScreenControls.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return touch_screen_controls_gen_1.PBTouchScreenControls.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return touch_screen_controls_gen_1.PBTouchScreenControls.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTouchScreenControls"
    }
};
