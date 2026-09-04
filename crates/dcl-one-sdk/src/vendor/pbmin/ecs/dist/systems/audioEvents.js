import * as components from '../components';
import { EntityState } from '../engine/entity';
/**
 * @internal
 */
export function createAudioEventsSystem(engine) {
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
            if (engine.getEntityState(entity) === EntityState.Removed || (!hasAudioSource && !hasAudioStream)) {
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
