import _m0 from "protobufjs/minimal";
/** Controls the direction for animated skybox transitions */
/**
 * @public
 */
export declare const enum TransitionMode {
    /** TM_FORWARD - transitions forward (default) */
    TM_FORWARD = 0,
    /** TM_BACKWARD - transitions backward */
    TM_BACKWARD = 1
}
/**
 * The SkyboxTime component allows controlling the time of day for the skybox,
 * affecting the lighting and appearance of the sky in the scene.
 */
/**
 * @public
 */
export interface PBSkyboxTime {
    /** fixed time of day, represented as a number of seconds since the start of the day, where 0 is 00:00hs, 43200 is 12:00hs and 86400 is 24:00hs */
    fixedTime: number;
    /** default = TransitionMode.TM_FORWARD, controls the direction of time transitions */
    transitionMode?: TransitionMode | undefined;
}
/**
 * @public
 */
export declare namespace PBSkyboxTime {
    function encode(message: PBSkyboxTime, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBSkyboxTime;
}
