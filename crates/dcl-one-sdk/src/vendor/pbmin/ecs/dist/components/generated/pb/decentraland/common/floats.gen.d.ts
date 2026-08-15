import _m0 from "protobufjs/minimal";
/** A range of float values. Randomized or lerped between start and end. */
/**
 * @public
 */
export interface FloatRange {
    start: number;
    end: number;
}
/**
 * @public
 */
export declare namespace FloatRange {
    function encode(message: FloatRange, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): FloatRange;
}
