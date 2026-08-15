import _m0 from "protobufjs/minimal";
/**
 * BillboardMode indicates one or more axis for automatic rotation, in OR-able bit flag form.
 * Only the values below and the (BM_X | BM_Y) combination are valid.
 */
/**
 * @public
 */
export declare const enum BillboardMode {
    BM_NONE = 0,
    BM_X = 1,
    BM_Y = 2,
    BM_Z = 4,
    /** BM_ALL - bitwise combination BM_X | BM_Y | BM_Z */
    BM_ALL = 7
}
/**
 * The Billboard component makes an Entity automatically reorient its rotation to face a target.
 * By default (when target_entity is unset), the billboard faces the main camera. When target_entity
 * is set, the billboard faces that entity instead. Setting target_entity to the camera reserved
 * entity (2) is equivalent to leaving the field unset. If the referenced target entity doesn’t exist
 * or is deleted, the billboard reorientation is disabled until the target exists again.
 *
 * As the name indicates, it’s used to display in-game billboards and frequently combined with
 * the TextShape component.
 *
 * Billboard only affects the Entity’s rotation. Its scale and position are still determined by its
 * Transform.
 */
/**
 * @public
 */
export interface PBBillboard {
    /** the BillboardMode (default: BM_ALL) */
    billboardMode?: BillboardMode | undefined;
    /** entity to face instead of the camera; if the referenced entity doesn’t exist, the billboard behavior is disabled until it does */
    targetEntity?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBBillboard {
    function encode(message: PBBillboard, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBBillboard;
}
