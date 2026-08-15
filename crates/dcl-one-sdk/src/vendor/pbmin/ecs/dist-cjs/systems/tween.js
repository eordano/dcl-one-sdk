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
exports.createTweenSystem = void 0;
const components = __importStar(require("../components"));
const ByteBuffer_1 = require("../serialization/ByteBuffer");
const utils_1 = require("./crdt/utils");
const globals_1 = require("../runtime/globals");
/**
 * Avoid creating multiple tween systems
 */
const cacheTween = new Map();
/**
 * @public
 * @returns tween helper to be used on the scene
 */
function createTweenSystem(engine) {
    if (cacheTween.has(engine._id)) {
        return cacheTween.get(engine._id);
    }
    const Tween = components.Tween(engine);
    const TweenState = components.TweenState(engine);
    const TweenSequence = components.TweenSequence(engine);
    const cache = new Map();
    function isCompleted(entity) {
        const tweenState = TweenState.getOrNull(entity);
        const tween = Tween.getOrNull(entity);
        const tweenCache = cache.get(entity);
        if (!tweenState || !tween || !tweenCache)
            return false;
        /* istanbul ignore next */
        if (
        // Renderer notified that the tween is completed
        // Only consider it completed if the tween hasn't changed this frame (to avoid false positives after YOYO/sequence processing)
        ((tweenState.state === 1 /* TweenStateStatus.TS_COMPLETED */ && !tweenCache.changed) ||
            (tweenChanged(entity) && !tweenCache.changed)) &&
            // Avoid sending isCompleted multiple times
            !tweenCache.completed) {
            return true;
        }
        return false;
    }
    function tweenChanged(entity) {
        const currentTween = Tween.getOrNull(entity);
        const prevTween = cache.get(entity)?.tween;
        /* istanbul ignore next */
        if ((currentTween && !prevTween) || (!currentTween && prevTween)) {
            return true;
        }
        if (!currentTween || !prevTween)
            return false;
        const currentBuff = new ByteBuffer_1.ReadWriteByteBuffer();
        Tween.schema.serialize(currentTween, currentBuff);
        const compareResult = (0, utils_1.dataCompare)(currentBuff.toBinary(), prevTween);
        return compareResult !== 0;
    }
    // System to manage cache (needed for tweenSystem.tweenCompleted() to work)
    engine.addSystem(() => {
        for (const [entity, tween] of engine.getEntitiesWith(Tween)) {
            if (tweenChanged(entity)) {
                const buffer = new ByteBuffer_1.ReadWriteByteBuffer();
                Tween.schema.serialize(tween, buffer);
                cache.set(entity, {
                    tween: buffer.toBinary(),
                    completed: false,
                    changed: true
                });
                continue;
            }
            const tweenCache = cache.get(entity);
            if (tweenCache) {
                tweenCache.changed = false;
                if (isCompleted(entity)) {
                    // set the tween completed to avoid calling this again for the same tween
                    tweenCache.completed = true;
                }
            }
        }
    }, Number.NEGATIVE_INFINITY);
    function initializeTweenSequenceSystem() {
        const restartTweens = [];
        function backwardsTween(tween) {
            if (tween.mode?.$case === 'move' && tween.mode.move) {
                return { ...tween, mode: { ...tween.mode, move: { start: tween.mode.move.end, end: tween.mode.move.start } } };
            }
            if (tween.mode?.$case === 'rotate' && tween.mode.rotate) {
                return {
                    ...tween,
                    mode: { ...tween.mode, rotate: { start: tween.mode.rotate.end, end: tween.mode.rotate.start } }
                };
            }
            if (tween.mode?.$case === 'scale' && tween.mode.scale) {
                return {
                    ...tween,
                    mode: { ...tween.mode, scale: { start: tween.mode.scale.end, end: tween.mode.scale.start } }
                };
            }
            if (tween.mode?.$case === 'textureMove' && tween.mode.textureMove) {
                return {
                    ...tween,
                    mode: { ...tween.mode, textureMove: { start: tween.mode.textureMove.end, end: tween.mode.textureMove.start } }
                };
            }
            /* istanbul ignore next */
            throw new Error('Invalid tween');
        }
        // Logic for sequence tweens
        engine.addSystem(() => {
            for (const restart of restartTweens) {
                restart();
            }
            restartTweens.length = 0;
            for (const [entity, tween] of engine.getEntitiesWith(Tween)) {
                const tweenCache = cache.get(entity);
                if (!tweenCache)
                    continue;
                // Only process tween sequences if the tween is completed
                if (tweenCache.completed) {
                    const tweenSequence = TweenSequence.getOrNull(entity);
                    if (!tweenSequence)
                        continue;
                    const { sequence } = tweenSequence;
                    if (sequence && sequence.length) {
                        const [nextTweenSequence, ...otherTweens] = sequence;
                        Tween.createOrReplace(entity, nextTweenSequence);
                        const mutableTweenHelper = TweenSequence.getMutable(entity);
                        mutableTweenHelper.sequence = otherTweens;
                        if (tweenSequence.loop === 0 /* TweenLoop.TL_RESTART */) {
                            mutableTweenHelper.sequence.push(tween);
                        }
                        // Reset completed flag for the next tween in sequence
                        // Mark as changed so the cache system will detect the change and reset the cache properly
                        tweenCache.completed = false;
                        tweenCache.changed = true;
                    }
                    else if (tweenSequence.loop === 1 /* TweenLoop.TL_YOYO */) {
                        Tween.createOrReplace(entity, backwardsTween(tween));
                        // Reset completed flag for the backwards tween
                        // Mark as changed so the cache system will detect the change and reset the cache properly
                        tweenCache.completed = false;
                        tweenCache.changed = true;
                    }
                    else if (tweenSequence.loop === 0 /* TweenLoop.TL_RESTART */) {
                        Tween.deleteFrom(entity);
                        cache.delete(entity);
                        restartTweens.push(() => {
                            Tween.createOrReplace(entity, tween);
                        });
                    }
                }
            }
        }, Number.NEGATIVE_INFINITY);
    }
    // Some Explorers may not inject the flag and TweenSequence logic must be enabled in that case
    const enableTweenSequenceLogic = (0, globals_1.getGlobal)('ENABLE_SDK_TWEEN_SEQUENCE');
    if (enableTweenSequenceLogic !== false)
        initializeTweenSequenceSystem();
    const tweenSystem = {
        // This event is fired only once per tween
        tweenCompleted: isCompleted
    };
    cacheTween.set(engine._id, tweenSystem);
    return tweenSystem;
}
exports.createTweenSystem = createTweenSystem;
