import { LastWriteWinElementSetComponentDefinition, IEngine } from '../../engine';
import { Entity } from '../../engine/entity';
import type { Vector3Type } from '../../schemas/custom/Vector3';
/**
 * @public
 */
export type TransformComponent = LastWriteWinElementSetComponentDefinition<TransformType>;
/**
 * @public
 */
export interface TransformComponentExtended extends TransformComponent {
    create(entity: Entity, val?: TransformTypeWithOptionals): TransformType;
    createOrReplace(entity: Entity, val?: TransformTypeWithOptionals): TransformType;
    /**
     * Transforms a direction vector from an entity's local coordinate space
     * to world space, accounting for the full parent hierarchy.
     *
     * This applies only rotation (not translation or scale) — suitable for
     * direction vectors like force/impulse directions.
     *
     * @param entity - The source entity whose local space defines the direction
     * @param localDirection - Direction vector in the entity's local coordinates
     * @returns The direction vector in world coordinates
     */
    localToWorldDirection(entity: Entity, localDirection: Vector3Type): Vector3Type;
}
/**
 * @public
 */
export type TransformType = {
    position: {
        x: number;
        y: number;
        z: number;
    };
    rotation: {
        x: number;
        y: number;
        z: number;
        w: number;
    };
    scale: {
        x: number;
        y: number;
        z: number;
    };
    parent?: Entity;
};
/**
 * @public
 */
export type TransformTypeWithOptionals = Partial<TransformType>;
export declare function defineTransformComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): TransformComponentExtended;
