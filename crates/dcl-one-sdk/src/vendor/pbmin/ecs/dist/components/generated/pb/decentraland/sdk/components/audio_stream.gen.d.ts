import _m0 from "protobufjs/minimal";
/**
 * The AudioStream component can play external audio clips given a URL, streaming it in real-time.
 *
 * Despite being attached to a particular entity, the sound is not affected by its position.
 */
/**
 * @public
 */
export interface PBAudioStream {
    /** whether the clip is currently playing */
    playing?: boolean | undefined;
    /** the audio volume (default: 1.0) */
    volume?: number | undefined;
    /** the audio stream HTTP URL */
    url: string;
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
export declare namespace PBAudioStream {
    function encode(message: PBAudioStream, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAudioStream;
}
