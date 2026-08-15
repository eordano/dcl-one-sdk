/**
 * Lightweight Vector3 math utilities for internal use.
 * Mirrors the subset of @dcl/ecs-math Vector3 API used by the physics helpers.
 *
 * TEMPORARY WORKAROUND: @dcl/ecs-math ships ESM-only, which breaks the dist-cjs build.
 * The proper fix is to add a CJS build to @dcl/ecs-math upstream, then replace this
 * file with `import { Vector3 } from '@dcl/ecs-math'`.
 *
 * @internal
 */
export const Vector3 = {
    add(a, b) {
        return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z };
    },
    subtract(a, b) {
        return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z };
    },
    scale(v, s) {
        return { x: v.x * s, y: v.y * s, z: v.z * s };
    },
    length(v) {
        return Math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    },
    normalize(v) {
        const len = Vector3.length(v);
        if (len === 0)
            return { x: 0, y: 0, z: 0 };
        return { x: v.x / len, y: v.y / len, z: v.z / len };
    },
    equals(a, b) {
        return a.x === b.x && a.y === b.y && a.z === b.z;
    },
    equalsToFloats(v, x, y, z) {
        return v.x === x && v.y === y && v.z === z;
    }
};
