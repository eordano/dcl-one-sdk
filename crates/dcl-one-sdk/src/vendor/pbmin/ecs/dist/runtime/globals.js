/**
 * Internal utilities for standardized globalThis access.
 */
/**
 * Type-safe globalThis property access.
 * @public
 */
export function getGlobal(key) {
    return globalThis[key];
}
/**
 * Sets a globalThis property as a polyfill (only if undefined/null).
 * @public
 */
export function setGlobalPolyfill(key, value) {
    ;
    globalThis[key] = globalThis[key] ?? value;
}
