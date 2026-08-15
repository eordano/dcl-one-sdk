/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * @public
 */
export var TriggerAreaMeshType;
(function (TriggerAreaMeshType) {
    TriggerAreaMeshType[TriggerAreaMeshType["TAMT_BOX"] = 0] = "TAMT_BOX";
    TriggerAreaMeshType[TriggerAreaMeshType["TAMT_SPHERE"] = 1] = "TAMT_SPHERE";
})(TriggerAreaMeshType || (TriggerAreaMeshType = {}));
function createBasePBTriggerArea() {
    return { mesh: undefined, collisionMask: undefined };
}
/**
 * @public
 */
export var PBTriggerArea;
(function (PBTriggerArea) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.mesh !== undefined) {
            writer.uint32(8).int32(message.mesh);
        }
        if (message.collisionMask !== undefined) {
            writer.uint32(16).uint32(message.collisionMask);
        }
        return writer;
    }
    PBTriggerArea.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBTriggerArea();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.mesh = reader.int32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.collisionMask = reader.uint32();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBTriggerArea.decode = decode;
})(PBTriggerArea || (PBTriggerArea = {}));
