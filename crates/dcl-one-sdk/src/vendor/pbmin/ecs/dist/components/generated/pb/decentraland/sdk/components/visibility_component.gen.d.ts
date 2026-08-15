import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export interface PBVisibilityComponent {
    /** default=true */
    visible?: boolean | undefined;
    /**
     * Propagation follows certain rules:
     * - Any own visibility component takes priority
     * - If no own visibility component, visibility is determined by visibility of the nearest parent with propagate = true
     * - If no own component and no parent with propagate = true, visibility is "visible"
     * - Visibility is always updated, whenever hierarchy, parent component or own component changes
     */
    propagateToChildren?: boolean | undefined;
}
/**
 * @public
 */
export declare namespace PBVisibilityComponent {
    function encode(message: PBVisibilityComponent, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBVisibilityComponent;
}
