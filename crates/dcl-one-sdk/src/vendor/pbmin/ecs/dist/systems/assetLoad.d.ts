import { DeepReadonlyObject, Entity } from '../engine';
import { PBAssetLoadLoadingState } from '../components';
/**
 * @public
 */
export type AssetLoadLoadingStateSystemCallback = (event: DeepReadonlyObject<PBAssetLoadLoadingState>) => void;
/**
 * @public
 */
export interface AssetLoadLoadingStateSystem {
    removeAssetLoadLoadingStateEntity(entity: Entity): void;
    registerAssetLoadLoadingStateEntity(entity: Entity, callback: AssetLoadLoadingStateSystemCallback): void;
}
