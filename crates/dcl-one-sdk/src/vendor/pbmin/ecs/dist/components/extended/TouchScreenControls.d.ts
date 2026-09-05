import { IEngine, LastWriteWinElementSetComponentDefinition } from '../../engine';
import { PBTouchScreenControls } from '../generated/index.gen';
import { InputAction } from '../generated/pb/decentraland/sdk/components/common/input_action.gen';
/**
 * @public
 * TouchScreenControls with convenience helpers. All helpers write the component onto the
 * RootEntity (where the client reads it) and merge with the current value.
 */
export interface TouchScreenControlsComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBTouchScreenControls> {
    /** Hide every on-screen gamepad button. */
    hideAll(): void;
    /**
     * Show every on-screen gamepad button (clears the button hide list).
     * Does NOT change the joystick/crosshair — use `showJoystick()` / `showCrosshair()` for those.
     */
    showAll(): void;
    /** Hide the given on-screen buttons (merged into the current config). */
    hide(actions: InputAction[]): void;
    /** Set which action the large central button triggers. */
    setMainAction(action: InputAction): void;
    /** Hide the native virtual joystick. */
    hideJoystick(): void;
    /** Show the native virtual joystick. */
    showJoystick(): void;
    /** Hide the on-screen crosshair / reticle. */
    hideCrosshair(): void;
    /** Show the on-screen crosshair / reticle. */
    showCrosshair(): void;
}
export declare function defineTouchScreenControlsComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): TouchScreenControlsComponentDefinitionExtended;
