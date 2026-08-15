/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBAssetLoad() {
    return { assets: [] };
}
/**
 * @public
 */
export var PBAssetLoad;
(function (PBAssetLoad) {
    function encode(message, writer = _m0.Writer.create()) {
        for (const v of message.assets) {
            writer.uint32(10).string(v);
        }
        return writer;
    }
    PBAssetLoad.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAssetLoad();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.assets.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAssetLoad.decode = decode;
})(PBAssetLoad || (PBAssetLoad = {}));
