import { PBAssetLoadLoadingState } from './pb/decentraland/sdk/components/asset_load_loading_state.gen';
/**
 * @internal
 */
export const AssetLoadLoadingStateSchema = {
    COMPONENT_ID: 1214,
    serialize(value, builder) {
        const writer = PBAssetLoadLoadingState.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBAssetLoadLoadingState.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBAssetLoadLoadingState.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAssetLoadLoadingState"
    }
};
