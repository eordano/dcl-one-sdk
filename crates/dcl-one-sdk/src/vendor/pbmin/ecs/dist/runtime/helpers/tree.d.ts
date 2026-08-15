import { Entity } from '../../engine/entity';
import { ComponentDefinition, IEngine } from '../../engine';
import { Vector3Type } from '../../schemas/custom/Vector3';
import { QuaternionType } from '../../schemas/custom/Quaternion';
/**
 * @public
 * Rotate a vector by a quaternion
 * Uses the formula: v' = q * v * q^(-1), optimized version
 */
export declare function rotateVectorByQuaternion(v: Vector3Type, q: QuaternionType): Vector3Type;
/**
 * Get an iterator of entities that follow a tree structure for a component
 * @public
 * @param engine - the engine running the entities
 * @param entity - the root entity of the tree
 * @param component - the parenting component to filter by
 * @returns An iterator of an array as [entity, entity2, ...]
 *
 * Example:
 * ```ts
 * const TreeComponent = engine.defineComponent('custom::TreeComponent', {
 *    label: Schemas.String,
 *    parent: Schemas.Entity
 * })
 *
 * for (const entity of getComponentEntityTree(engine, entity, TreeComponent)) {
 *    // entity in the tree
 * }
 * ```
 */
export declare function getComponentEntityTree<T>(engine: Pick<IEngine, 'getEntitiesWith'>, entity: Entity, component: ComponentDefinition<T & {
    parent?: Entity;
}>): Generator<Entity>;
/**
 * Remove all components of each entity in the tree made with Transform parenting
 * @param engine - the engine running the entities
 * @param firstEntity - the root entity of the tree
 * @public
 */
export declare function removeEntityWithChildren(engine: Pick<IEngine, 'getEntitiesWith' | 'defineComponentFromSchema' | 'removeEntity' | 'defineComponent'>, entity: Entity): void;
/**
 * Get all entities that have the given entity as their parent
 * @public
 * @param engine - the engine running the entities
 * @param parent - the parent entity to find children for
 * @returns An array of entities that have the given parent
 *
 * Example:
 * ```ts
 * const children = getEntitiesWithParent(engine, myEntity)
 * for (const child of children) {
 *   // process each child entity
 * }
 * ```
 */
export declare function getEntitiesWithParent(engine: Pick<IEngine, 'getEntitiesWith' | 'defineComponentFromSchema'>, parent: Entity): Entity[];
/** @public Engine type for world transform functions */
export type WorldTransformEngine = Pick<IEngine, 'getEntitiesWith' | 'defineComponentFromSchema' | 'PlayerEntity'>;
/**
 * Get the world position of an entity, taking into account the full parent hierarchy.
 * This computes the world-space position by accumulating all parent transforms
 * (position, rotation, and scale).
 *
 * When the entity has AvatarAttach and Transform, the renderer updates the Transform
 * with avatar-relative values (including the exact anchor point offset for hand, head, etc.).
 * This function combines the player's transform with those values to compute the world position.
 *
 * @public
 * @param engine - the engine running the entities
 * @param entity - the entity to get the world position for
 * @returns The entity's position in world space. Returns `{x: 0, y: 0, z: 0}` if the entity has no Transform.
 *
 * Example:
 * ```ts
 * const worldPos = getWorldPosition(engine, childEntity)
 * console.log(`World position: ${worldPos.x}, ${worldPos.y}, ${worldPos.z}`)
 * ```
 */
export declare function getWorldPosition(engine: WorldTransformEngine, entity: Entity): Vector3Type;
/**
 * Get the world rotation of an entity, taking into account the full parent hierarchy.
 * This computes the world-space rotation by combining all parent rotations.
 *
 * When the entity has AvatarAttach and Transform, the renderer updates the Transform
 * with avatar-relative values (including the exact anchor point rotation for hand, head, etc.).
 * This function combines the player's rotation with those values to compute the world rotation.
 *
 * @public
 * @param engine - the engine running the entities
 * @param entity - the entity to get the world rotation for
 * @returns The entity's rotation in world space as a quaternion. Returns identity quaternion `{x: 0, y: 0, z: 0, w: 1}` if the entity has no Transform.
 *
 * Example:
 * ```ts
 * const worldRot = getWorldRotation(engine, childEntity)
 * console.log(`World rotation: ${worldRot.x}, ${worldRot.y}, ${worldRot.z}, ${worldRot.w}`)
 * ```
 */
export declare function getWorldRotation(engine: WorldTransformEngine, entity: Entity): QuaternionType;
