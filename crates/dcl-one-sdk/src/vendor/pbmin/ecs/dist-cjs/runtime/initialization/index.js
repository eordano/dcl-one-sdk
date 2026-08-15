"use strict";
/**
 * @alpha * This file initialization is an alpha one. This is based on the old-ecs
 * init and it'll be changing.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.executeTask = exports.Physics = exports.createTimers = exports.timers = exports.triggerAreaEventsSystem = exports.tweenSystem = exports.assetLoadLoadingStateSystem = exports.videoEventsSystem = exports.raycastSystem = exports.pointerEventsSystem = exports.inputSystem = exports.engine = void 0;
const engine_1 = require("../../engine");
const async_task_1 = require("../../systems/async-task");
const events_1 = require("../../systems/events");
const input_1 = require("./../../engine/input");
const raycast_1 = require("../../systems/raycast");
const videoEvents_1 = require("../../systems/videoEvents");
const assetLoad_1 = require("../../systems/assetLoad");
const tween_1 = require("../../systems/tween");
const pointer_event_collider_checker_1 = require("../../systems/pointer-event-collider-checker");
const triggerArea_1 = require("../../systems/triggerArea");
const timers_1 = require("../helpers/timers");
Object.defineProperty(exports, "createTimers", { enumerable: true, get: function () { return timers_1.createTimers; } });
const globals_1 = require("../globals");
const physics_1 = require("../../systems/physics");
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
exports.engine = (0, engine_1.Engine)();
/**
 * @public
 * Input system manager. Check for button events
 * @example
 * inputSystem.isTriggered: Returns true if an input action ocurred since the last tick.
 * inputSystem.isPressed: Returns true if an input is currently being pressed down. It will return true on every tick until the button goes up again.
 * inputSystem.getInputCommand: Returns an object with data about the input action.
 */
exports.inputSystem = (0, input_1.createInputSystem)(exports.engine);
/**
 * @public
 * Register callback functions to a particular entity on input events.
 */
exports.pointerEventsSystem = (0, events_1.createPointerEventsSystem)(exports.engine, exports.inputSystem);
/**
 * @public
 * Register callback functions to a particular entity on raycast results.
 */
exports.raycastSystem = (0, raycast_1.createRaycastSystem)(exports.engine);
/**
 * @public
 * Register callback functions to a particular entity on video events.
 */
exports.videoEventsSystem = (0, videoEvents_1.createVideoEventsSystem)(exports.engine);
/**
 * @public
 * Register callback functions to a particular entity on asset pre-load events.
 */
exports.assetLoadLoadingStateSystem = 
/* @__PURE__ */ (0, assetLoad_1.createAssetLoadLoadingStateSystem)(exports.engine);
/**
 * @public
 * Register callback functions to a particular entity on tween events.
 */
exports.tweenSystem = (0, tween_1.createTweenSystem)(exports.engine);
/**
 * @public
 * Register callback functions for trigger area results.
 */
exports.triggerAreaEventsSystem = (0, triggerArea_1.createTriggerAreaEventsSystem)(exports.engine);
/**
 * @public
 * Timer utilities for delayed and repeated execution.
 */
exports.timers = (0, timers_1.createTimers)(exports.engine);
(0, globals_1.setGlobalPolyfill)('setTimeout', exports.timers.setTimeout);
(0, globals_1.setGlobalPolyfill)('clearTimeout', exports.timers.clearTimeout);
(0, globals_1.setGlobalPolyfill)('setInterval', exports.timers.setInterval);
(0, globals_1.setGlobalPolyfill)('clearInterval', exports.timers.clearInterval);
/**
 * @public
 * Physics helpers for applying impulses and forces to the player.
 */
exports.Physics = (0, physics_1.createPhysicsSystem)(exports.engine);
/**
 * Adds pointer event collider system only in DEV env
 */
(0, pointer_event_collider_checker_1.pointerEventColliderChecker)(exports.engine);
/**
 * @public
 * Runs an async function
 */
exports.executeTask = (0, async_task_1.createTaskSystem)(exports.engine);
