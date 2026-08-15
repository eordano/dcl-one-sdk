import _m0 from "protobufjs/minimal";
import { BorderRect } from "../../common/border_rect.gen";
/** This component is created by the renderer and used by the scenes to know the resolution of the UI canvas */
/**
 * @public
 */
export interface PBUiCanvasInformation {
    /** informs the scene about the resolution used for the UI rendering */
    devicePixelRatio: number;
    /** informs about the width of the canvas, in virtual pixels. this value does not change when the pixel ratio changes. */
    width: number;
    /** informs about the height of the canvas, in virtual pixels. this value does not change when the pixel ratio changes. */
    height: number;
    /**
     * informs the sdk about the interactable area. some implementations may change
     * this area depending on the HUD that is being shown. this value may change at
     * any time by the Renderer to create reactive UIs. as an example, an explorer with the
     * chat UI hidden and with no minimap could have a rect that covers the whole screen.
     * on the contrary, if the chat UI is shown, the rect would be smaller.
     */
    interactableArea: BorderRect | undefined;
    /**
     * informs the sdk about the screen inset area (safe margins). these are the
     * insets from each edge of the screen that are reserved by the device or
     * platform UI (for example: the notch, status bar, home indicator, or rounded
     * corners on mobile). scenes should avoid placing critical UI within these
     * insets to ensure it is not occluded. on desktop this is typically (0, 0, 0, 0).
     */
    screenInsetArea?: BorderRect | undefined;
}
/**
 * @public
 */
export declare namespace PBUiCanvasInformation {
    function encode(message: PBUiCanvasInformation, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBUiCanvasInformation;
}
