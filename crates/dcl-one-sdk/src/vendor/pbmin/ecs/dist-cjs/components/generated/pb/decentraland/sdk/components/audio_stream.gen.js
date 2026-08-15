"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBAudioStream = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBAudioStream() {
    return {
        playing: undefined,
        volume: undefined,
        url: "",
        spatial: undefined,
        spatialMinDistance: undefined,
        spatialMaxDistance: undefined,
    };
}
/**
 * @public
 */
var PBAudioStream;
(function (PBAudioStream) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.playing !== undefined) {
            writer.uint32(8).bool(message.playing);
        }
        if (message.volume !== undefined) {
            writer.uint32(21).float(message.volume);
        }
        if (message.url !== "") {
            writer.uint32(26).string(message.url);
        }
        if (message.spatial !== undefined) {
            writer.uint32(32).bool(message.spatial);
        }
        if (message.spatialMinDistance !== undefined) {
            writer.uint32(45).float(message.spatialMinDistance);
        }
        if (message.spatialMaxDistance !== undefined) {
            writer.uint32(53).float(message.spatialMaxDistance);
        }
        return writer;
    }
    PBAudioStream.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAudioStream();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.playing = reader.bool();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.volume = reader.float();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.url = reader.string();
                    continue;
                case 4:
                    if (tag !== 32) {
                        break;
                    }
                    message.spatial = reader.bool();
                    continue;
                case 5:
                    if (tag !== 45) {
                        break;
                    }
                    message.spatialMinDistance = reader.float();
                    continue;
                case 6:
                    if (tag !== 53) {
                        break;
                    }
                    message.spatialMaxDistance = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAudioStream.decode = decode;
})(PBAudioStream = exports.PBAudioStream || (exports.PBAudioStream = {}));
