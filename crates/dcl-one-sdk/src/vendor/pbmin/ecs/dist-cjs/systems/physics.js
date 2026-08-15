"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createPhysicsSystem = exports.KnockbackFalloff = void 0;
const physics_impulse_1 = require("./physics-impulse");
const physics_force_1 = require("./physics-force");
var physics_impulse_2 = require("./physics-impulse");
Object.defineProperty(exports, "KnockbackFalloff", { enumerable: true, get: function () { return physics_impulse_2.KnockbackFalloff; } });
/**
 * @internal
 */
function createPhysicsSystem(engine) {
    const impulse = (0, physics_impulse_1.createPhysicsImpulseHelper)(engine);
    const force = (0, physics_force_1.createPhysicsForceHelper)(engine);
    return {
        applyImpulseToPlayer: impulse.applyImpulseToPlayer,
        applyForceToPlayer: force.applyForceToPlayer,
        removeForceFromPlayer: force.removeForceFromPlayer,
        applyKnockbackToPlayer: impulse.applyKnockbackToPlayer,
        applyForceToPlayerForDuration: force.applyForceToPlayerForDuration,
        applyRepulsionForceToPlayer: force.applyRepulsionForceToPlayer
    };
}
exports.createPhysicsSystem = createPhysicsSystem;
