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
exports.createAudioEventsSystem = void 0;
const components = __importStar(require("../components"));
const entity_1 = require("../engine/entity");
/**
 * @internal
 */
function createAudioEventsSystem(engine) {
    const audioSourceComponent = components.AudioSource(engine);
    const audioStreamComponent = components.AudioStream(engine);
    const audioEventComponent = components.AudioEvent(engine);
    const entitiesCallbackAudioStateMap = new Map();
    function registerAudioEventsEntity(entity, callback) {
        // audio event component is not added here because the renderer adds it
        // to every entity with an AudioSource or AudioStream component
        entitiesCallbackAudioStateMap.set(entity, { callback: callback });
    }
    function removeAudioEventsEntity(entity) {
        entitiesCallbackAudioStateMap.delete(entity);
    }
    function hasAudioEventsEntity(entity) {
        return entitiesCallbackAudioStateMap.has(entity);
    }
    // @internal
    engine.addSystem(function AudioEventSystem() {
        for (const [entity, data] of entitiesCallbackAudioStateMap) {
            const hasAudioSource = audioSourceComponent.has(entity);
            const hasAudioStream = audioStreamComponent.has(entity);
            if (engine.getEntityState(entity) === entity_1.EntityState.Removed || (!hasAudioSource && !hasAudioStream)) {
                removeAudioEventsEntity(entity);
                continue;
            }
            // Compare with last state
            const audioEvent = audioEventComponent.get(entity);
            const values = Array.from(audioEvent.values());
            const lastValue = values[audioEvent.size - 1];
            if (lastValue === undefined || (data.lastAudioState !== undefined && data.lastAudioState === lastValue.state))
                continue;
            data.callback(lastValue);
            entitiesCallbackAudioStateMap.set(entity, {
                callback: data.callback,
                lastAudioState: lastValue.state
            });
        }
    });
    return {
        removeAudioEventsEntity(entity) {
            removeAudioEventsEntity(entity);
        },
        registerAudioEventsEntity(entity, callback) {
            registerAudioEventsEntity(entity, callback);
        },
        hasAudioEventsEntity(entity) {
            return hasAudioEventsEntity(entity);
        },
        getAudioState(entity) {
            const audioEvent = audioEventComponent.get(entity);
            const values = Array.from(audioEvent.values());
            const lastValue = values[audioEvent.size - 1];
            return lastValue;
        }
    };
}
exports.createAudioEventsSystem = createAudioEventsSystem;
