/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBAssetLoadLoadingState() {
    return { currentState: 0, asset: "", timestamp: 0 };
}
/**
 * @public
 */
export var PBAssetLoadLoadingState;
(function (PBAssetLoadLoadingState) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.currentState !== 0) {
            writer.uint32(8).int32(message.currentState);
        }
        if (message.asset !== "") {
            writer.uint32(18).string(message.asset);
        }
        if (message.timestamp !== 0) {
            writer.uint32(24).uint32(message.timestamp);
        }
        return writer;
    }
    PBAssetLoadLoadingState.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAssetLoadLoadingState();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.currentState = reader.int32();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.asset = reader.string();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.timestamp = reader.uint32();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAssetLoadLoadingState.decode = decode;
})(PBAssetLoadLoadingState || (PBAssetLoadLoadingState = {}));
