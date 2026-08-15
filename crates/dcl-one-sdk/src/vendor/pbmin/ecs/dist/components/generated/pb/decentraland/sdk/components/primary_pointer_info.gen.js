/* eslint-disable */
import _m0 from "protobufjs/minimal";
import { Vector2, Vector3 } from "../../common/vectors.gen";
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * PointerType enumerates the different input devices that can be used for pointer interactions.
 * Each type has specific characteristics and use cases in the virtual world.
 */
/**
 * @public
 */
export var PointerType;
(function (PointerType) {
    /** POT_NONE - No pointer input */
    PointerType[PointerType["POT_NONE"] = 0] = "POT_NONE";
    /** POT_MOUSE - Traditional mouse input */
    PointerType[PointerType["POT_MOUSE"] = 1] = "POT_MOUSE";
})(PointerType || (PointerType = {}));
function createBasePBPrimaryPointerInfo() {
    return { pointerType: undefined, screenCoordinates: undefined, screenDelta: undefined, worldRayDirection: undefined };
}
/**
 * @public
 */
export var PBPrimaryPointerInfo;
(function (PBPrimaryPointerInfo) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.pointerType !== undefined) {
            writer.uint32(8).int32(message.pointerType);
        }
        if (message.screenCoordinates !== undefined) {
            Vector2.encode(message.screenCoordinates, writer.uint32(18).fork()).ldelim();
        }
        if (message.screenDelta !== undefined) {
            Vector2.encode(message.screenDelta, writer.uint32(26).fork()).ldelim();
        }
        if (message.worldRayDirection !== undefined) {
            Vector3.encode(message.worldRayDirection, writer.uint32(34).fork()).ldelim();
        }
        return writer;
    }
    PBPrimaryPointerInfo.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
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
                    message.screenCoordinates = Vector2.decode(reader, reader.uint32());
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.screenDelta = Vector2.decode(reader, reader.uint32());
                    continue;
                case 4:
                    if (tag !== 34) {
                        break;
                    }
                    message.worldRayDirection = Vector3.decode(reader, reader.uint32());
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
})(PBPrimaryPointerInfo || (PBPrimaryPointerInfo = {}));
