"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createTimers = void 0;
/**
 * Creates a timer system bound to a specific engine instance.
 *
 * @param targetEngine - The engine instance to bind timers to
 * @returns A Timers object with setTimeout, clearTimeout, setInterval, and clearInterval methods
 *
 * @example
 * ```ts
 * import { Engine } from '@dcl/sdk/ecs'
 * import { createTimers } from '@dcl/sdk/ecs'
 *
 * const engine = Engine()
 * const timers = createTimers(engine)
 *
 * timers.setTimeout(() => console.log('done'), 1000)
 * ```
 *
 * @public
 */
function createTimers(targetEngine) {
    const timers = new Map();
    let timerIdCounter = 0;
    // While a timer's callback is running this holds the time already consumed this
    // frame before that timer's logical fire instant (`elapsedMs - residualMs`);
    // `null` otherwise. A timer armed from within the callback is seeded with the
    // negative of this, so its delay is measured from the parent's fire instant
    // rather than from the start of the frame.
    let armContext = null;
    function system(dt) {
        const elapsedMs = 1000 * dt;
        for (const [timerId, timerData] of timers) {
            timerData.accumulatedTime += elapsedMs;
            if (timerData.accumulatedTime < timerData.interval) {
                continue;
            }
            // Time elapsed past this timer's logical fire instant this frame.
            const residualMs = timerData.recurrent
                ? timerData.accumulatedTime % timerData.interval
                : timerData.accumulatedTime - timerData.interval;
            if (timerData.recurrent) {
                // Collapse any missed periods into a single callback, keep the remainder.
                timerData.accumulatedTime = residualMs;
            }
            else {
                timers.delete(timerId);
            }
            armContext = { accruedMs: elapsedMs - residualMs };
            timerData.callback();
            armContext = null;
        }
    }
    targetEngine.addSystem(system, Number.MAX_SAFE_INTEGER, '@dcl/ecs/timers');
    function addTimer(callback, interval, recurrent) {
        const timerId = timerIdCounter++;
        let accumulatedTime = 0;
        if (armContext) {
            // Armed from inside a firing callback: this timer is appended to the map
            // being iterated, so the loop will still add this frame's elapsed to it.
            // Inherit the residual so that `+= elapsedMs` nets to the true time
            // elapsed since the parent fired (phase-accurate). Each successive arming
            // starts one interval lower, so a re-arming chain terminates this frame.
            accumulatedTime = -armContext.accruedMs;
        }
        timers.set(timerId, { callback, interval, recurrent, accumulatedTime });
        return timerId;
    }
    return {
        setTimeout(callback, ms) {
            return addTimer(callback, ms, false);
        },
        clearTimeout(timerId) {
            timers.delete(timerId);
        },
        setInterval(callback, ms) {
            return addTimer(callback, ms, true);
        },
        clearInterval(timerId) {
            timers.delete(timerId);
        }
    };
}
exports.createTimers = createTimers;
