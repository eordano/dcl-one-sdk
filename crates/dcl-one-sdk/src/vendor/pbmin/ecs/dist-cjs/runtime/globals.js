"use strict";
/**
 * Internal utilities for standardized globalThis access.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.setGlobalPolyfill = exports.getGlobal = void 0;
/**
 * Type-safe globalThis property access.
 * @public
 */
function getGlobal(key) {
    return globalThis[key];
}
exports.getGlobal = getGlobal;
/**
 * Sets a globalThis property as a polyfill (only if undefined/null).
 * @public
 */
function setGlobalPolyfill(key, value) {
    ;
    globalThis[key] = globalThis[key] ?? value;
}
exports.setGlobalPolyfill = setGlobalPolyfill;
