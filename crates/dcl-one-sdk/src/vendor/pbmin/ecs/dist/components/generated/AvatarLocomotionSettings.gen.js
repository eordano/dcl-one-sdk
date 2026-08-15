import { PBAvatarLocomotionSettings } from './pb/decentraland/sdk/components/avatar_locomotion_settings.gen';
/**
 * @internal
 */
export const AvatarLocomotionSettingsSchema = {
    COMPONENT_ID: 1211,
    serialize(value, builder) {
        const writer = PBAvatarLocomotionSettings.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBAvatarLocomotionSettings.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBAvatarLocomotionSettings.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAvatarLocomotionSettings"
    }
};
