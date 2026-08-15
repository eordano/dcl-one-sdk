/**
 * Internal utilities for standardized globalThis access.
 */
/**
 * Type-safe globalThis property access.
 * @public
 */
export declare function getGlobal<T>(key: string): T | undefined;
/**
 * Sets a globalThis property as a polyfill (only if undefined/null).
 * @public
 */
export declare function setGlobalPolyfill<T>(key: string, value: T): void;
