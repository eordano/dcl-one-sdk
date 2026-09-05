import { DeepReadonlyObject, Entity } from '../engine';
import { PBAudioEvent } from '../components';
/**
 * @public
 */
export type AudioEventsSystemCallback = (event: DeepReadonlyObject<PBAudioEvent>) => void;
/**
 * @public
 */
export interface AudioEventsSystem {
    removeAudioEventsEntity(entity: Entity): void;
    registerAudioEventsEntity(entity: Entity, callback: AudioEventsSystemCallback): void;
    hasAudioEventsEntity(entity: Entity): boolean;
    /**
     * Returns the latest state of the AudioEvent
     * @param entity - Entity to retrieve the audio status
     */
    getAudioState(entity: Entity): DeepReadonlyObject<PBAudioEvent> | undefined;
}
