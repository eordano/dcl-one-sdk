import { createPhysicsImpulseHelper } from './physics-impulse';
import { createPhysicsForceHelper } from './physics-force';
export { KnockbackFalloff } from './physics-impulse';
/**
 * @internal
 */
export function createPhysicsSystem(engine) {
    const impulse = createPhysicsImpulseHelper(engine);
    const force = createPhysicsForceHelper(engine);
    return {
        applyImpulseToPlayer: impulse.applyImpulseToPlayer,
        applyForceToPlayer: force.applyForceToPlayer,
        removeForceFromPlayer: force.removeForceFromPlayer,
        applyKnockbackToPlayer: impulse.applyKnockbackToPlayer,
        applyForceToPlayerForDuration: force.applyForceToPlayerForDuration,
        applyRepulsionForceToPlayer: force.applyRepulsionForceToPlayer
    };
}
