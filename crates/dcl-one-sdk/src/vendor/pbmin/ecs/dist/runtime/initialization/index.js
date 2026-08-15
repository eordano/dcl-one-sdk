/**
 * @alpha * This file initialization is an alpha one. This is based on the old-ecs
 * init and it'll be changing.
 */
import { Engine } from '../../engine';
import { createTaskSystem } from '../../systems/async-task';
import { createPointerEventsSystem } from '../../systems/events';
import { createInputSystem } from './../../engine/input';
import { createRaycastSystem } from '../../systems/raycast';
import { createVideoEventsSystem } from '../../systems/videoEvents';
import { createAssetLoadLoadingStateSystem } from '../../systems/assetLoad';
import { createTweenSystem } from '../../systems/tween';
import { pointerEventColliderChecker } from '../../systems/pointer-event-collider-checker';
import { createTriggerAreaEventsSystem } from '../../systems/triggerArea';
import { createTimers } from '../helpers/timers';
import { setGlobalPolyfill } from '../globals';
import { createPhysicsSystem } from '../../systems/physics';
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
export const engine = /* @__PURE__ */ Engine();
/**
 * @public
 * Input system manager. Check for button events
 * @example
 * inputSystem.isTriggered: Returns true if an input action ocurred since the last tick.
 * inputSystem.isPressed: Returns true if an input is currently being pressed down. It will return true on every tick until the button goes up again.
 * inputSystem.getInputCommand: Returns an object with data about the input action.
 */
export const inputSystem = /* @__PURE__ */ createInputSystem(engine);
/**
 * @public
 * Register callback functions to a particular entity on input events.
 */
export const pointerEventsSystem = /* @__PURE__ */ createPointerEventsSystem(engine, inputSystem);
/**
 * @public
 * Register callback functions to a particular entity on raycast results.
 */
export const raycastSystem = /* @__PURE__ */ createRaycastSystem(engine);
/**
 * @public
 * Register callback functions to a particular entity on video events.
 */
export const videoEventsSystem = /* @__PURE__ */ createVideoEventsSystem(engine);
/**
 * @public
 * Register callback functions to a particular entity on asset pre-load events.
 */
export const assetLoadLoadingStateSystem = 
/* @__PURE__ */ createAssetLoadLoadingStateSystem(engine);
/**
 * @public
 * Register callback functions to a particular entity on tween events.
 */
export const tweenSystem = createTweenSystem(engine);
/**
 * @public
 * Register callback functions for trigger area results.
 */
export const triggerAreaEventsSystem = /* @__PURE__ */ createTriggerAreaEventsSystem(engine);
/**
 * @public
 * Timer utilities for delayed and repeated execution.
 */
export const timers = /* @__PURE__ */ createTimers(engine);
export { createTimers };
setGlobalPolyfill('setTimeout', timers.setTimeout);
setGlobalPolyfill('clearTimeout', timers.clearTimeout);
setGlobalPolyfill('setInterval', timers.setInterval);
setGlobalPolyfill('clearInterval', timers.clearInterval);
/**
 * @public
 * Physics helpers for applying impulses and forces to the player.
 */
export const Physics = /* @__PURE__ */ createPhysicsSystem(engine);
/**
 * Adds pointer event collider system only in DEV env
 */
pointerEventColliderChecker(engine);
/**
 * @public
 * Runs an async function
 */
export const executeTask = /* @__PURE__ */ createTaskSystem(engine);
