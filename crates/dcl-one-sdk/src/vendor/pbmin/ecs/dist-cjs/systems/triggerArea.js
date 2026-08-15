"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.createTriggerAreaEventsSystem = void 0;
const components = __importStar(require("../components"));
const entity_1 = require("../engine/entity");
/**
 * Builds a synthetic PBTriggerAreaResult for a per-tick onStay callback.
 *
 * Transform components are resolved at call time so that scene-owned entities report
 * up-to-date position/rotation/scale. For player-avatar triggerers (reserved entities
 * without a scene-side Transform), there is no scene Transform component, so the cached
 * values from the last ENTER or wire-STAY event are used as-is. These cached values may
 * be slightly stale for the current frame — this is expected and acceptable for the
 * avatar case.
 */
function buildSyntheticStayResult(cached, triggerAreaEntity, triggererEntity, currentTimestamp, Transform) {
    // Shallow-clone the trigger sub-object so we can mutate it.
    const trigger = cached.trigger
        ? {
            entity: cached.trigger.entity,
            layers: cached.trigger.layers,
            position: cached.trigger.position ? { ...cached.trigger.position } : undefined,
            rotation: cached.trigger.rotation ? { ...cached.trigger.rotation } : undefined,
            scale: cached.trigger.scale ? { ...cached.trigger.scale } : undefined
        }
        : undefined;
    // Build the cloned result with a forced TAET_STAY eventType and refreshed timestamp.
    const result = {
        triggeredEntity: cached.triggeredEntity,
        triggeredEntityPosition: cached.triggeredEntityPosition ? { ...cached.triggeredEntityPosition } : undefined,
        triggeredEntityRotation: cached.triggeredEntityRotation ? { ...cached.triggeredEntityRotation } : undefined,
        eventType: 1 /* TriggerAreaEventType.TAET_STAY */,
        timestamp: currentTimestamp,
        trigger
    };
    // Refresh trigger-area entity transform when it is scene-owned.
    const triggerAreaTransform = Transform.getOrNull(triggerAreaEntity);
    if (triggerAreaTransform !== null) {
        result.triggeredEntityPosition = { ...triggerAreaTransform.position };
        result.triggeredEntityRotation = { ...triggerAreaTransform.rotation };
    }
    // Refresh triggerer transform when it is scene-owned.
    // For player-avatar entities (reserved, no scene-side Transform) the cached values are kept.
    const triggererTransform = Transform.getOrNull(triggererEntity);
    if (triggererTransform !== null && result.trigger) {
        result.trigger.position = { ...triggererTransform.position };
        result.trigger.rotation = { ...triggererTransform.rotation };
        result.trigger.scale = { ...triggererTransform.scale };
    }
    return result;
}
/**
 * @internal
 */
