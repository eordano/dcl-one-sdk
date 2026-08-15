import _m0 from "protobufjs/minimal";
import { Vector3 } from "../../common/vectors.gen";
/**
 * This component applies a continuous physics force.
 *
 * @remarks Low-level component. Use Physics.applyForceToPlayer()/.removeForceToPlayer() instead.
 * Direct manipulation will conflict with the force accumulation registry.
 * Summary component: stores the accumulated result of all active forces registered by the scene in the current frame.
 *
 * State-like component: the force is applied every physics tick while the component is present on the entity.
 */
/**
 * @public
 */
export interface PBPhysicsCombinedForce {
    /** Includes force direction and magnitude */
    vector: Vector3 | undefined;
}
/**
 * @public
 */
export declare namespace PBPhysicsCombinedForce {
    function encode(message: PBPhysicsCombinedForce, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPhysicsCombinedForce;
}
