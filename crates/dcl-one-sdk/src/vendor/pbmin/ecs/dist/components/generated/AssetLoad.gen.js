import { PBAssetLoad } from './pb/decentraland/sdk/components/asset_load.gen';
/**
 * @internal
 */
export const AssetLoadSchema = {
    COMPONENT_ID: 1213,
    serialize(value, builder) {
        const writer = PBAssetLoad.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBAssetLoad.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBAssetLoad.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAssetLoad"
    }
};
