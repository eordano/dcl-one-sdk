import _m0 from "protobufjs/minimal";
import { LoadingState } from "./common/loading_state.gen";
/**
 * AssetLoadLoadingState is set by the engine and provides information about
 * the current state of the AssetLoad of an entity.
 * The renderer appends a new object of this in each command, there can be many commands per frames
 */
/**
 * @public
 */
export interface PBAssetLoadLoadingState {
    /** current loading state */
    currentState: LoadingState;
    /** the asset being loaded (asset's scene path) */
    asset: string;
    /** monotonic counter */
    timestamp: number;
}
/**
 * @public
 */
export declare namespace PBAssetLoadLoadingState {
    function encode(message: PBAssetLoadLoadingState, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAssetLoadLoadingState;
}
