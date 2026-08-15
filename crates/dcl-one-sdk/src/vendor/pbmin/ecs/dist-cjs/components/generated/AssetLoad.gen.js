"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.AssetLoadSchema = void 0;
const asset_load_gen_1 = require("./pb/decentraland/sdk/components/asset_load.gen");
/**
 * @internal
 */
exports.AssetLoadSchema = {
    COMPONENT_ID: 1213,
    serialize(value, builder) {
        const writer = asset_load_gen_1.PBAssetLoad.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return asset_load_gen_1.PBAssetLoad.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return asset_load_gen_1.PBAssetLoad.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAssetLoad"
    }
};
