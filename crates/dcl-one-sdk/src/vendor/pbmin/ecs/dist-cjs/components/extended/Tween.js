"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineTweenComponent = void 0;
const index_gen_1 = require("../generated/index.gen");
function assertDuration(duration, apiName) {
    if (!Number.isFinite(duration) || duration < 0) {
        throw new Error(`${apiName}: duration must be a non-negative finite number`);
    }
}
/** Shared validation for params that have optional position/rotation/scale with start & end. */
function assertMoveRotateScaleAxesStartEnd(params, apiName) {
    if (!params.position && !params.rotation && !params.scale) {
        throw new Error(`${apiName}: at least one of position, rotation, or scale must be provided`);
    }
    if (params.position) {
        const pos = params.position;
        if (!pos.start || !pos.end) {
            throw new Error(`${apiName}: position must have both start and end`);
        }
    }
    if (params.rotation) {
        const rot = params.rotation;
        if (!rot.start || !rot.end) {
            throw new Error(`${apiName}: rotation must have both start and end`);
        }
    }
    if (params.scale) {
        const scl = params.scale;
        if (!scl.start || !scl.end) {
            throw new Error(`${apiName}: scale must have both start and end`);
        }
    }
}
const TweenHelper = {
    Move(move) {
        return {
            $case: 'move',
            move
        };
    },
    MoveContinuous(moveContinuous) {
        return {
            $case: 'moveContinuous',
            moveContinuous
        };
    },
    Rotate(rotate) {
        return {
            $case: 'rotate',
            rotate
        };
    },
    RotateContinuous(rotateContinuous) {
        return {
            $case: 'rotateContinuous',
            rotateContinuous
        };
    },
    Scale(scale) {
        return {
            $case: 'scale',
            scale
        };
    },
    TextureMove(textureMove) {
        return {
            $case: 'textureMove',
            textureMove
        };
    },
    TextureMoveContinuous(textureMoveContinuous) {
        return {
            $case: 'textureMoveContinuous',
            textureMoveContinuous
        };
    },
    MoveRotateScale(params) {
        assertMoveRotateScaleAxesStartEnd(params, 'Tween.Mode.MoveRotateScale');
        const moveRotateScale = {
            positionStart: params.position ? params.position.start : undefined,
            positionEnd: params.position ? params.position.end : undefined,
            rotationStart: params.rotation ? params.rotation.start : undefined,
            rotationEnd: params.rotation ? params.rotation.end : undefined,
            scaleStart: params.scale ? params.scale.start : undefined,
            scaleEnd: params.scale ? params.scale.end : undefined
        };
        return {
            $case: 'moveRotateScale',
            moveRotateScale
        };
    }
};
function defineTweenComponent(engine) {
    const theComponent = (0, index_gen_1.Tween)(engine);
    return {
        ...theComponent,
        Mode: TweenHelper,
        setMove(entity, start, end, duration, easingFunction = 0 /* EasingFunction.EF_LINEAR */) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'move',
                    move: {
                        start,
                        end
                    }
                },
                duration,
                easingFunction,
                playing: true
            });
        },
        setScale(entity, start, end, duration, easingFunction = 0 /* EasingFunction.EF_LINEAR */) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'scale',
                    scale: {
                        start,
                        end
                    }
                },
                duration,
                easingFunction,
                playing: true
            });
        },
        setRotate(entity, start, end, duration, easingFunction = 0 /* EasingFunction.EF_LINEAR */) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'rotate',
                    rotate: {
                        start,
                        end
                    }
                },
                duration,
                easingFunction,
                playing: true
            });
        },
        setTextureMove(entity, start, end, duration, movementType = 0 /* TextureMovementType.TMT_OFFSET */, easingFunction = 0 /* EasingFunction.EF_LINEAR */) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'textureMove',
                    textureMove: {
                        start,
                        end,
                        movementType
                    }
                },
                duration,
                easingFunction,
                playing: true
            });
        },
        setMoveContinuous(entity, direction, speed, duration = 0) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'moveContinuous',
                    moveContinuous: {
                        direction,
                        speed
                    }
                },
                duration,
                easingFunction: 0 /* EasingFunction.EF_LINEAR */,
                playing: true
            });
        },
        setRotateContinuous(entity, direction, speed, duration = 0) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'rotateContinuous',
                    rotateContinuous: {
                        direction,
                        speed
                    }
                },
                duration,
                easingFunction: 0 /* EasingFunction.EF_LINEAR */,
                playing: true
            });
        },
        setTextureMoveContinuous(entity, direction, speed, movementType = 0 /* TextureMovementType.TMT_OFFSET */, duration = 0) {
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'textureMoveContinuous',
                    textureMoveContinuous: {
                        direction,
                        speed,
                        movementType
                    }
                },
                duration,
                easingFunction: 0 /* EasingFunction.EF_LINEAR */,
                playing: true
            });
        },
        setMoveRotateScale(entity, params) {
            assertMoveRotateScaleAxesStartEnd(params, 'setMoveRotateScale');
            assertDuration(params.duration, 'setMoveRotateScale');
            const { position, rotation, scale, duration, easingFunction = 0 /* EasingFunction.EF_LINEAR */ } = params;
            const moveRotateScale = {
                positionStart: position ? position.start : undefined,
                positionEnd: position ? position.end : undefined,
                rotationStart: rotation ? rotation.start : undefined,
                rotationEnd: rotation ? rotation.end : undefined,
                scaleStart: scale ? scale.start : undefined,
                scaleEnd: scale ? scale.end : undefined
            };
            theComponent.createOrReplace(entity, {
                mode: {
                    $case: 'moveRotateScale',
                    moveRotateScale
                },
                duration,
                easingFunction,
                playing: true
            });
        }
    };
}
exports.defineTweenComponent = defineTweenComponent;
