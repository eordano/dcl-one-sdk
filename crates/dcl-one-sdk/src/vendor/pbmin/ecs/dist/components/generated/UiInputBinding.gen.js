import { PBUiInputBinding } from './pb/decentraland/sdk/components/ui_input_binding.gen';
/**
 * @internal
 */
export const UiInputBindingSchema = {
    COMPONENT_ID: 1219,
    serialize(value, builder) {
        const writer = PBUiInputBinding.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBUiInputBinding.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBUiInputBinding.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBUiInputBinding"
    }
};
