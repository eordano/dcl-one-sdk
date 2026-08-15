import { InputAction } from '../components/generated/pb/decentraland/sdk/components/common/input_action.gen';
import { PBPointerEventsResult } from '../components/generated/pb/decentraland/sdk/components/pointer_events_result.gen';
import { IEngine } from '../engine/types';
import { Entity } from '../engine/entity';
import { IInputSystem } from '../engine/input';
/**
 * @public
 */
export type EventSystemCallback = (event: PBPointerEventsResult) => void;
/**
 * @public
 */
export type EventSystemOptions = {
    button: InputAction;
    hoverText?: string;
    maxDistance?: number;
    showFeedback?: boolean;
    showHighlight?: boolean;
    maxPlayerDistance?: number;
    priority?: number;
};
export declare const getDefaultOpts: (opts?: Partial<EventSystemOptions>) => EventSystemOptions;
/**
 * @public
 */
export interface PointerEventsSystem {
    /**
     * @public
     * Remove the callback for onPointerDown event
     * @param entity - Entity where the callback was attached
     */
    removeOnPointerDown(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onPointerUp event
     * @param entity - Entity where the callback was attached
     */
    removeOnPointerUp(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onPointerHoverEnter event
     * @param entity - Entity where the callback was attached
     */
    removeOnPointerHoverEnter(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onPointerHoverLeave event
     * @param entity - Entity where the callback was attached
     */
    removeOnPointerHoverLeave(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onProximityDown event
     * @param entity - Entity where the callback was attached
     */
    removeOnProximityDown(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onProximityUp event
     * @param entity - Entity where the callback was attached
     */
    removeOnProximityUp(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onProximityEnter event
     * @param entity - Entity where the callback was attached
     */
    removeOnProximityEnter(entity: Entity): void;
    /**
     * @public
     * Remove the callback for onProximityLeave event
     * @param entity - Entity where the callback was attached
     */
    removeOnProximityLeave(entity: Entity): void;
    /**
     * @public
     * Execute callback when the user press the InputButton pointing at the entity
     * @param pointerData - Entity to attach the callback, Opts to trigger Feedback and Button
     * @param cb - Function to execute when click fires
     */
    onPointerDown(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @deprecated Use onPointerDown with (pointerData, cb)
     * @param entity - Entity to attach the callback
     * @param cb - Function to execute when click fires
     * @param opts - Opts to trigger Feedback and Button
     */
    onPointerDown(entity: Entity, cb: EventSystemCallback, opts?: Partial<EventSystemOptions>): void;
    /**
     * @public
     * Execute callback when the user releases the InputButton pointing at the entity
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when click fires
     */
    onPointerUp(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @deprecated Use onPointerUp with (pointerData, cb)
     * @param entity - Entity to attach the callback
     * @param cb - Function to execute when click fires
     * @param opts - Opts to trigger Feedback and Button
     */
    onPointerUp(entity: Entity, cb: EventSystemCallback, opts?: Partial<EventSystemOptions>): void;
    /**
     * @public
     * Execute callback when the user place the pointer over the entity
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when click fires
     */
    onPointerHoverEnter(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @public
     * Execute callback when the user take the pointer out of the entity
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when click fires
     */
    onPointerHoverLeave(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @public
     * Execute callback when the user presses the proximity button on the entity
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when click fires
     */
    onProximityDown(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @public
     * Execute callback when the user releases the proximity button on the entity
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when event fires
     */
    onProximityUp(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @public
     * Execute callback when the entity enters the proximity zone of the user
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when event fires
     */
    onProximityEnter(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
    /**
     * @public
     * Execute callback when the entity leaves the proximity zone of the user
     * @param pointerData - Entity to attach the callback - Opts to trigger Feedback and Button
     * @param cb - Function to execute when event fires
     */
    onProximityLeave(pointerData: {
        entity: Entity;
        opts?: Partial<EventSystemOptions>;
    }, cb: EventSystemCallback): void;
}
/**
 * @public
 * ___DO NOT USE___ use pointerEventsSystem instead
 */
export declare function createPointerEventsSystem(engine: IEngine, inputSystem: IInputSystem): PointerEventsSystem;
