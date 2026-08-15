import { DeepReadonlyObject, Entity } from '../engine';
import { PBTriggerAreaResult } from '../components';
/**
 * @public
 */
export type TriggerAreaEventSystemCallback = (result: DeepReadonlyObject<PBTriggerAreaResult>) => void;
/**
 * @public
 */
export interface TriggerAreaEventsSystem {
    /**
     * @public
     * Execute callback when an entity enters the Trigger Area
     * @param entity - The entity that already has the TriggerArea component
     * @param cb - Function to execute the 'Enter' type of result is detected
     */
    onTriggerEnter(entity: Entity, cb: TriggerAreaEventSystemCallback): void;
    /**
     * @public
     * Remove the callback for Trigger Area 'Enter' type of result
     * @param entity - Entity where the Trigger Area was attached
     */
    removeOnTriggerEnter(entity: Entity): void;
    /**
     * @public
     * Execute callback when an entity stays in the Trigger Area
     * @param entity - The entity that already has the TriggerArea component
     * @param cb - Function to execute the 'Stay' type of result is detected
     *
     * Note: stay callbacks are synthesized by the SDK on every tick between a wire ENTER and a wire EXIT.
     * Wire-level TAET_STAY events (still emitted by legacy Explorers) are ignored entirely — they neither
     * fire callbacks nor mutate state. The SDK is the sole source of onTriggerStay dispatches, driven
     * from the ENTER/EXIT state machine.
     */
    onTriggerStay(entity: Entity, cb: TriggerAreaEventSystemCallback): void;
    /**
     * @public
     * Remove the callback for Trigger Area 'Stay' type of result
     * @param entity - Entity where the Trigger Area was attached
     */
    removeOnTriggerStay(entity: Entity): void;
    /**
     * @public
     * Execute callback when an entity exits the Trigger Area
     * @param entity - The entity that already has the TriggerArea component
     * @param cb - Function to execute the 'Exit' type of result is detected
     */
    onTriggerExit(entity: Entity, cb: TriggerAreaEventSystemCallback): void;
    /**
     * @public
     * Remove the callback for Trigger Area 'Exit' type of result
     * @param entity - Entity where the Trigger Area was attached
     */
    removeOnTriggerExit(entity: Entity): void;
}
