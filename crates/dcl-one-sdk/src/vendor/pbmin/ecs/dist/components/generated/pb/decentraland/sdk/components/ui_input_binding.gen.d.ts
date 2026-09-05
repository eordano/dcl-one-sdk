import _m0 from "protobufjs/minimal";
import { InputAction } from "./common/input_action.gen";
/**
 * The UiInputBinding component binds a UI entity to one or more InputActions. While the
 * element is pressed (touch or pointer) the listed actions are held down, driving both
 * the local player input and scene InputAction listeners, just like the native on-screen
 * buttons. It is typically combined with PBTouchScreenControls to replace the native
 * controls with a custom touch UI.
 *
 * Release semantics: the held actions are released as soon as the press that started them
 * ends. A renderer MUST release all actions held by this binding when any of the following
 * happens: the press/touch is lifted or cancelled, the pointer/touch leaves the element
 * (loses press ownership), the actions list changes (the previous set is released before
 * the new set is applied), the component is removed or its actions list becomes empty, the
 * UI element is hidden, disabled or removed from the tree, or the scene unloads. In short,
 * no action may remain held once the element is no longer both present and actively pressed.
 *
 * Multi-touch: the binding is a single held state, not reference-counted per pointer. The
 * actions are held while the element is pressed and released when that press ends; a second
 * simultaneous press on the same element does not stack, and does not extend the hold past
 * the first release.
 */
/**
 * @public
 */
export interface PBUiInputBinding {
    /** the input actions fired while this element is pressed */
    actions: InputAction[];
}
/**
 * @public
 */
export declare namespace PBUiInputBinding {
    function encode(message: PBUiInputBinding, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBUiInputBinding;
}
