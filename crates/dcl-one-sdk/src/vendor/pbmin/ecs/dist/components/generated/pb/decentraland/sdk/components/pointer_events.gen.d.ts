import _m0 from "protobufjs/minimal";
import { InputAction, InteractionType, PointerEventType } from "./common/input_action.gen";
/**
 * PointerEvents adds configurable pointer-based interactions to the attached Entity.
 *
 * Events that match the criteria defined in the PointerEvents structure are reported back to the
 * Entity via the PointerEventsResult component.
 *
 * Some examples of events that can be detected:
 * - Pointer hovering over the Entity.
 * - Held mouse button released over the Entity.
 * - Controller button pressed while targeting the Entity.
 * - Key pressed while targeting the Entity, but only in close range.
 *
 * It also supports simple visual feedback when interactions occur, by showing floating text.
 * More sophisticated feedback requires the use of other components.
 *
 * Distance rules
 * --------------
 * PointerEvents can enforce interaction range using two independent distance checks:
 *
 * - Camera distance (`max_distance`): distance from the active camera to the target entity.
 * - Player distance (`max_player_distance`): distance from the avatar/player position to the target entity.
 *
 * How the interaction checks are combined:
 *
 * 1) Only `max_distance` is present
 *    - The interaction is allowed only if the camera distance is <= `max_distance`.
 *
 * 2) Only `max_player_distance` is present
 *    - The interaction is allowed only if the player distance is <= `max_player_distance`.
 *
 * 3) Both `max_distance` and `max_player_distance` are present
 *    - The interaction is allowed if ANY of the checks passes (OR logic):
 *      (camera distance <= `max_distance`) OR (player distance <= `max_player_distance`).
 *
 * 4) Neither `max_distance` nor `max_player_distance` is present
 *    - The system behaves as if `max_distance` were set to its default value (10),
 *      i.e., it uses the camera distance check with a threshold of 10.
 */
/**
 * @public
 */
export interface PBPointerEvents {
    /** the list of relevant events to detect */
    pointerEvents: PBPointerEvents_Entry[];
}
/**
 * @public
 */
export interface PBPointerEvents_Info {
    /** key/button in use (default IA_ANY) */
    button?: InputAction | undefined;
    /** feedback on hover (default 'Interact') */
    hoverText?: string | undefined;
    /** range of interaction (default 10) */
    maxDistance?: number | undefined;
    /** enable or disable hover text and highlight (default true) */
    showFeedback?: boolean | undefined;
    /** enable or disable hover highlight (default true) */
    showHighlight?: boolean | undefined;
    /** range of interaction from the avatar's position (default 0) */
    maxPlayerDistance?: number | undefined;
    /** resolution order when multiple events overlap, higher wins (default 0) */
    priority?: number | undefined;
}
/**
 * @public
 */
export interface PBPointerEvents_Entry {
    /** the kind of interaction to detect */
    eventType: PointerEventType;
    /** additional configuration for this detection */
    eventInfo: PBPointerEvents_Info | undefined;
    /** the type of interaction source (default 0 == CURSOR) */
    interactionType?: InteractionType | undefined;
}
/**
 * @public
 */
export declare namespace PBPointerEvents {
    function encode(message: PBPointerEvents, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPointerEvents;
}
/**
 * @public
 */
export declare namespace PBPointerEvents_Info {
    function encode(message: PBPointerEvents_Info, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPointerEvents_Info;
}
/**
 * @public
 */
export declare namespace PBPointerEvents_Entry {
    function encode(message: PBPointerEvents_Entry, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBPointerEvents_Entry;
}
