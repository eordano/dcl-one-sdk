import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export declare const enum TriggerAreaMeshType {
    TAMT_BOX = 0,
    TAMT_SPHERE = 1
}
/**
 * The PBTriggerArea component is used to raise collision triggering events (through the TriggerAreaResult component)
 * when entities enter this component's defined area.
 *
 * ADR: https://github.com/decentraland/adr/blob/2b30a5e2b4f359a7c22a68fb827db282f6e5f887/content/ADR-258-trigger-areas.md
 *
 * The area size and rotation is defined by the Entity's Transform.
 */
/**
 * @public
 */
export interface PBTriggerArea {
    /** default: MT_BOX */
    mesh?: TriggerAreaMeshType | undefined;
    /** default: CL_PLAYER */
    collisionMask?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBTriggerArea {
    function encode(message: PBTriggerArea, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTriggerArea;
}
