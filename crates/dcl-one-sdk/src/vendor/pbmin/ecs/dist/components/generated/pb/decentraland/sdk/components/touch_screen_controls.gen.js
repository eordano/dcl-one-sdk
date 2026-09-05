/* eslint-disable */
import _m0 from "protobufjs/minimal";
import { TextureUnion } from "../../common/texture.gen";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBTouchScreenControls() {
    return { touchInputs: [], mainAction: undefined, hideJoystick: false, hideCrosshair: false };
}
/**
 * @public
 */
export var PBTouchScreenControls;
(function (PBTouchScreenControls) {
    function encode(message, writer = _m0.Writer.create()) {
        for (const v of message.touchInputs) {
            PBTouchScreenControls_TouchInput.encode(v, writer.uint32(10).fork()).ldelim();
        }
        if (message.mainAction !== undefined) {
            writer.uint32(16).int32(message.mainAction);
        }
        if (message.hideJoystick === true) {
            writer.uint32(24).bool(message.hideJoystick);
        }
        if (message.hideCrosshair === true) {
            writer.uint32(32).bool(message.hideCrosshair);
        }
        return writer;
    }
    PBTouchScreenControls.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBTouchScreenControls();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.touchInputs.push(PBTouchScreenControls_TouchInput.decode(reader, reader.uint32()));
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.mainAction = reader.int32();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.hideJoystick = reader.bool();
                    continue;
                case 4:
                    if (tag !== 32) {
                        break;
                    }
                    message.hideCrosshair = reader.bool();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBTouchScreenControls.decode = decode;
})(PBTouchScreenControls || (PBTouchScreenControls = {}));
function createBasePBTouchScreenControls_TouchInput() {
    return { inputAction: 0, hide: false, icon: undefined };
}
/**
 * @public
 */
export var PBTouchScreenControls_TouchInput;
(function (PBTouchScreenControls_TouchInput) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.inputAction !== 0) {
            writer.uint32(8).int32(message.inputAction);
        }
        if (message.hide === true) {
            writer.uint32(16).bool(message.hide);
        }
        if (message.icon !== undefined) {
            TextureUnion.encode(message.icon, writer.uint32(26).fork()).ldelim();
        }
        return writer;
    }
    PBTouchScreenControls_TouchInput.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBTouchScreenControls_TouchInput();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.inputAction = reader.int32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.hide = reader.bool();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.icon = TextureUnion.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBTouchScreenControls_TouchInput.decode = decode;
})(PBTouchScreenControls_TouchInput || (PBTouchScreenControls_TouchInput = {}));
