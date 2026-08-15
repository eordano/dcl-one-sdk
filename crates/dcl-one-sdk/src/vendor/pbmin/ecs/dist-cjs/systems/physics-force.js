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
exports.createPhysicsForceHelper = void 0;
const components = __importStar(require("../components"));
const helpers_1 = require("../runtime/helpers");
const physics_impulse_1 = require("./physics-impulse");
/** @internal */
function createPhysicsForceHelper(engine) {
    const PhysicsCombinedForce = components.PhysicsCombinedForce(engine);
    const Transform = components.Transform(engine);
    const timers = (0, helpers_1.createTimers)(engine);
    const durationTimers = new Map();
    // Key = source entity producing the force (not the target); all forces target PlayerEntity.
    const forceSources = new Map();
    const repulsionSources = new Map();
    let lastWrittenForceVector = null;
    function recalcForce() {
        if (forceSources.size === 0) {
            if (PhysicsCombinedForce.has(engine.PlayerEntity)) {
                PhysicsCombinedForce.deleteFrom(engine.PlayerEntity);
            }
            lastWrittenForceVector = null;
            return;
        }
        const current = PhysicsCombinedForce.getOrNull(engine.PlayerEntity);
        if (current && lastWrittenForceVector && current.vector) {
            if (!helpers_1.Vector3.equals(current.vector, lastWrittenForceVector)) {
                console.error('PBPhysicsCombinedForce was modified externally.', 'Expected:', lastWrittenForceVector, 'Found:', current.vector, '-- overwriting with local state.');
            }
        }
        let sum = { x: 0, y: 0, z: 0 };
        for (const v of forceSources.values()) {
            sum = helpers_1.Vector3.add(sum, v);
        }
        PhysicsCombinedForce.createOrReplace(engine.PlayerEntity, { vector: sum });
        lastWrittenForceVector = sum;
    }
    function applyForceToPlayer(source, vector, magnitude) {
        let finalVector;
        if (helpers_1.Vector3.equalsToFloats(vector, 0, 0, 0))
            return;
        if (typeof magnitude === 'number') {
            finalVector = helpers_1.Vector3.scale(helpers_1.Vector3.normalize(vector), magnitude);
        }
        else {
            finalVector = vector;
        }
        forceSources.set(source, finalVector);
        recalcForce();
    }
    function removeForceFromPlayer(source) {
        repulsionSources.delete(source);
        const timerId = durationTimers.get(source);
        if (timerId !== undefined) {
            timers.clearTimeout(timerId);
            durationTimers.delete(source);
        }
        if (!forceSources.has(source))
            return;
        forceSources.delete(source);
        recalcForce();
    }
    function scheduleForceDuration(source, seconds) {
        const existing = durationTimers.get(source);
        if (existing !== undefined) {
            timers.clearTimeout(existing);
        }
        const timerId = timers.setTimeout(() => {
            durationTimers.delete(source);
            removeForceFromPlayer(source);
        }, seconds * 1000);
        durationTimers.set(source, timerId);
    }
    function applyForceToPlayerForDuration(source, duration, vector, magnitude) {
        applyForceToPlayer(source, vector, magnitude);
        scheduleForceDuration(source, duration);
    }
    function computeRepulsionVector(fromPosition, magnitude, radius, falloff) {
        const diff = helpers_1.Vector3.subtract(Transform.get(engine.PlayerEntity).position, fromPosition);
        if (helpers_1.Vector3.equalsToFloats(diff, 0, 0, 0))
            return { x: 0, y: magnitude, z: 0 };
        // Fast path: default params — no need to compute distance
        if (radius === Infinity && falloff === physics_impulse_1.KnockbackFalloff.CONSTANT) {
            return helpers_1.Vector3.scale(helpers_1.Vector3.normalize(diff), magnitude);
        }
        const distance = helpers_1.Vector3.length(diff);
        if (distance > radius)
            return null;
        let effectiveMagnitude;
        switch (falloff) {
            case physics_impulse_1.KnockbackFalloff.LINEAR:
                effectiveMagnitude = magnitude * (1 - distance / radius);
                break;
            case physics_impulse_1.KnockbackFalloff.INVERSE_SQUARE:
                effectiveMagnitude = magnitude / (distance * distance + 1);
                break;
            case physics_impulse_1.KnockbackFalloff.CONSTANT:
            default:
                effectiveMagnitude = magnitude;
                break;
        }
        if (effectiveMagnitude === 0)
            return null;
        // normalize(diff) * effectiveMagnitude in one step
        return helpers_1.Vector3.scale(diff, effectiveMagnitude / distance);
    }
    function applyRepulsionForceToPlayer(source, fromPosition, magnitude, radius = Infinity, falloff = physics_impulse_1.KnockbackFalloff.CONSTANT) {
        repulsionSources.set(source, { fromPosition, magnitude, radius, falloff });
        const vector = computeRepulsionVector(fromPosition, magnitude, radius, falloff);
        if (vector) {
            forceSources.set(source, vector);
        }
        else {
            forceSources.delete(source);
        }
        recalcForce();
    }
    // Background system: recalculate repulsion vectors and clean up stale forces every tick.
    // Stale forces can appear when CRDT sync from another client writes PhysicsCombinedForce
    // externally (entity-1 ambiguity: each client interprets entity 1 as its own player).
    engine.addSystem(() => {
        // Repulsion forces need per-tick direction recalculation as the player moves
        if (repulsionSources.size > 0) {
            for (const [source, { fromPosition, magnitude, radius, falloff }] of repulsionSources) {
                const vector = computeRepulsionVector(fromPosition, magnitude, radius, falloff);
                if (vector) {
                    forceSources.set(source, vector);
                }
                else {
                    forceSources.delete(source);
                }
            }
            recalcForce();
            return;
        }
        // No local sources — clean up any externally-created component (e.g. from CRDT sync)
        if (forceSources.size === 0 && PhysicsCombinedForce.has(engine.PlayerEntity)) {
            PhysicsCombinedForce.deleteFrom(engine.PlayerEntity);
            lastWrittenForceVector = null;
        }
    });
    return { applyForceToPlayer, removeForceFromPlayer, applyForceToPlayerForDuration, applyRepulsionForceToPlayer };
}
exports.createPhysicsForceHelper = createPhysicsForceHelper;
