import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export interface Color3 {
    r: number;
    g: number;
    b: number;
}
/**
 * @public
 */
export interface Color4 {
    r: number;
    g: number;
    b: number;
    a: number;
}
/** A range of Color4 values. Randomized or lerped between start and end. */
/**
 * @public
 */
export interface ColorRange {
    start: Color4 | undefined;
    end: Color4 | undefined;
}
/**
 * @public
 */
export declare namespace Color3 {
    function encode(message: Color3, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): Color3;
}
/**
 * @public
 */
export declare namespace Color4 {
    function encode(message: Color4, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): Color4;
}
/**
 * @public
 */
export declare namespace ColorRange {
    function encode(message: ColorRange, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): ColorRange;
}
