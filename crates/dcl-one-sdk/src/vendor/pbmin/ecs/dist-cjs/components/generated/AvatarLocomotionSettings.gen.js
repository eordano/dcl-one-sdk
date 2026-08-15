"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.AvatarLocomotionSettingsSchema = void 0;
const avatar_locomotion_settings_gen_1 = require("./pb/decentraland/sdk/components/avatar_locomotion_settings.gen");
/**
 * @internal
 */
exports.AvatarLocomotionSettingsSchema = {
    COMPONENT_ID: 1211,
    serialize(value, builder) {
        const writer = avatar_locomotion_settings_gen_1.PBAvatarLocomotionSettings.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return avatar_locomotion_settings_gen_1.PBAvatarLocomotionSettings.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return avatar_locomotion_settings_gen_1.PBAvatarLocomotionSettings.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAvatarLocomotionSettings"
    }
};
