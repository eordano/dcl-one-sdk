import _m0 from "protobufjs/minimal";
/** AssetLoad component allows an entity to request the pre-loading of one or more assets by the renderer. */
/**
 * @public
 */
export interface PBAssetLoad {
    assets: string[];
}
/**
 * @public
 */
export declare namespace PBAssetLoad {
    function encode(message: PBAssetLoad, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAssetLoad;
}
