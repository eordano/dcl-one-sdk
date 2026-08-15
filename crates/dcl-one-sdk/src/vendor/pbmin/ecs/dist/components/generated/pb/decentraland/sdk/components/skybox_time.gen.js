/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
/** Controls the direction for animated skybox transitions */
/**
 * @public
 */
export var TransitionMode;
(function (TransitionMode) {
    /** TM_FORWARD - transitions forward (default) */
    TransitionMode[TransitionMode["TM_FORWARD"] = 0] = "TM_FORWARD";
    /** TM_BACKWARD - transitions backward */
    TransitionMode[TransitionMode["TM_BACKWARD"] = 1] = "TM_BACKWARD";
})(TransitionMode || (TransitionMode = {}));
function createBasePBSkyboxTime() {
    return { fixedTime: 0, transitionMode: undefined };
}
/**
 * @public
 */
export var PBSkyboxTime;
(function (PBSkyboxTime) {
    function encode(message, writer = _m0.Writer.create()) {
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
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
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
})(PBSkyboxTime || (PBSkyboxTime = {}));
