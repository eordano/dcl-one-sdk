import { Entity, IEngine, LastWriteWinElementSetComponentDefinition } from '../../engine';
import { ColliderLayer, PBTriggerArea } from '../generated/index.gen';
/**
 * @public
 */
export interface TriggerAreaComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBTriggerArea> {
    /**
     * @public
     * Set a box in the MeshCollider component
     * @param entity - entity to create or replace the TriggerArea component
     * @param collisionMask - the collision layers mask for the trigger to react, default: CL_PLAYER
     */
    setBox(entity: Entity, collisionMask?: ColliderLayer | ColliderLayer[]): void;
    /**
     * @public
     * Set a sphere in the MeshCollider component
     * @param entity - entity to create or replace the TriggerArea component
     * @param collisionMask - the collision layers mask for the trigger to react, default: CL_PLAYER
     */
    setSphere(entity: Entity, collisionMask?: ColliderLayer | ColliderLayer[]): void;
}
export declare function defineTriggerAreaComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): TriggerAreaComponentDefinitionExtended;
