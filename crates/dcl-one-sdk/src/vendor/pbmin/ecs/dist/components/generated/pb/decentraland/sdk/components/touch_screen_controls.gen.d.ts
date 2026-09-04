import _m0 from "protobufjs/minimal";
import { TextureUnion } from "../../common/texture.gen";
import { InputAction } from "./common/input_action.gen";
/**
 * The TouchScreenControls component lets a scene configure the native on-screen touch
 * controls (the mobile joystick + gamepad). It must be set on the RootEntity.
 *
 * By default every on-screen button is shown; list a button in `touch_inputs` with
 * `hide = true` to remove it (declutter). `main_action` picks which action the large
 * central button triggers, and `hide_joystick` removes the native virtual joystick. It is
 * a no-op on platforms without native on-screen controls (e.g. desktop).
 *
 * Accepted actions: only the on-screen gamepad actions map to a button — `IA_POINTER`,
 * `IA_PRIMARY`, `IA_SECONDARY`, `IA_JUMP`, and `IA_ACTION_3`..`IA_ACTION_6`. Any other
 * `InputAction` (movement actions, `IA_ANY`, `IA_MODIFIER`, or unknown/future values) is
 * ignored: a `TouchInput` entry naming a non-button action has no effect, and a `main_action`
 * that isn't a valid gamepad action falls back to the default central button (`IA_JUMP`).
 */
/**
 * @public
 */
export interface PBTouchScreenControls {
    touchInputs: PBTouchScreenControls_TouchInput[];
    /**
     * The large central button's action. Only the gamepad actions are valid:
     * jump / pointer / primary (E) / secondary (F) / action_3..action_6 (1/2/3/4).
     * When unset, the default central button (jump) is kept.
     */
    mainAction?: InputAction | undefined;
    /** hide the native virtual joystick */
    hideJoystick: boolean;
    /** hide the on-screen crosshair / reticle */
    hideCrosshair: boolean;
}
/** Per-button configuration. A button not listed here keeps its default (shown). */
/**
 * @public
 */
export interface PBTouchScreenControls_TouchInput {
    /** which on-screen button this configures */
    inputAction: InputAction;
    /** hide this button (default: shown) */
    hide: boolean;
    /**
     * Override the button glyph with this texture. For the jump button it replaces all
     * of its dynamic states (jump / double-jump / glide).
     */
    icon?: TextureUnion | undefined;
}
/**
 * @public
 */
export declare namespace PBTouchScreenControls {
    function encode(message: PBTouchScreenControls, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTouchScreenControls;
}
/**
 * @public
 */
export declare namespace PBTouchScreenControls_TouchInput {
    function encode(message: PBTouchScreenControls_TouchInput, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBTouchScreenControls_TouchInput;
}
