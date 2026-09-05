import _m0 from "protobufjs/minimal";
import { ExplorerUi } from "./common/explorer_ui.gen";
/**
 * PBExplorerUiEventsResult transports the lifecycle events of fullscreen explorer panels — a panel was
 * opened, a panel was closed. It is a grow only value set appended to the scene root entity, so every
 * event of a tick is delivered and none overwrites another.
 */
/**
 * @public
 */
export interface PBExplorerUiEventsResult {
    /** The panel that the event refers to */
    ui: ExplorerUi;
    /** The scene tick when the event occurred */
    timestamp: number;
    event?: {
        $case: "opened";
        opened: PBExplorerUiEventsResult_UiOpened;
    } | {
        $case: "closed";
        closed: PBExplorerUiEventsResult_UiClosed;
    } | undefined;
}
/** Emitted when a fullscreen explorer panel is opened. */
/**
 * @public
 */
export interface PBExplorerUiEventsResult_UiOpened {
}
/** Emitted when a fullscreen explorer panel is closed. */
/**
 * @public
 */
export interface PBExplorerUiEventsResult_UiClosed {
}
/**
 * @public
 */
export declare namespace PBExplorerUiEventsResult {
    function encode(message: PBExplorerUiEventsResult, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBExplorerUiEventsResult;
}
/**
 * @public
 */
export declare namespace PBExplorerUiEventsResult_UiOpened {
    function encode(_: PBExplorerUiEventsResult_UiOpened, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBExplorerUiEventsResult_UiOpened;
}
/**
 * @public
 */
export declare namespace PBExplorerUiEventsResult_UiClosed {
    function encode(_: PBExplorerUiEventsResult_UiClosed, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBExplorerUiEventsResult_UiClosed;
}
