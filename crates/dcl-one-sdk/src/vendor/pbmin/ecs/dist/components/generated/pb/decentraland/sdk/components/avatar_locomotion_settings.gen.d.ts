import _m0 from "protobufjs/minimal";
/**
 * The PBAvatarLocomotionSettings component allows scenes to modify locomotion settings defining things such
 * as the avatar movement speed, jump height etc.
 */
/**
 * @public
 */
export interface PBAvatarLocomotionSettings {
    /** Maximum speed when walking (in meters per second) */
    walkSpeed?: number | undefined;
    /** Maximum speed when jogging (in meters per second) */
    jogSpeed?: number | undefined;
    /** Maximum speed when running (in meters per second) */
    runSpeed?: number | undefined;
    /** Height of a regular jump (in meters) */
    jumpHeight?: number | undefined;
    /** Height of a jump while running (in meters) */
    runJumpHeight?: number | undefined;
    /** Cooldown time after a hard landing before the avatar can move again (in seconds) */
    hardLandingCooldown?: number | undefined;
    /** Height of the double jump (in meters) */
    doubleJumpHeight?: number | undefined;
    /** Maximum speed when gliding (in meters per second) */
    glidingSpeed?: number | undefined;
    /** Maximum falling speed when gliding (in meters per second) */
    glidingFallingSpeed?: number | undefined;
}
/**
 * @public
 */
export declare namespace PBAvatarLocomotionSettings {
    function encode(message: PBAvatarLocomotionSettings, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAvatarLocomotionSettings;
}
