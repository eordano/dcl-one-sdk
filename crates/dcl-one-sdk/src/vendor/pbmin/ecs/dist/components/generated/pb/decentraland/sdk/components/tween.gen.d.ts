import _m0 from "protobufjs/minimal";
import { Quaternion, Vector2, Vector3 } from "../../common/vectors.gen";
/**
 * @public
 */
export declare const enum TextureMovementType {
    /** TMT_OFFSET - default = TextureMovementType.TMT_OFFSET */
    TMT_OFFSET = 0,
    TMT_TILING = 1
}
/**
 * Implementation guidelines for these easing functions can be found
 * at https://github.com/ai/easings.net/blob/6fcd5f852a470bf1a7890e8178afa0f471d5f2ec/src/easings/easingsFunctions.ts
 */
/**
 * @public
 */
export declare const enum EasingFunction {
    /** EF_LINEAR - default */
    EF_LINEAR = 0,
    EF_EASEINQUAD = 1,
    EF_EASEOUTQUAD = 2,
    EF_EASEQUAD = 3,
    EF_EASEINSINE = 4,
    EF_EASEOUTSINE = 5,
    EF_EASESINE = 6,
    EF_EASEINEXPO = 7,
    EF_EASEOUTEXPO = 8,
    EF_EASEEXPO = 9,
    EF_EASEINELASTIC = 10,
    EF_EASEOUTELASTIC = 11,
    EF_EASEELASTIC = 12,
    EF_EASEINBOUNCE = 13,
    EF_EASEOUTBOUNCE = 14,
    EF_EASEBOUNCE = 15,
    EF_EASEINCUBIC = 16,
    EF_EASEOUTCUBIC = 17,
    EF_EASECUBIC = 18,
    EF_EASEINQUART = 19,
    EF_EASEOUTQUART = 20,
    EF_EASEQUART = 21,
    EF_EASEINQUINT = 22,
    EF_EASEOUTQUINT = 23,
    EF_EASEQUINT = 24,
    EF_EASEINCIRC = 25,
    EF_EASEOUTCIRC = 26,
    EF_EASECIRC = 27,
    EF_EASEINBACK = 28,
    EF_EASEOUTBACK = 29,
    EF_EASEBACK = 30
}
/**
 * @public
 */
export interface PBTween {
    /** in milliseconds */
    duration: number;
    easingFunction: EasingFunction;
    mode?: {
        $case: "move";
        move: Move;
    } | {
        $case: "rotate";
        rotate: Rotate;
    } | {
        $case: "scale";
        scale: Scale;
    } | {
        $case: "textureMove";
        textureMove: TextureMove;
    } | {
        $case: "rotateContinuous";
        rotateContinuous: RotateContinuous;
    } | {
        $case: "moveContinuous";
        moveContinuous: MoveContinuous;
    } | {
        $case: "textureMoveContinuous";
        textureMoveContinuous: TextureMoveContinuous;
    } | {
        $case: "moveRotateScale";
        moveRotateScale: MoveRotateScale;
    } | undefined;
    /** default true (pause or running) */
    playing?: boolean | undefined;
    /** between 0 and 1 */
    currentTime?: number | undefined;
}
/**
 * @public
 */
export interface Move {
    start: Vector3 | undefined;
    end: Vector3 | undefined;
    faceDirection?: boolean | undefined;
}
/**
 * @public
 */
export interface Rotate {
    start: Quaternion | undefined;
    end: Quaternion | undefined;
}
/**
 * @public
 */
export interface Scale {
    start: Vector3 | undefined;
    end: Vector3 | undefined;
}
/**
 * @public
 */
export interface MoveRotateScale {
    positionStart: Vector3 | undefined;
    positionEnd: Vector3 | undefined;
    rotationStart: Quaternion | undefined;
    rotationEnd: Quaternion | undefined;
    scaleStart: Vector3 | undefined;
    scaleEnd: Vector3 | undefined;
}
/**
 * This tween mode allows to move the texture of a PbrMaterial or UnlitMaterial.
 * You can also specify the movement type (offset or tiling)
 */
/**
 * @public
 */
export interface TextureMove {
    start: Vector2 | undefined;
    end: Vector2 | undefined;
    /** default = TextureMovementType.TMT_OFFSET */
    movementType?: TextureMovementType | undefined;
}
/**
 * @public
 */
export interface RotateContinuous {
    direction: Quaternion | undefined;
    speed: number;
}
/**
 * @public
 */
export interface MoveContinuous {
    direction: Vector3 | undefined;
    speed: number;
}
/**
 * @public
 */
export interface TextureMoveContinuous {
    direction: Vector2 | undefined;
    speed: number;
    /** default = TextureMovementType.TMT_OFFSET */
    movementType?: TextureMovementType | undefined;
}
/**
 * @public
 */
export declare namespace PBTween {
    function encode(message: PBTween, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTween;
}
/**
 * @public
 */
export declare namespace Move {
    function encode(message: Move, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): Move;
}
/**
 * @public
 */
export declare namespace Rotate {
    function encode(message: Rotate, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): Rotate;
}
/**
 * @public
 */
export declare namespace Scale {
    function encode(message: Scale, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): Scale;
}
/**
 * @public
 */
export declare namespace MoveRotateScale {
    function encode(message: MoveRotateScale, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): MoveRotateScale;
}
/**
 * @public
 */
export declare namespace TextureMove {
    function encode(message: TextureMove, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): TextureMove;
}
/**
 * @public
 */
export declare namespace RotateContinuous {
    function encode(message: RotateContinuous, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): RotateContinuous;
}
/**
 * @public
 */
export declare namespace MoveContinuous {
    function encode(message: MoveContinuous, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): MoveContinuous;
}
/**
 * @public
 */
export declare namespace TextureMoveContinuous {
    function encode(message: TextureMoveContinuous, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): TextureMoveContinuous;
}
