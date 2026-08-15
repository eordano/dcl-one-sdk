import _m0 from "protobufjs/minimal";
import { AvatarMask } from "./common/avatar_mask.gen";
/**
 * AvatarEmoteCommand is a grow only value set, used to signal the renderer about
 * avatar emotes playback.
 */
/**
 * @public
 */
export interface PBAvatarEmoteCommand {
    emoteUrn: string;
    loop: boolean;
    /** monotonic counter */
    timestamp: number;
    mask?: AvatarMask | undefined;
}
/**
 * @public
 */
export declare namespace PBAvatarEmoteCommand {
    function encode(message: PBAvatarEmoteCommand, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAvatarEmoteCommand;
}
