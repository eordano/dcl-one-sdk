import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export declare const enum PBAudioAnalysisMode {
    MODE_RAW = 0,
    MODE_LOGARITHMIC = 1
}
/**
 * @public
 */
export interface PBAudioAnalysis {
    /** Parameters section */
    mode: PBAudioAnalysisMode;
    /** Used only when mode == MODE_LOGARITHMIC */
    amplitudeGain?: number | undefined;
    /** End when mode == MODE_LOGARITHMIC */
    bandsGain?: number | undefined;
    /** Result section */
    amplitude: number;
    /** Protobuf doesn't support fixed arrays -> 8 band fields */
    band0: number;
    band1: number;
    band2: number;
    band3: number;
    band4: number;
    band5: number;
    band6: number;
    band7: number;
}
/**
 * @public
 */
export declare namespace PBAudioAnalysis {
    function encode(message: PBAudioAnalysis, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAudioAnalysis;
}
