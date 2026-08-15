import { Entity } from '../../engine/entity';
import type { ComponentDefinition } from '../../engine';
import { CrdtMessageType } from '../../serialization/crdt/types';
export declare const LIVEKIT_MAX_SIZE = 12;
/**
 * @public
 */
export type OnChangeFunction = (entity: Entity, operation: CrdtMessageType, component?: ComponentDefinition<any>, componentValue?: any) => void;
