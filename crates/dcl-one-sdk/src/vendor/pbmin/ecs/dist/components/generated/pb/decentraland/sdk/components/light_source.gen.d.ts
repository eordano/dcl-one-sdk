import _m0 from "protobufjs/minimal";
import { Color3 } from "../../common/colors.gen";
import { TextureUnion } from "../../common/texture.gen";
/**
 * @public
 */
export interface PBLightSource {
    /** default = true, whether the lightSource is active or not. */
    active?: boolean | undefined;
    /** default = white, the tint of the light, in RGB format where each component is a floating point value with a range from 0 to 1. */
    color?: Color3 | undefined;
    /** default = 16000, light intensity expressed in candels (lumens/m^2 at 1 m distance, or lumens divided by 4*pi) */
    intensity?: number | undefined;
    /** default = -1, how far the light travels, expressed in meters. If negative will be computed automatically as pow(intensity, 0.25) */
    range?: number | undefined;
    /** default = false, whether the light casts shadows or not. */
    shadow?: boolean | undefined;
    /** Texture mask through which shadows are cast to simulate caustics, soft shadows, and light shapes such as light entering from a window. */
    shadowMaskTexture?: TextureUnion | undefined;
    type?: {
        $case: "point";
        point: PBLightSource_Point;
    } | {
        $case: "spot";
        spot: PBLightSource_Spot;
    } | undefined;
}
/**
 * @public
 */
export interface PBLightSource_Point {
}
/**
 * @public
 */
export interface PBLightSource_Spot {
    /** default = 21.8. Inner angle can't be higher than outer angle, otherwise will default to same value. Min value is 0. Max value is 179. */
    innerAngle?: number | undefined;
    /** default = 30. Outer angle can't be lower than inner angle, otherwise will inner angle will be set to same value. Max value is 179. */
    outerAngle?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBLightSource {
    function encode(message: PBLightSource, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBLightSource;
}
/**
 * @public
 */
export declare namespace PBLightSource_Point {
    function encode(_: PBLightSource_Point, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBLightSource_Point;
}
/**
 * @public
 */
export declare namespace PBLightSource_Spot {
    function encode(message: PBLightSource_Spot, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBLightSource_Spot;
}
