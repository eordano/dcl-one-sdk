import _m0 from "protobufjs/minimal";
import { Vector3 } from "../../common/vectors.gen";
/**
 * This component applies a one-shot physics summary impulse.
 *
 * @remarks Low-level component. Use Physics.applyImpulseToPlayer() instead.
 * Direct manipulation will conflict with the force accumulation registry.
 * Summary component: stores the accumulated result of all impulses registered by the scene in the current frame.
 *
 * Event-like component: each new impulse must increment the eventID to ensure delivery via CRDT, even if the direction is identical to the previous one.
 * Renderer processes impulse with the unique ID only once. Increase eventID of the component to apply another impulse.
 */
/**
 * @public
 */
export interface PBPhysicsCombinedImpulse {
    /** Includes impulse direction and magnitude */
    vector: Vector3 | undefined;
    /** Monotonic counter to distinguish different impulses. */
    eventId: number;
}
/**
 * @public
 */
export declare namespace PBPhysicsCombinedImpulse {
    function encode(message: PBPhysicsCombinedImpulse, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPhysicsCombinedImpulse;
}
