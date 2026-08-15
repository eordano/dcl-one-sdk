"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.AssetLoadLoadingStateSchema = void 0;
const asset_load_loading_state_gen_1 = require("./pb/decentraland/sdk/components/asset_load_loading_state.gen");
/**
 * @internal
 */
exports.AssetLoadLoadingStateSchema = {
    COMPONENT_ID: 1214,
    serialize(value, builder) {
        const writer = asset_load_loading_state_gen_1.PBAssetLoadLoadingState.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return asset_load_loading_state_gen_1.PBAssetLoadLoadingState.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return asset_load_loading_state_gen_1.PBAssetLoadLoadingState.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAssetLoadLoadingState"
    }
};
