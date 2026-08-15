import _m0 from "protobufjs/minimal";
import { Vector2, Vector3 } from "../../common/vectors.gen";
/**
 * PointerType enumerates the different input devices that can be used for pointer interactions.
 * Each type has specific characteristics and use cases in the virtual world.
 */
/**
 * @public
 */
export declare const enum PointerType {
    /** POT_NONE - No pointer input */
    POT_NONE = 0,
    /** POT_MOUSE - Traditional mouse input */
    POT_MOUSE = 1
}
/**
 * The PBPrimaryPointerInfo component provides information about the current state of the primary
 * pointer input device (mouse, touch, gamepad, or VR controller). It tracks the pointer's position,
 * movement, and interaction capabilities in both 2D screen space and 3D world space.
 *
 * This component is essential for:
 * - Tracking cursor/pointer position and movement
 * - Converting 2D screen coordinates to 3D world space interactions
 * - Supporting multiple input methods (mouse, touch, gamepad, VR)
 * - Enabling ray-based interactions for 3D object selection
 *
 * The component can be used to:
 * - Implement drag-and-drop functionality
 * - Handle touch interactions on mobile devices
 * - Support gamepad navigation
 * - Enable VR controller interactions
 * - Convert screen coordinates to world space rays for 3D interactions
 *
 * Note: Touch, Pad, and Wand support, as well as dragging, will be added later.
 */
/**
 * @public
 */
export interface PBPrimaryPointerInfo {
    /** The type of input device being used */
    pointerType?: PointerType | undefined;
    /** Current position in screen space (pixels) */
    screenCoordinates?: Vector2 | undefined;
    /** Movement since last frame (pixels) */
    screenDelta?: Vector2 | undefined;
    /** Direction vector for 3D ray casting */
    worldRayDirection?: Vector3 | undefined;
}
/**
 * @public
 */
export declare namespace PBPrimaryPointerInfo {
    function encode(message: PBPrimaryPointerInfo, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPrimaryPointerInfo;
}
