/* eslint-disable */
import _m0 from "protobufjs/minimal";
import { Vector3 } from "../../common/vectors.gen";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBPhysicsCombinedForce() {
    return { vector: undefined };
}
/**
 * @public
 */
export var PBPhysicsCombinedForce;
(function (PBPhysicsCombinedForce) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.vector !== undefined) {
            Vector3.encode(message.vector, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    PBPhysicsCombinedForce.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBPhysicsCombinedForce();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.vector = Vector3.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBPhysicsCombinedForce.decode = decode;
})(PBPhysicsCombinedForce || (PBPhysicsCombinedForce = {}));
