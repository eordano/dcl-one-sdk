"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBSkyboxTime = exports.TransitionMode = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
/** Controls the direction for animated skybox transitions */
/**
 * @public
 */
var TransitionMode;
(function (TransitionMode) {
    /** TM_FORWARD - transitions forward (default) */
    TransitionMode[TransitionMode["TM_FORWARD"] = 0] = "TM_FORWARD";
    /** TM_BACKWARD - transitions backward */
    TransitionMode[TransitionMode["TM_BACKWARD"] = 1] = "TM_BACKWARD";
})(TransitionMode = exports.TransitionMode || (exports.TransitionMode = {}));
function createBasePBSkyboxTime() {
    return { fixedTime: 0, transitionMode: undefined };
}
/**
 * @public
 */
var PBSkyboxTime;
(function (PBSkyboxTime) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.fixedTime !== 0) {
            writer.uint32(8).uint32(message.fixedTime);
        }
        if (message.transitionMode !== undefined) {
            writer.uint32(16).int32(message.transitionMode);
        }
        return writer;
    }
    PBSkyboxTime.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBSkyboxTime();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.fixedTime = reader.uint32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.transitionMode = reader.int32();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBSkyboxTime.decode = decode;
})(PBSkyboxTime = exports.PBSkyboxTime || (exports.PBSkyboxTime = {}));
