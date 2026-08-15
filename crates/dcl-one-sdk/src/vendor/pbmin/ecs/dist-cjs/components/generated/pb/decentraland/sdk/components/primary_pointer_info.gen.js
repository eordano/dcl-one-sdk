"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBPrimaryPointerInfo = exports.PointerType = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const vectors_gen_1 = require("../../common/vectors.gen");
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * PointerType enumerates the different input devices that can be used for pointer interactions.
 * Each type has specific characteristics and use cases in the virtual world.
 */
/**
 * @public
 */
var PointerType;
(function (PointerType) {
    /** POT_NONE - No pointer input */
    PointerType[PointerType["POT_NONE"] = 0] = "POT_NONE";
    /** POT_MOUSE - Traditional mouse input */
    PointerType[PointerType["POT_MOUSE"] = 1] = "POT_MOUSE";
})(PointerType = exports.PointerType || (exports.PointerType = {}));
function createBasePBPrimaryPointerInfo() {
    return { pointerType: undefined, screenCoordinates: undefined, screenDelta: undefined, worldRayDirection: undefined };
}
/**
 * @public
 */
var PBPrimaryPointerInfo;
(function (PBPrimaryPointerInfo) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.pointerType !== undefined) {
            writer.uint32(8).int32(message.pointerType);
        }
        if (message.screenCoordinates !== undefined) {
            vectors_gen_1.Vector2.encode(message.screenCoordinates, writer.uint32(18).fork()).ldelim();
        }
        if (message.screenDelta !== undefined) {
            vectors_gen_1.Vector2.encode(message.screenDelta, writer.uint32(26).fork()).ldelim();
        }
        if (message.worldRayDirection !== undefined) {
            vectors_gen_1.Vector3.encode(message.worldRayDirection, writer.uint32(34).fork()).ldelim();
        }
        return writer;
    }
    PBPrimaryPointerInfo.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBPrimaryPointerInfo();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.pointerType = reader.int32();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.screenCoordinates = vectors_gen_1.Vector2.decode(reader, reader.uint32());
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.screenDelta = vectors_gen_1.Vector2.decode(reader, reader.uint32());
                    continue;
                case 4:
                    if (tag !== 34) {
                        break;
                    }
                    message.worldRayDirection = vectors_gen_1.Vector3.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBPrimaryPointerInfo.decode = decode;
})(PBPrimaryPointerInfo = exports.PBPrimaryPointerInfo || (exports.PBPrimaryPointerInfo = {}));