function createTriggerAreaEventsSystem(engine) {
    const triggerAreaResultComponent = components.TriggerAreaResult(engine);
    const Transform = components.Transform(engine);
    const entitiesMap = new Map();
    function hasCallbacksMap(entity) {
        return entitiesMap.has(entity) && entitiesMap.get(entity) !== undefined;
    }
    function addEntityCallback(entity, triggerType, callback) {
        if (hasCallbacksMap(entity)) {
            entitiesMap.get(entity).triggerCallbackMap.set(triggerType, callback);
        }
        else {
            entitiesMap.set(entity, {
                triggerCallbackMap: new Map([[triggerType, callback]]),
                lastConsumedTimestamp: -1,
                insideTriggerers: new Map()
            });
        }
    }
    function removeEntityCallback(entity, triggerType) {
        if (!entitiesMap.has(entity) || !entitiesMap.get(entity).triggerCallbackMap.has(triggerType))
            return;
        const triggerCallbackMap = entitiesMap.get(entity).triggerCallbackMap;
        triggerCallbackMap.delete(triggerType);
        // Remove entity if no more trigger callbacks are registered.
        // insideTriggerers is intentionally left populated so that re-subscription picks up
        // in-flight sessions without missing the first synthesized onStay.
        if (triggerCallbackMap.size === 0)
            entitiesMap.delete(entity);
    }
    function onTriggerEnter(entity, cb) {
        addEntityCallback(entity, 0 /* TriggerAreaEventType.TAET_ENTER */, cb);
    }
    function removeOnTriggerEnter(entity) {
        removeEntityCallback(entity, 0 /* TriggerAreaEventType.TAET_ENTER */);
    }
    function onTriggerStay(entity, cb) {
        addEntityCallback(entity, 1 /* TriggerAreaEventType.TAET_STAY */, cb);
    }
    function removeOnTriggerStay(entity) {
        removeEntityCallback(entity, 1 /* TriggerAreaEventType.TAET_STAY */);
    }
    function onTriggerExit(entity, cb) {
        addEntityCallback(entity, 2 /* TriggerAreaEventType.TAET_EXIT */, cb);
    }
    function removeOnTriggerExit(entity) {
        removeEntityCallback(entity, 2 /* TriggerAreaEventType.TAET_EXIT */);
    }
    engine.addSystem(function TriggerAreaResultSystem() {
        const garbageEntries = [];
        for (const [entity, data] of entitiesMap) {
            if (engine.getEntityState(entity) === entity_1.EntityState.Removed) {
                garbageEntries.push(entity);
                continue;
            }
            const result = triggerAreaResultComponent.get(entity);
            // -----------------------------------------------------------------------
            // Pass 1: drain new GOVS events
            // -----------------------------------------------------------------------
            // The Explorer may be taking time before the result component is put.
            if (result.size > 0) {
                const values = Array.from(result.values());
                // Determine starting index for new values (more than one could be added between System updates).
                // Search backwards to find the anchor at lastConsumedTimestamp.
                let startIndex = 0;
                if (data.lastConsumedTimestamp >= 0) {
                    const newestTimestamp = values[values.length - 1].timestamp;
                    // If nothing new, skip processing.
                    if (newestTimestamp > data.lastConsumedTimestamp) {
                        // Find index of value with the lastConsumedTimestamp.
                        let i = values.length - 2;
                        while (i >= 0 && values[i].timestamp > data.lastConsumedTimestamp)
                            i--;
                        // Mark the following value index as the starting point to trigger all the new value callbacks.
                        startIndex = i + 1;
                    }
                    else {
                        // No new events — skip to Pass 2.
                        startIndex = values.length;
                    }
                }
                if (startIndex < values.length) {
                    // Process new wire events in chronological order.
                    for (let i = startIndex; i < values.length; i++) {
                        const event = values[i];
                        switch (event.eventType) {
                            case 0 /* TriggerAreaEventType.TAET_ENTER */:
                                // Update in-flight tracking before firing the callback.
                                data.insideTriggerers.set(event.trigger.entity, {
                                    triggeredEntity: event.triggeredEntity,
                                    triggeredEntityPosition: event.triggeredEntityPosition
                                        ? { ...event.triggeredEntityPosition }
                                        : undefined,
                                    triggeredEntityRotation: event.triggeredEntityRotation
                                        ? { ...event.triggeredEntityRotation }
                                        : undefined,
                                    eventType: event.eventType,
                                    timestamp: event.timestamp,
                                    trigger: event.trigger
                                        ? {
                                            entity: event.trigger.entity,
                                            layers: event.trigger.layers,
                                            position: event.trigger.position ? { ...event.trigger.position } : undefined,
                                            rotation: event.trigger.rotation ? { ...event.trigger.rotation } : undefined,
                                            scale: event.trigger.scale ? { ...event.trigger.scale } : undefined
                                        }
                                        : undefined
                                });
                                if (data.triggerCallbackMap.has(0 /* TriggerAreaEventType.TAET_ENTER */)) {
                                    data.triggerCallbackMap.get(0 /* TriggerAreaEventType.TAET_ENTER */)(event);
                                }
                                break;
                            case 2 /* TriggerAreaEventType.TAET_EXIT */:
                                data.insideTriggerers.delete(event.trigger.entity);
                                if (data.triggerCallbackMap.has(2 /* TriggerAreaEventType.TAET_EXIT */)) {
                                    data.triggerCallbackMap.get(2 /* TriggerAreaEventType.TAET_EXIT */)(event);
                                }
                                break;
                            // Wire-level TAET_STAY and any unknown event types are ignored — no callback, no state mutation.
                        }
                    }
                    data.lastConsumedTimestamp = values[values.length - 1].timestamp;
                }
            }
            // -----------------------------------------------------------------------
            // Pass 2: synthesize per-tick onStay callbacks
            // -----------------------------------------------------------------------
            // Only run if an onStay callback is registered and there are tracked triggerers.
            if (data.triggerCallbackMap.has(1 /* TriggerAreaEventType.TAET_STAY */) &&
                data.insideTriggerers.size > 0) {
                const onStay = data.triggerCallbackMap.get(1 /* TriggerAreaEventType.TAET_STAY */);
                const currentTimestamp = Date.now();
                for (const [triggererEntity, cachedResult] of data.insideTriggerers) {
                    onStay(buildSyntheticStayResult(cachedResult, entity, triggererEntity, currentTimestamp, Transform));
                }
            }
        }
        // Clean up garbage entries.
        garbageEntries.forEach((garbageEntity) => entitiesMap.delete(garbageEntity));
    });
    return {
        onTriggerEnter,
        removeOnTriggerEnter,
        onTriggerStay,
        removeOnTriggerStay,
        onTriggerExit,
        removeOnTriggerExit
    };
}
exports.createTriggerAreaEventsSystem = createTriggerAreaEventsSystem;
