"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBPhysicsCombinedImpulse = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const vectors_gen_1 = require("../../common/vectors.gen");
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBPhysicsCombinedImpulse() {
    return { vector: undefined, eventId: 0 };
}
/**
 * @public
 */
var PBPhysicsCombinedImpulse;
(function (PBPhysicsCombinedImpulse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.vector !== undefined) {
            vectors_gen_1.Vector3.encode(message.vector, writer.uint32(10).fork()).ldelim();
        }
        if (message.eventId !== 0) {
            writer.uint32(16).uint32(message.eventId);
        }
        return writer;
    }
    PBPhysicsCombinedImpulse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBPhysicsCombinedImpulse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.vector = vectors_gen_1.Vector3.decode(reader, reader.uint32());
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.eventId = reader.uint32();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBPhysicsCombinedImpulse.decode = decode;
})(PBPhysicsCombinedImpulse = exports.PBPhysicsCombinedImpulse || (exports.PBPhysicsCombinedImpulse = {}));
