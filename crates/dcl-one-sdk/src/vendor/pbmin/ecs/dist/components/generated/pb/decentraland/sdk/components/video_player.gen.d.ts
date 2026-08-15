import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export interface PBVideoPlayer {
    /** which file to load */
    src: string;
    /** default true */
    playing?: boolean | undefined;
    /** default 0.0 */
    position?: number | undefined;
    /** default 1.0 */
    volume?: number | undefined;
    /** default 1.0 */
    playbackRate?: number | undefined;
    /** default false */
    loop?: boolean | undefined;
    /**
     * either the audio will be global or spatial (default: false)
     * global: plays the same way for every listener. It is not affected by distance, direction, or position.
     * spatial: changes depending on where the listener is relative to the sound source
     */
    spatial?: boolean | undefined;
    /** Within the min distance the audio will cease to grow louder in volume (default: 0) */
    spatialMinDistance?: number | undefined;
    /** The distance where sound either becomes inaudible or stops attenuation (default: 60) */
    spatialMaxDistance?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBVideoPlayer {
    function encode(message: PBVideoPlayer, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBVideoPlayer;
}
