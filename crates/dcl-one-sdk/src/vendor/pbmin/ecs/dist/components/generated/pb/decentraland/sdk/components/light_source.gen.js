/* eslint-disable */
import _m0 from "protobufjs/minimal";
import { Color3 } from "../../common/colors.gen";
import { TextureUnion } from "../../common/texture.gen";
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBLightSource() {
    return {
        active: undefined,
        color: undefined,
        intensity: undefined,
        range: undefined,
        shadow: undefined,
        shadowMaskTexture: undefined,
        type: undefined,
    };
}
/**
 * @public
 */
export var PBLightSource;
(function (PBLightSource) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.active !== undefined) {
            writer.uint32(8).bool(message.active);
        }
        if (message.color !== undefined) {
            Color3.encode(message.color, writer.uint32(18).fork()).ldelim();
        }
        if (message.intensity !== undefined) {
            writer.uint32(29).float(message.intensity);
        }
        if (message.range !== undefined) {
            writer.uint32(37).float(message.range);
        }
        if (message.shadow !== undefined) {
            writer.uint32(40).bool(message.shadow);
        }
        if (message.shadowMaskTexture !== undefined) {
            TextureUnion.encode(message.shadowMaskTexture, writer.uint32(50).fork()).ldelim();
        }
        switch (message.type?.$case) {
            case "point":
                PBLightSource_Point.encode(message.type.point, writer.uint32(58).fork()).ldelim();
                break;
            case "spot":
                PBLightSource_Spot.encode(message.type.spot, writer.uint32(66).fork()).ldelim();
                break;
        }
        return writer;
    }
    PBLightSource.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBLightSource();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.active = reader.bool();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.color = Color3.decode(reader, reader.uint32());
                    continue;
                case 3:
                    if (tag !== 29) {
                        break;
                    }
                    message.intensity = reader.float();
                    continue;
                case 4:
                    if (tag !== 37) {
                        break;
                    }
                    message.range = reader.float();
                    continue;
                case 5:
                    if (tag !== 40) {
                        break;
                    }
                    message.shadow = reader.bool();
                    continue;
                case 6:
                    if (tag !== 50) {
                        break;
                    }
                    message.shadowMaskTexture = TextureUnion.decode(reader, reader.uint32());
                    continue;
                case 7:
                    if (tag !== 58) {
                        break;
                    }
                    message.type = { $case: "point", point: PBLightSource_Point.decode(reader, reader.uint32()) };
                    continue;
                case 8:
                    if (tag !== 66) {
                        break;
                    }
                    message.type = { $case: "spot", spot: PBLightSource_Spot.decode(reader, reader.uint32()) };
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBLightSource.decode = decode;
})(PBLightSource || (PBLightSource = {}));
function createBasePBLightSource_Point() {
    return {};
}
/**
 * @public
 */
export var PBLightSource_Point;
(function (PBLightSource_Point) {
    function encode(_, writer = _m0.Writer.create()) {
        return writer;
    }
    PBLightSource_Point.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBLightSource_Point();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBLightSource_Point.decode = decode;
})(PBLightSource_Point || (PBLightSource_Point = {}));
function createBasePBLightSource_Spot() {
    return { innerAngle: undefined, outerAngle: undefined };
}
/**
 * @public
 */
export var PBLightSource_Spot;
(function (PBLightSource_Spot) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.innerAngle !== undefined) {
            writer.uint32(77).float(message.innerAngle);
        }
        if (message.outerAngle !== undefined) {
            writer.uint32(85).float(message.outerAngle);
        }
        return writer;
    }
    PBLightSource_Spot.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBLightSource_Spot();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 9:
                    if (tag !== 77) {
                        break;
                    }
                    message.innerAngle = reader.float();
                    continue;
                case 10:
                    if (tag !== 85) {
                        break;
                    }
                    message.outerAngle = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBLightSource_Spot.decode = decode;
})(PBLightSource_Spot || (PBLightSource_Spot = {}));
