/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.common";
function createBaseFloatRange() {
    return { start: 0, end: 0 };
}
/**
 * @public
 */
export var FloatRange;
(function (FloatRange) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.start !== 0) {
            writer.uint32(13).float(message.start);
        }
        if (message.end !== 0) {
            writer.uint32(21).float(message.end);
        }
        return writer;
    }
    FloatRange.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseFloatRange();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.start = reader.float();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.end = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    FloatRange.decode = decode;
})(FloatRange || (FloatRange = {}));
