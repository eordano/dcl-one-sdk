import _m0 from "protobufjs/minimal";
import { Quaternion, Vector3 } from "../../common/vectors.gen";
/**
 * @public
 */
export declare const enum TriggerAreaEventType {
    TAET_ENTER = 0,
    TAET_STAY = 1,
    TAET_EXIT = 2
}
/**
 * The PBTriggerAreaResult component is used to transport the trigger collision event data for the PBTriggerArea component.
 * ADR: https://github.com/decentraland/adr/blob/2b30a5e2b4f359a7c22a68fb827db282f6e5f887/content/ADR-258-trigger-areas.md
 */
/**
 * @public
 */
export interface PBTriggerAreaResult {
    /** The entity that was triggered (this is the entity that owns the trigger area) */
    triggeredEntity: number;
    /** The position of the triggered entity at the time of the trigger */
    triggeredEntityPosition: Vector3 | undefined;
    /** The rotation of the triggered entity at the time of the trigger */
    triggeredEntityRotation: Quaternion | undefined;
    /** The state of the trigger event (ENTER, STAY, EXIT) */
    eventType: TriggerAreaEventType;
    /** The timestamp of the trigger event */
    timestamp: number;
    trigger: PBTriggerAreaResult_Trigger | undefined;
}
/** Trigger data object */
/**
 * @public
 */
export interface PBTriggerAreaResult_Trigger {
    /** The entity that triggered the Trigger Area */
    entity: number;
    /** The collision layermask of the entity that triggered the Trigger Area */
    layers: number;
    /** The position of the entity that triggered the trigger */
    position: Vector3 | undefined;
    /** The rotation of the entity that triggered the trigger */
    rotation: Quaternion | undefined;
    /** The scale of the entity that triggered the trigger */
    scale: Vector3 | undefined;
}
/**
 * @public
 */
export declare namespace PBTriggerAreaResult {
    function encode(message: PBTriggerAreaResult, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTriggerAreaResult;
}
/**
 * @public
 */
export declare namespace PBTriggerAreaResult_Trigger {
    function encode(message: PBTriggerAreaResult_Trigger, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTriggerAreaResult_Trigger;
}
