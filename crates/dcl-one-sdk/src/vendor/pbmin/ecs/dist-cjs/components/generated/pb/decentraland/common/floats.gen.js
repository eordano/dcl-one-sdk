"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.FloatRange = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.common";
function createBaseFloatRange() {
    return { start: 0, end: 0 };
}
/**
 * @public
 */
var FloatRange;
(function (FloatRange) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
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
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
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
})(FloatRange = exports.FloatRange || (exports.FloatRange = {}));
