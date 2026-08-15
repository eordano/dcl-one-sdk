"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBGltfNodeModifiers_GltfNodeModifier = exports.PBGltfNodeModifiers = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const material_gen_1 = require("./material.gen");
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBGltfNodeModifiers() {
    return { modifiers: [] };
}
/**
 * @public
 */
var PBGltfNodeModifiers;
(function (PBGltfNodeModifiers) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.modifiers) {
            PBGltfNodeModifiers_GltfNodeModifier.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    PBGltfNodeModifiers.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBGltfNodeModifiers();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.modifiers.push(PBGltfNodeModifiers_GltfNodeModifier.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBGltfNodeModifiers.decode = decode;
})(PBGltfNodeModifiers = exports.PBGltfNodeModifiers || (exports.PBGltfNodeModifiers = {}));
function createBasePBGltfNodeModifiers_GltfNodeModifier() {
    return { path: "", castShadows: undefined, material: undefined };
}
/**
 * @public
 */
var PBGltfNodeModifiers_GltfNodeModifier;
(function (PBGltfNodeModifiers_GltfNodeModifier) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.castShadows !== undefined) {
            writer.uint32(16).bool(message.castShadows);
        }
        if (message.material !== undefined) {
            material_gen_1.PBMaterial.encode(message.material, writer.uint32(26).fork()).ldelim();
        }
        return writer;
    }
    PBGltfNodeModifiers_GltfNodeModifier.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBGltfNodeModifiers_GltfNodeModifier();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.castShadows = reader.bool();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.material = material_gen_1.PBMaterial.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBGltfNodeModifiers_GltfNodeModifier.decode = decode;
})(PBGltfNodeModifiers_GltfNodeModifier = exports.PBGltfNodeModifiers_GltfNodeModifier || (exports.PBGltfNodeModifiers_GltfNodeModifier = {}));
