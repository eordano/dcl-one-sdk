import _m0 from "protobufjs/minimal";
import { AvatarMask } from "./common/avatar_mask.gen";
/** EmoteState describes the lifecycle state of an emote playback. */
/**
 * @public
 */
export declare const enum EmoteState {
    /**
     * ES_STARTED - ES_STARTED indicates the emote started playing.
     * This is the zero value and is used for backward compatibility — entries
     * written by older explorers (field absent) read as "started".
     */
    ES_STARTED = 0,
    /** ES_FINISHED - ES_FINISHED indicates a non-looping emote completed naturally. */
    ES_FINISHED = 1,
    /**
     * ES_INTERRUPTED - ES_INTERRUPTED indicates playback was cancelled (movement, teleport,
     * another emote superseding it, explicit stop, or scene change).
     */
    ES_INTERRUPTED = 2
}
/**
 * AvatarEmoteCommand is a grow only value set, written by the explorer to report
 * avatar emote playback to the scene. It is appended to every player entity in the
 * scene (the local player and remote avatars alike).
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
    /**
     * state describes the lifecycle event for this emote entry.
     * When absent (older explorers), defaults to ES_STARTED.
     */
    state?: EmoteState | undefined;
}
/**
 * @public
 */
export declare namespace PBAvatarEmoteCommand {
    function encode(message: PBAvatarEmoteCommand, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBAvatarEmoteCommand;
}
