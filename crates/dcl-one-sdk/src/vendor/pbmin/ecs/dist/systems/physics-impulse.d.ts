/**
 * @public
 * Falloff mode for knockback force over distance.
 */
export declare enum KnockbackFalloff {
    /** Same force at any distance within radius */
    CONSTANT = 0,
    /** Smooth linear decrease to 0 at radius edge: F = magnitude * (1 - distance / radius) */
    LINEAR = 1,
    /** Sharp drop-off, physically realistic: F = magnitude / (distance^2 + 1) */
    INVERSE_SQUARE = 2
}
