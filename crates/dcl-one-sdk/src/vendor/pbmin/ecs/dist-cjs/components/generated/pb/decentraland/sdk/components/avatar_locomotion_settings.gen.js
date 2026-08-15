"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBAvatarLocomotionSettings = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBAvatarLocomotionSettings() {
    return {
        walkSpeed: undefined,
        jogSpeed: undefined,
        runSpeed: undefined,
        jumpHeight: undefined,
        runJumpHeight: undefined,
        hardLandingCooldown: undefined,
        doubleJumpHeight: undefined,
        glidingSpeed: undefined,
        glidingFallingSpeed: undefined,
    };
}
/**
 * @public
 */
var PBAvatarLocomotionSettings;
(function (PBAvatarLocomotionSettings) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.walkSpeed !== undefined) {
            writer.uint32(13).float(message.walkSpeed);
        }
        if (message.jogSpeed !== undefined) {
            writer.uint32(21).float(message.jogSpeed);
        }
        if (message.runSpeed !== undefined) {
            writer.uint32(29).float(message.runSpeed);
        }
        if (message.jumpHeight !== undefined) {
            writer.uint32(37).float(message.jumpHeight);
        }
        if (message.runJumpHeight !== undefined) {
            writer.uint32(45).float(message.runJumpHeight);
        }
        if (message.hardLandingCooldown !== undefined) {
            writer.uint32(53).float(message.hardLandingCooldown);
        }
        if (message.doubleJumpHeight !== undefined) {
            writer.uint32(61).float(message.doubleJumpHeight);
        }
        if (message.glidingSpeed !== undefined) {
            writer.uint32(69).float(message.glidingSpeed);
        }
        if (message.glidingFallingSpeed !== undefined) {
            writer.uint32(77).float(message.glidingFallingSpeed);
        }
        return writer;
    }
    PBAvatarLocomotionSettings.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAvatarLocomotionSettings();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 13) {
                        break;
                    }
                    message.walkSpeed = reader.float();
                    continue;
                case 2:
                    if (tag !== 21) {
                        break;
                    }
                    message.jogSpeed = reader.float();
                    continue;
                case 3:
                    if (tag !== 29) {
                        break;
                    }
                    message.runSpeed = reader.float();
                    continue;
                case 4:
                    if (tag !== 37) {
                        break;
                    }
                    message.jumpHeight = reader.float();
                    continue;
                case 5:
                    if (tag !== 45) {
                        break;
                    }
                    message.runJumpHeight = reader.float();
                    continue;
                case 6:
                    if (tag !== 53) {
                        break;
                    }
                    message.hardLandingCooldown = reader.float();
                    continue;
                case 7:
                    if (tag !== 61) {
                        break;
                    }
                    message.doubleJumpHeight = reader.float();
                    continue;
                case 8:
                    if (tag !== 69) {
                        break;
                    }
                    message.glidingSpeed = reader.float();
                    continue;
                case 9:
                    if (tag !== 77) {
                        break;
                    }
                    message.glidingFallingSpeed = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAvatarLocomotionSettings.decode = decode;
})(PBAvatarLocomotionSettings = exports.PBAvatarLocomotionSettings || (exports.PBAvatarLocomotionSettings = {}));
