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
exports.createPhysicsImpulseHelper = exports.KnockbackFalloff = void 0;
const components = __importStar(require("../components"));
const helpers_1 = require("../runtime/helpers");
/**
 * @public
 * Falloff mode for knockback force over distance.
 */
var KnockbackFalloff;
(function (KnockbackFalloff) {
    /** Same force at any distance within radius */
    KnockbackFalloff[KnockbackFalloff["CONSTANT"] = 0] = "CONSTANT";
    /** Smooth linear decrease to 0 at radius edge: F = magnitude * (1 - distance / radius) */
    KnockbackFalloff[KnockbackFalloff["LINEAR"] = 1] = "LINEAR";
    /** Sharp drop-off, physically realistic: F = magnitude / (distance^2 + 1) */
    KnockbackFalloff[KnockbackFalloff["INVERSE_SQUARE"] = 2] = "INVERSE_SQUARE";
})(KnockbackFalloff = exports.KnockbackFalloff || (exports.KnockbackFalloff = {}));
/** @internal */
function createPhysicsImpulseHelper(engine) {
    const PhysicsCombinedImpulse = components.PhysicsCombinedImpulse(engine);
    const Transform = components.Transform(engine);
    const EngineInfo = components.EngineInfo(engine);
    let impulseEventId = 0;
    let lastWrittenEventId = 0;
    let lastWrittenTick = -1;
    function applyImpulseToPlayer(vector, magnitude) {
        let finalVector;
        if (typeof magnitude === 'number') {
            if (helpers_1.Vector3.equalsToFloats(vector, 0, 0, 0))
                return;
            finalVector = helpers_1.Vector3.scale(helpers_1.Vector3.normalize(vector), magnitude);
        }
        else {
            if (helpers_1.Vector3.equalsToFloats(vector, 0, 0, 0))
                return;
            finalVector = vector;
        }
        const currentTick = EngineInfo.getOrNull(engine.RootEntity)?.tickNumber ?? 0;
        const existing = PhysicsCombinedImpulse.getOrNull(engine.PlayerEntity);
        if (existing && existing.eventId !== lastWrittenEventId && lastWrittenEventId !== 0) {
            throw new Error('PBPhysicsCombinedImpulse was modified outside Physics helper. ' +
                'Do not mix direct component access with Physics.applyImpulseToPlayer().');
        }
        if (lastWrittenTick === currentTick && existing) {
            finalVector = helpers_1.Vector3.add(existing.vector ?? { x: 0, y: 0, z: 0 }, finalVector);
        }
        else {
            lastWrittenEventId = ++impulseEventId;
        }
        lastWrittenTick = currentTick;
        PhysicsCombinedImpulse.createOrReplace(engine.PlayerEntity, {
            vector: finalVector,
            eventId: lastWrittenEventId
        });
    }
    function applyKnockbackToPlayer(fromPosition, magnitude, radius = Infinity, falloff = KnockbackFalloff.CONSTANT) {
        const diff = helpers_1.Vector3.subtract(Transform.get(engine.PlayerEntity).position, fromPosition);
        if (helpers_1.Vector3.equalsToFloats(diff, 0, 0, 0)) {
            applyImpulseToPlayer({ x: 0, y: magnitude, z: 0 });
            return;
        }
        // Fast path: default params — no need to compute distance
        if (radius === Infinity && falloff === KnockbackFalloff.CONSTANT) {
            applyImpulseToPlayer(helpers_1.Vector3.scale(helpers_1.Vector3.normalize(diff), magnitude));
            return;
        }
        const distance = helpers_1.Vector3.length(diff);
        if (distance > radius)
            return;
        let effectiveMagnitude;
        switch (falloff) {
            case KnockbackFalloff.LINEAR:
                effectiveMagnitude = magnitude * (1 - distance / radius);
                break;
            case KnockbackFalloff.INVERSE_SQUARE:
                effectiveMagnitude = magnitude / (distance * distance + 1);
                break;
            case KnockbackFalloff.CONSTANT:
            default:
                effectiveMagnitude = magnitude;
                break;
        }
        // normalize(diff) * effectiveMagnitude in one step
        applyImpulseToPlayer(helpers_1.Vector3.scale(diff, effectiveMagnitude / distance));
    }
    return { applyImpulseToPlayer, applyKnockbackToPlayer };
}
exports.createPhysicsImpulseHelper = createPhysicsImpulseHelper;
