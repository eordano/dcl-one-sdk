"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBTriggerAreaResult_Trigger = exports.PBTriggerAreaResult = exports.TriggerAreaEventType = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const vectors_gen_1 = require("../../common/vectors.gen");
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * @public
 */
var TriggerAreaEventType;
(function (TriggerAreaEventType) {
    TriggerAreaEventType[TriggerAreaEventType["TAET_ENTER"] = 0] = "TAET_ENTER";
    TriggerAreaEventType[TriggerAreaEventType["TAET_STAY"] = 1] = "TAET_STAY";
    TriggerAreaEventType[TriggerAreaEventType["TAET_EXIT"] = 2] = "TAET_EXIT";
})(TriggerAreaEventType = exports.TriggerAreaEventType || (exports.TriggerAreaEventType = {}));
function createBasePBTriggerAreaResult() {
    return {
        triggeredEntity: 0,
        triggeredEntityPosition: undefined,
        triggeredEntityRotation: undefined,
        eventType: 0,
        timestamp: 0,
        trigger: undefined,
    };
}
/**
 * @public
 */
var PBTriggerAreaResult;
(function (PBTriggerAreaResult) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.triggeredEntity !== 0) {
            writer.uint32(8).uint32(message.triggeredEntity);
        }
        if (message.triggeredEntityPosition !== undefined) {
            vectors_gen_1.Vector3.encode(message.triggeredEntityPosition, writer.uint32(18).fork()).ldelim();
        }
        if (message.triggeredEntityRotation !== undefined) {
            vectors_gen_1.Quaternion.encode(message.triggeredEntityRotation, writer.uint32(26).fork()).ldelim();
        }
        if (message.eventType !== 0) {
            writer.uint32(32).int32(message.eventType);
        }
        if (message.timestamp !== 0) {
            writer.uint32(40).uint32(message.timestamp);
        }
        if (message.trigger !== undefined) {
            PBTriggerAreaResult_Trigger.encode(message.trigger, writer.uint32(50).fork()).ldelim();
        }
        return writer;
    }
    PBTriggerAreaResult.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBTriggerAreaResult();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.triggeredEntity = reader.uint32();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.triggeredEntityPosition = vectors_gen_1.Vector3.decode(reader, reader.uint32());
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.triggeredEntityRotation = vectors_gen_1.Quaternion.decode(reader, reader.uint32());
                    continue;
                case 4:
                    if (tag !== 32) {
                        break;
                    }
                    message.eventType = reader.int32();
                    continue;
                case 5:
                    if (tag !== 40) {
                        break;
                    }
                    message.timestamp = reader.uint32();
                    continue;
                case 6:
                    if (tag !== 50) {
                        break;
                    }
                    message.trigger = PBTriggerAreaResult_Trigger.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBTriggerAreaResult.decode = decode;
})(PBTriggerAreaResult = exports.PBTriggerAreaResult || (exports.PBTriggerAreaResult = {}));
function createBasePBTriggerAreaResult_Trigger() {
    return { entity: 0, layers: 0, position: undefined, rotation: undefined, scale: undefined };
}
/**
 * @public
 */
var PBTriggerAreaResult_Trigger;
(function (PBTriggerAreaResult_Trigger) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.entity !== 0) {
            writer.uint32(8).uint32(message.entity);
        }
        if (message.layers !== 0) {
            writer.uint32(16).uint32(message.layers);
        }
        if (message.position !== undefined) {
            vectors_gen_1.Vector3.encode(message.position, writer.uint32(26).fork()).ldelim();
        }
        if (message.rotation !== undefined) {
            vectors_gen_1.Quaternion.encode(message.rotation, writer.uint32(34).fork()).ldelim();
        }
        if (message.scale !== undefined) {
            vectors_gen_1.Vector3.encode(message.scale, writer.uint32(42).fork()).ldelim();
        }
        return writer;
    }
    PBTriggerAreaResult_Trigger.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBTriggerAreaResult_Trigger();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.entity = reader.uint32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.layers = reader.uint32();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.position = vectors_gen_1.Vector3.decode(reader, reader.uint32());
                    continue;
                case 4:
                    if (tag !== 34) {
                        break;
                    }
                    message.rotation = vectors_gen_1.Quaternion.decode(reader, reader.uint32());
                    continue;
                case 5:
                    if (tag !== 42) {
                        break;
                    }
                    message.scale = vectors_gen_1.Vector3.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBTriggerAreaResult_Trigger.decode = decode;
})(PBTriggerAreaResult_Trigger = exports.PBTriggerAreaResult_Trigger || (exports.PBTriggerAreaResult_Trigger = {}));
