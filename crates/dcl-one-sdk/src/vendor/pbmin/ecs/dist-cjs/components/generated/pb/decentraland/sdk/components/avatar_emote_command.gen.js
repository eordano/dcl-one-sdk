"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBAvatarEmoteCommand = exports.EmoteState = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
/** EmoteState describes the lifecycle state of an emote playback. */
/**
 * @public
 */
var EmoteState;
(function (EmoteState) {
    /**
     * ES_STARTED - ES_STARTED indicates the emote started playing.
     * This is the zero value and is used for backward compatibility — entries
     * written by older explorers (field absent) read as "started".
     */
    EmoteState[EmoteState["ES_STARTED"] = 0] = "ES_STARTED";
    /** ES_FINISHED - ES_FINISHED indicates a non-looping emote completed naturally. */
    EmoteState[EmoteState["ES_FINISHED"] = 1] = "ES_FINISHED";
    /**
     * ES_INTERRUPTED - ES_INTERRUPTED indicates playback was cancelled (movement, teleport,
     * another emote superseding it, explicit stop, or scene change).
     */
    EmoteState[EmoteState["ES_INTERRUPTED"] = 2] = "ES_INTERRUPTED";
})(EmoteState = exports.EmoteState || (exports.EmoteState = {}));
function createBasePBAvatarEmoteCommand() {
    return { emoteUrn: "", loop: false, timestamp: 0, mask: undefined, state: undefined };
}
/**
 * @public
 */
var PBAvatarEmoteCommand;
(function (PBAvatarEmoteCommand) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.emoteUrn !== "") {
            writer.uint32(10).string(message.emoteUrn);
        }
        if (message.loop === true) {
            writer.uint32(16).bool(message.loop);
        }
        if (message.timestamp !== 0) {
            writer.uint32(24).uint32(message.timestamp);
        }
        if (message.mask !== undefined) {
            writer.uint32(32).int32(message.mask);
        }
        if (message.state !== undefined) {
            writer.uint32(40).int32(message.state);
        }
        return writer;
    }
    PBAvatarEmoteCommand.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAvatarEmoteCommand();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.emoteUrn = reader.string();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.loop = reader.bool();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.timestamp = reader.uint32();
                    continue;
                case 4:
                    if (tag !== 32) {
                        break;
                    }
                    message.mask = reader.int32();
                    continue;
                case 5:
                    if (tag !== 40) {
                        break;
                    }
                    message.state = reader.int32();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAvatarEmoteCommand.decode = decode;
})(PBAvatarEmoteCommand = exports.PBAvatarEmoteCommand || (exports.PBAvatarEmoteCommand = {}));
