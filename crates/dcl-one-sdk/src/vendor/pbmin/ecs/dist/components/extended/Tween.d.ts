import { Entity, IEngine, LastWriteWinElementSetComponentDefinition } from '../../engine';
import { Quaternion, Vector2, Vector3 } from '../generated/pb/decentraland/common/vectors.gen';
import { EasingFunction, Move, MoveContinuous, PBTween, Rotate, RotateContinuous, Scale, TextureMove, TextureMoveContinuous, TextureMovementType } from '../generated/index.gen';
/**
 * @public
 * Partial params for Tween.Mode.MoveRotateScale(). At least one of position, rotation, or scale must be provided.
 * Use this when building a mode for Tween.createOrReplace() or TweenSequence (e.g. only positionStart/positionEnd).
 */
export interface MoveRotateScaleModeParams {
    /** Position tween (start → end). Optional. */
    position?: {
        start: Vector3;
        end: Vector3;
    };
    /** Rotation tween (start → end). Optional. */
    rotation?: {
        start: Quaternion;
        end: Quaternion;
    };
    /** Scale tween (start → end). Optional. */
    scale?: {
        start: Vector3;
        end: Vector3;
    };
}
/**
 * @public
 * Parameters for setMoveRotateScale. At least one of position, rotation, or scale must be provided.
 */
export interface SetMoveRotateScaleParams extends MoveRotateScaleModeParams {
    /** Duration of the tween in milliseconds. */
    duration: number;
    /** Easing function (defaults to EF_LINEAR). */
    easingFunction?: EasingFunction;
}
/**
 * @public
 */
export interface TweenHelper {
    /**
     * @returns a move mode tween
     */
    Move: (move: Move) => PBTween['mode'];
    /**
     * @returns a move-continuous mode tween
     */
    MoveContinuous: (move: MoveContinuous) => PBTween['mode'];
    /**
     * @returns a rotate mode tween
     */
    Rotate: (rotate: Rotate) => PBTween['mode'];
    /**
     * @returns a rotate-continuous mode tween
     */
    RotateContinuous: (rotate: RotateContinuous) => PBTween['mode'];
    /**
     * @returns a scale mode tween
     */
    Scale: (scale: Scale) => PBTween['mode'];
    /**
     * @returns a texture-move mode tween
     */
    TextureMove: (textureMove: TextureMove) => PBTween['mode'];
    /**
     * @returns a texture-move-continuous mode tween
     */
    TextureMoveContinuous: (textureMove: TextureMoveContinuous) => PBTween['mode'];
    /**
     * @returns a move-rotate-scale mode tween
     * @param params - partial transform (at least one of position, rotation, scale); omit axes you don't need
     */
    MoveRotateScale: (params: MoveRotateScaleModeParams) => PBTween['mode'];
}
/**
 * @public
 */
export interface TweenComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBTween> {
    /**
     * Helpers with constructor
     */
    Mode: TweenHelper;
    /**
     * @public
     *
     * Creates or replaces a move tween component that animates an entity's position from start to end
     * @param entity - entity to apply the tween to
     * @param start - starting position vector
     * @param end - ending position vector
     * @param duration - duration of the tween in milliseconds
     * @param easingFunction - easing function to use (defaults to EF_LINEAR)
     */
    setMove(entity: Entity, start: Vector3, end: Vector3, duration: number, easingFunction?: EasingFunction): void;
    /**
     * @public
     *
     * Creates or replaces a scale tween component that animates an entity's scale from start to end
     * @param entity - entity to apply the tween to
     * @param start - starting scale vector
     * @param end - ending scale vector
     * @param duration - duration of the tween in milliseconds
     * @param easingFunction - easing function to use (defaults to EF_LINEAR)
     */
    setScale(entity: Entity, start: Vector3, end: Vector3, duration: number, easingFunction?: EasingFunction): void;
    /**
     * @public
     *
     * Creates or replaces a rotation tween component that animates an entity's rotation from start to end
     * @param entity - entity to apply the tween to
     * @param start - starting rotation quaternion
     * @param end - ending rotation quaternion
     * @param duration - duration of the tween in milliseconds
     * @param easingFunction - easing function to use (defaults to EF_LINEAR)
     */
    setRotate(entity: Entity, start: Quaternion, end: Quaternion, duration: number, easingFunction?: EasingFunction): void;
    /**
     * @public
     *
     * Creates or replaces a texture move tween component that animates texture UV coordinates from start to end
     * @param entity - entity to apply the tween to
     * @param start - starting UV coordinates
     * @param end - ending UV coordinates
     * @param duration - duration of the tween in milliseconds
     * @param movementType - type of texture movement (defaults to TMT_OFFSET)
     * @param easingFunction - easing function to use (defaults to EF_LINEAR)
     */
    setTextureMove(entity: Entity, start: Vector2, end: Vector2, duration: number, movementType?: TextureMovementType, easingFunction?: EasingFunction): void;
    /**
     * @public
     *
     * Creates or replaces a continuous move tween component that moves an entity continuously in a direction
     * @param entity - entity to apply the tween to
     * @param direction - direction vector to move towards
     * @param speed - speed of movement per second
     * @param duration - duration of the tween in milliseconds (defaults to 0 for infinite)
     */
    setMoveContinuous(entity: Entity, direction: Vector3, speed: number, duration?: number): void;
    /**
     * @public
     *
     * Creates or replaces a continuous rotation tween component that rotates an entity continuously
     * @param entity - entity to apply the tween to
     * @param direction - rotation direction quaternion
     * @param speed - speed of rotation per second
     * @param duration - duration of the tween in milliseconds (defaults to 0 for infinite)
     */
    setRotateContinuous(entity: Entity, direction: Quaternion, speed: number, duration?: number): void;
    /**
     * @public
     *
     * Creates or replaces a continuous texture move tween component that moves texture UV coordinates continuously
     * @param entity - entity to apply the tween to
     * @param direction - direction vector for UV movement
     * @param speed - speed of UV movement per second
     * @param movementType - type of texture movement (defaults to TMT_OFFSET)
     * @param duration - duration of the tween in milliseconds (defaults to 0 for infinite)
     */
    setTextureMoveContinuous(entity: Entity, direction: Vector2, speed: number, movementType?: TextureMovementType, duration?: number): void;
    /**
     * @public
     *
     * Creates or replaces a move-rotate-scale tween component that simultaneously animates
     * an entity's position, rotation, and/or scale from start to end. Provide only the
     * properties you need (at least one of position, rotation, or scale).
     * @param entity - entity to apply the tween to
     * @param params - object with optional position, rotation, scale (each with start/end), duration, and optional easingFunction
     */
    setMoveRotateScale(entity: Entity, params: SetMoveRotateScaleParams): void;
}
export declare function defineTweenComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): TweenComponentDefinitionExtended;
