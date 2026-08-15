"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBInputModifier_StandardInput = exports.PBInputModifier = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBInputModifier() {
    return { mode: undefined };
}
/**
 * @public
 */
var PBInputModifier;
(function (PBInputModifier) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        switch (message.mode?.$case) {
            case "standard":
                PBInputModifier_StandardInput.encode(message.mode.standard, writer.uint32(10).fork()).ldelim();
                break;
        }
        return writer;
    }
    PBInputModifier.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBInputModifier();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.mode = { $case: "standard", standard: PBInputModifier_StandardInput.decode(reader, reader.uint32()) };
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBInputModifier.decode = decode;
})(PBInputModifier = exports.PBInputModifier || (exports.PBInputModifier = {}));
function createBasePBInputModifier_StandardInput() {
    return {
        disableAll: undefined,
        disableWalk: undefined,
        disableJog: undefined,
        disableRun: undefined,
        disableJump: undefined,
        disableEmote: undefined,
        disableDoubleJump: undefined,
        disableGliding: undefined,
    };
}
/**
 * @public
 */
var PBInputModifier_StandardInput;
(function (PBInputModifier_StandardInput) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.disableAll !== undefined) {
            writer.uint32(8).bool(message.disableAll);
        }
        if (message.disableWalk !== undefined) {
            writer.uint32(16).bool(message.disableWalk);
        }
        if (message.disableJog !== undefined) {
            writer.uint32(24).bool(message.disableJog);
        }
        if (message.disableRun !== undefined) {
            writer.uint32(32).bool(message.disableRun);
        }
        if (message.disableJump !== undefined) {
            writer.uint32(40).bool(message.disableJump);
        }
        if (message.disableEmote !== undefined) {
            writer.uint32(48).bool(message.disableEmote);
        }
        if (message.disableDoubleJump !== undefined) {
            writer.uint32(56).bool(message.disableDoubleJump);
        }
        if (message.disableGliding !== undefined) {
            writer.uint32(64).bool(message.disableGliding);
        }
        return writer;
    }
    PBInputModifier_StandardInput.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBInputModifier_StandardInput();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.disableAll = reader.bool();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.disableWalk = reader.bool();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.disableJog = reader.bool();
                    continue;
                case 4:
                    if (tag !== 32) {
                        break;
                    }
                    message.disableRun = reader.bool();
                    continue;
                case 5:
                    if (tag !== 40) {
                        break;
                    }
                    message.disableJump = reader.bool();
                    continue;
                case 6:
                    if (tag !== 48) {
                        break;
                    }
                    message.disableEmote = reader.bool();
                    continue;
                case 7:
                    if (tag !== 56) {
                        break;
                    }
                    message.disableDoubleJump = reader.bool();
                    continue;
                case 8:
                    if (tag !== 64) {
                        break;
                    }
                    message.disableGliding = reader.bool();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBInputModifier_StandardInput.decode = decode;
})(PBInputModifier_StandardInput = exports.PBInputModifier_StandardInput || (exports.PBInputModifier_StandardInput = {}));
