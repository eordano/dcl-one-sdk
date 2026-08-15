/**
 * @alpha * This file initialization is an alpha one. This is based on the old-ecs
 * init and it'll be changing.
 */
import { IEngine } from '../../engine';
import { Task } from '../../systems/async-task';
import { PointerEventsSystem } from '../../systems/events';
import { IInputSystem } from './../../engine/input';
import { RaycastSystem } from '../../systems/raycast';
import { VideoEventsSystem } from '../../systems/videoEvents';
import { AssetLoadLoadingStateSystem } from '../../systems/assetLoad';
import { TweenSystem } from '../../systems/tween';
import { TriggerAreaEventsSystem } from '../../systems/triggerArea';
import { createTimers, Timers } from '../helpers/timers';
import { PhysicsSystem } from '../../systems/physics';
/**
 * @public
 * The engine is the part of the scene that sits in the middle and manages all of the other parts.
 * It determines what entities are rendered and how players interact with them.
 * It also coordinates what functions from systems are executed and when.
 *
 * @example
 * import { engine } from '@dcl/sdk/ecs'
 * const entity = engine.addEntity()
 * engine.addSystem(someSystemFunction)
 *
 */
export declare const engine: IEngine;
/**
 * @public
 * Input system manager. Check for button events
 * @example
 * inputSystem.isTriggered: Returns true if an input action ocurred since the last tick.
 * inputSystem.isPressed: Returns true if an input is currently being pressed down. It will return true on every tick until the button goes up again.
 * inputSystem.getInputCommand: Returns an object with data about the input action.
 */
export declare const inputSystem: IInputSystem;
export { IInputSystem };
/**
 * @public
 * Register callback functions to a particular entity on input events.
 */
export declare const pointerEventsSystem: PointerEventsSystem;
export { PointerEventsSystem };
/**
 * @public
 * Register callback functions to a particular entity on raycast results.
 */
export declare const raycastSystem: RaycastSystem;
export { RaycastSystem };
/**
 * @public
 * Register callback functions to a particular entity on video events.
 */
export declare const videoEventsSystem: VideoEventsSystem;
export { VideoEventsSystem };
/**
 * @public
 * Register callback functions to a particular entity on asset pre-load events.
 */
export declare const assetLoadLoadingStateSystem: AssetLoadLoadingStateSystem;
export { AssetLoadLoadingStateSystem };
/**
 * @public
 * Register callback functions to a particular entity on tween events.
 */
export declare const tweenSystem: TweenSystem;
export { TweenSystem };
/**
 * @public
 * Register callback functions for trigger area results.
 */
export declare const triggerAreaEventsSystem: TriggerAreaEventsSystem;
export { TriggerAreaEventsSystem };
/**
 * @public
 * Timer utilities for delayed and repeated execution.
 */
export declare const timers: Timers;
export { Timers, createTimers };
/**
 * @public
 * Physics helpers for applying impulses and forces to the player.
 */
export declare const Physics: PhysicsSystem;
export { PhysicsSystem };
/**
 * @public
 * Runs an async function
 */
export declare const executeTask: (task: Task<unknown>) => void;
/**
 * @public
 */
export type { Task };
