/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBUiInputBinding() {
    return { actions: [] };
}
/**
 * @public
 */
export var PBUiInputBinding;
(function (PBUiInputBinding) {
    function encode(message, writer = _m0.Writer.create()) {
        writer.uint32(10).fork();
        for (const v of message.actions) {
            writer.int32(v);
        }
        writer.ldelim();
        return writer;
    }
    PBUiInputBinding.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBUiInputBinding();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag === 8) {
                        message.actions.push(reader.int32());
                        continue;
                    }
                    if (tag === 10) {
                        const end2 = reader.uint32() + reader.pos;
                        while (reader.pos < end2) {
                            message.actions.push(reader.int32());
                        }
                        continue;
                    }
                    break;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBUiInputBinding.decode = decode;
})(PBUiInputBinding || (PBUiInputBinding = {}));
