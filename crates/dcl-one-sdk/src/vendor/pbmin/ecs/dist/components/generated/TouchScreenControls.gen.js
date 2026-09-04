import { PBTouchScreenControls } from './pb/decentraland/sdk/components/touch_screen_controls.gen';
/**
 * @internal
 */
export const TouchScreenControlsSchema = {
    COMPONENT_ID: 1218,
    serialize(value, builder) {
        const writer = PBTouchScreenControls.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBTouchScreenControls.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBTouchScreenControls.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBTouchScreenControls"
    }
};
